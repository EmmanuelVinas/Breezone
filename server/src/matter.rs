//! Breezone as a Matter bridge (ADR 0015), first step: the lights of Breezone, on and off, for the
//! Matter controllers of the home (Google Home and its Nest Hub, Apple Home, Alexa, SmartThings),
//! in local, with no cloud between them and Breezone.
//!
//! The Matter stack (`rs-matter`) runs on a thread of its own with a simple executor; it reads and
//! drives the devices through the same functions as the web and the app (`devices::list`,
//! `devices::apply`), on the server's runtime:
//!
//! * endpoint 0: the root of the node (commissioning, fabrics, network);
//! * endpoint 1: the aggregator, which tells the controllers the bridged devices behind it;
//! * endpoints 2…: one per light of Breezone (On/Off Light), at most [`SLOTS`] for now.
//!
//! On by `BREEZONE_MATTER=1`. The node is not commissioned at first: its QR code and pairing code
//! are written in the log, to add in the controller's app. Its fabrics live in `<data>/matter`.

use crate::devices::{self, ActionInput, DeviceView};
use crate::AppState;
use core::pin::pin;
use rs_matter::crypto::{default_crypto, Crypto};
use rs_matter::dm::clusters::app::level_control::LevelControlHooks;
use rs_matter::dm::clusters::app::on_off::{self, OnOffHooks, OutOfBandMessage, StartUpOnOffEnum};
use rs_matter::dm::clusters::basic_info::BasicInfoConfig;
use rs_matter::dm::clusters::decl::bridged_device_basic_information::{self, ClusterHandler as _, KeepActiveRequest};
use rs_matter::dm::clusters::decl::on_off as on_off_cluster;
use rs_matter::dm::clusters::desc::{self, ClusterHandler as _};
use rs_matter::dm::clusters::groups::{self, ClusterHandler as _};
use rs_matter::dm::devices::test::{DAC_PRIVKEY, TEST_DEV_ATT, TEST_DEV_COMM, TEST_PID, TEST_VID};
use rs_matter::dm::devices::{DEV_TYPE_AGGREGATOR, DEV_TYPE_BRIDGED_NODE, DEV_TYPE_ON_OFF_LIGHT};
use rs_matter::dm::networks::eth::EthNetwork;
use rs_matter::dm::{Async, Cluster, DataModel, Dataver, Endpoint, InvokeContext, Node, ReadContext};
use rs_matter::error::Error;
use rs_matter::im::{EthInteractionModelState, InteractionModel};
use rs_matter::pairing::qr::QrTextType;
use rs_matter::pairing::DiscoveryCapabilities;
use rs_matter::persist::DirKvBlobStore;
use rs_matter::respond::DefaultResponder;
use rs_matter::sc::pase::MAX_COMM_WINDOW_TIMEOUT_SECS;
use rs_matter::tlv::{Nullable, TLVBuilderParent, Utf8StrBuilder};
use rs_matter::transport::exchange::MatterBuffers;
use rs_matter::utils::select::Coalesce;
use rs_matter::{clusters, devices as matter_devices, root_endpoint, with, Matter};
use serde_json::{json, Map};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc;

/// The lights a bridge offers at most, for now (each is a fixed endpoint of the node).
pub const SLOTS: usize = 8;

/// The first endpoint of a bridged light.
const FIRST_ENDPOINT: u16 = 2;

/// A light of Breezone seen by Matter: its last state, kept up to date by the server, and the
/// commands of the controllers, sent to the server.
pub struct Light {
    /// The Breezone device (0: the slot is free).
    device: AtomicI64,
    name: Mutex<String>,
    on: AtomicBool,
    reachable: AtomicBool,
    start_up: Mutex<Option<StartUpOnOffEnum>>,
    commands: mpsc::UnboundedSender<(i64, bool)>,
}

impl Light {
    fn new(commands: mpsc::UnboundedSender<(i64, bool)>) -> Self {
        Self { device: AtomicI64::new(0), name: Mutex::new(String::new()), on: AtomicBool::new(false), reachable: AtomicBool::new(true), start_up: Mutex::new(None), commands }
    }
}

impl OnOffHooks for &Light {
    // The On/Off Light device type asks for the Lighting feature of the cluster, as rs-matter's own
    // test device declares it.
    const CLUSTER: Cluster<'static> = on_off_cluster::FULL_CLUSTER
        .with_revision(6)
        .with_features(on_off_cluster::Feature::LIGHTING.bits())
        .with_attrs(with!(
            required;
            on_off_cluster::AttributeId::OnOff
                | on_off_cluster::AttributeId::GlobalSceneControl
                | on_off_cluster::AttributeId::OnTime
                | on_off_cluster::AttributeId::OffWaitTime
                | on_off_cluster::AttributeId::StartUpOnOff
        ))
        .with_cmds(with!(
            on_off_cluster::CommandId::Off
                | on_off_cluster::CommandId::On
                | on_off_cluster::CommandId::Toggle
                | on_off_cluster::CommandId::OffWithEffect
                | on_off_cluster::CommandId::OnWithRecallGlobalScene
                | on_off_cluster::CommandId::OnWithTimedOff
        ));

    fn on_off(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// A controller switches the light: Breezone does it.
    fn set_on_off(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
        let device = self.device.load(Ordering::Relaxed);
        if device != 0 {
            let _ = self.commands.send((device, on));
        }
    }

    fn start_up_on_off(&self) -> Nullable<StartUpOnOffEnum> {
        match *self.start_up.lock().unwrap_or_else(|e| e.into_inner()) {
            Some(v) => Nullable::some(v),
            None => Nullable::none(),
        }
    }

    fn set_start_up_on_off(&self, value: Nullable<StartUpOnOffEnum>) -> Result<(), Error> {
        *self.start_up.lock().unwrap_or_else(|e| e.into_inner()) = value.into_option();
        Ok(())
    }

    async fn handle_off_with_effect(&self, _effect: on_off::EffectVariantEnum) {}

    /// The light changed in Breezone (the app, an automation, its own switch): the controllers are
    /// told, at most half a second later.
    async fn run<F: Fn(OutOfBandMessage)>(&self, notify: F) {
        let mut last = self.on.load(Ordering::Relaxed);
        loop {
            async_io::Timer::after(Duration::from_millis(500)).await;
            let now = self.on.load(Ordering::Relaxed);
            if now != last {
                last = now;
                notify(OutOfBandMessage::Update);
            }
        }
    }
}

/// The bridged-device information of a light: its name in Breezone, a stable identity, whether it
/// answers.
#[derive(Clone)]
struct Bridged {
    dataver: Dataver,
    light: &'static Light,
}

impl bridged_device_basic_information::ClusterHandler for Bridged {
    const CLUSTER: Cluster<'static> = bridged_device_basic_information::FULL_CLUSTER
        .with_features(0)
        .with_attrs(with!(required; bridged_device_basic_information::AttributeId::NodeLabel | bridged_device_basic_information::AttributeId::UniqueID))
        .with_cmds(with!());

    fn dataver(&self) -> u32 {
        self.dataver.get()
    }

    fn dataver_changed(&self) {
        self.dataver.changed();
    }

    fn reachable(&self, _ctx: impl ReadContext) -> Result<bool, Error> {
        Ok(self.light.reachable.load(Ordering::Relaxed))
    }

    fn node_label<P: TLVBuilderParent>(&self, _ctx: impl ReadContext, builder: Utf8StrBuilder<P>) -> Result<P, Error> {
        let name = self.light.name.lock().unwrap_or_else(|e| e.into_inner()).clone();
        // At most 32 bytes, on a character boundary.
        let mut end = name.len().min(32);
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        builder.set(&name[..end])
    }

    fn unique_id<P: TLVBuilderParent>(&self, _ctx: impl ReadContext, builder: Utf8StrBuilder<P>) -> Result<P, Error> {
        builder.set(&format!("breezone-device-{}", self.light.device.load(Ordering::Relaxed)))
    }

    fn handle_keep_active(&self, _ctx: impl InvokeContext, _request: KeepActiveRequest<'_>) -> Result<(), Error> {
        Ok(())
    }
}

/// The identity of the node: Breezone, with the test vendor and product of the Matter standard
/// (a certified identity is a later decision, ADR 0015).
const DETAILS: BasicInfoConfig<'static> = BasicInfoConfig {
    vid: TEST_VID,
    pid: TEST_PID,
    hw_ver: 1,
    hw_ver_str: "1",
    sw_ver: 1,
    sw_ver_str: env!("CARGO_PKG_VERSION"),
    serial_no: "breezone",
    manufacturing_date: "20261120",
    product_name: "Breezone",
    vendor_name: "Breezone",
    device_name: "Breezone",
    ..BasicInfoConfig::new()
};

/// The root of the node, and the aggregator of the bridged devices.
const ROOT: Endpoint<'static> = root_endpoint!(eth);
const AGGREGATOR: Endpoint<'static> = Endpoint::new(1, matter_devices!(DEV_TYPE_AGGREGATOR), clusters!(desc::DescHandler::CLUSTER));

/// The endpoint of a bridged light.
macro_rules! light_endpoint {
    ($id:literal) => {
        Endpoint::new(
            $id,
            matter_devices!(DEV_TYPE_ON_OFF_LIGHT, DEV_TYPE_BRIDGED_NODE),
            clusters!(desc::DescHandler::CLUSTER, groups::GroupsHandler::CLUSTER, Bridged::CLUSTER, <&Light as OnOffHooks>::CLUSTER),
        )
    };
}

/// The endpoints of the light slots (2 to 9).
const LIGHT_ENDPOINTS: [Endpoint<'static>; SLOTS] =
    [light_endpoint!(2), light_endpoint!(3), light_endpoint!(4), light_endpoint!(5), light_endpoint!(6), light_endpoint!(7), light_endpoint!(8), light_endpoint!(9)];

/// The lights of Breezone that are offered: on/off, in the order of their ids.
fn offered(devices: &[DeviceView]) -> Vec<&DeviceView> {
    let ids = pick(devices.iter().map(|d| (d.id, d.traits.as_slice())));
    ids.iter().filter_map(|id| devices.iter().find(|d| d.id == *id)).collect()
}

/// The ids of the lights among `(id, traits)`, in order, at most [`SLOTS`].
fn pick<'a>(devices: impl Iterator<Item = (i64, &'a [String])>) -> Vec<i64> {
    let mut ids: Vec<i64> = devices.filter(|(_, traits)| traits.iter().any(|t| t == "light@1")).map(|(id, _)| id).collect();
    ids.sort_unstable();
    ids.truncate(SLOTS);
    ids
}

/// Starts the bridge, when asked (`BREEZONE_MATTER`): the lights known now are its endpoints.
pub async fn spawn(state: AppState) {
    if !state.cfg.matter {
        return;
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<(i64, bool)>();
    let lights: &'static [Light; SLOTS] = Box::leak(Box::new(std::array::from_fn(|_| Light::new(tx.clone()))));
    let list = devices::list(&state).await.unwrap_or_default();
    let chosen = offered(&list);
    for (slot, d) in chosen.iter().enumerate() {
        lights[slot].device.store(d.id, Ordering::Relaxed);
        *lights[slot].name.lock().unwrap_or_else(|e| e.into_inner()) = d.name.clone();
        lights[slot].on.store(d.state.get("on").and_then(|v| v.as_bool()).unwrap_or(false), Ordering::Relaxed);
    }
    tracing::info!("Matter bridge: {} light(s) offered", chosen.len());
    let count = chosen.len();

    // The controllers' commands, done by Breezone.
    let commands = state.clone();
    tokio::spawn(async move {
        while let Some((id, on)) = rx.recv().await {
            let Ok(Some(view)) = devices::get(&commands, id).await else { continue };
            let input = ActionInput { trait_name: "light".into(), action: "set".into(), params: Map::from_iter([("on".to_string(), json!(on))]) };
            if let Err(e) = devices::apply(&commands, &view, &input, "Matter").await {
                tracing::warn!("Matter: « {} » not switched: {}", view.name, e.message.en);
            }
        }
    });

    // The state of the lights, followed for the controllers.
    let follow = state.clone();
    tokio::spawn(async move {
        loop {
            if let Ok(list) = devices::list(&follow).await {
                for light in lights.iter() {
                    let id = light.device.load(Ordering::Relaxed);
                    if id == 0 {
                        continue;
                    }
                    let Some(d) = list.iter().find(|d| d.id == id) else { continue };
                    if let Some(on) = d.state.get("on").and_then(|v| v.as_bool()) {
                        light.on.store(on, Ordering::Relaxed);
                    }
                    light.reachable.store(d.available && d.state.get("reachable").and_then(|v| v.as_bool()) != Some(false), Ordering::Relaxed);
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });

    let dir = state.cfg.data_dir.join("matter");
    let port = state.cfg.matter_port;
    let spawned = std::thread::Builder::new().name("matter".into()).spawn(move || {
        if let Err(e) = run(lights, count, dir, port) {
            tracing::error!("Matter bridge stopped: {e:?}");
        }
    });
    if let Err(e) = spawned {
        tracing::error!("Matter bridge not started: {e}");
    }
}

/// The Matter node, on the thread of the bridge.
fn run(lights: &'static [Light; SLOTS], count: usize, dir: std::path::PathBuf, port: u16) -> Result<(), Error> {
    std::fs::create_dir_all(&dir).map_err(|_| rs_matter::error::ErrorCode::StdIoError)?;
    let matter = Matter::new(&DETAILS, TEST_DEV_COMM, &TEST_DEV_ATT, port);
    let store = DirKvBlobStore::new(dir);
    let buffers: MatterBuffers = MatterBuffers::new();
    let state: EthInteractionModelState = EthInteractionModelState::new(EthNetwork::new_default());
    let kv = matter.kv(store);
    matter.startup(&kv)?;
    let crypto = default_crypto(rand::rng(), DAC_PRIVKEY);
    let mut rand = crypto.rand()?;

    // The endpoints of the node: the root, the aggregator, one per light offered.
    let mut endpoints = vec![ROOT, AGGREGATOR];
    endpoints.extend_from_slice(&LIGHT_ENDPOINTS[..count]);
    let node: Node<'static> = Node { endpoints: Box::leak(endpoints.into_boxed_slice()) };

    let handlers: Vec<on_off::OnOffHandler<'static, &'static Light, on_off::NoLevelControl>> =
        lights.iter().enumerate().map(|(slot, l)| on_off::OnOffHandler::new_standalone(Dataver::new_rand(&mut rand), FIRST_ENDPOINT + slot as u16, l)).collect();
    let handlers: &'static [on_off::OnOffHandler<'static, &'static Light, on_off::NoLevelControl>] = Box::leak(handlers.into_boxed_slice());

    let im = InteractionModel::new(&matter, &crypto, &buffers, data_model(node, rand, lights, handlers), &kv, &state);
    futures_lite::future::block_on(im.startup())?;
    let responder = DefaultResponder::new(&im);
    let mut respond = pin!(responder.run::<4, 4>());
    let mut im_job = pin!(im.run());
    let socket = async_io::Async::<UdpSocket>::bind(std::net::SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, port)))?;
    let mut mdns = pin!(mdns::run(&matter, &crypto));
    let mut transport = pin!(matter.run(&crypto, &socket, &socket, &socket));

    if !matter.has_fabrics() {
        // Not added to any controller yet: the codes to add it, in the log.
        matter.print_standard_qr_text(DiscoveryCapabilities::IP)?;
        matter.print_standard_qr_code(QrTextType::Unicode, DiscoveryCapabilities::IP)?;
        matter.open_basic_comm_window(MAX_COMM_WINDOW_TIMEOUT_SECS, &crypto, &())?;
    }
    let all = embassy_futures::select::select4(&mut transport, &mut mdns, &mut respond, &mut im_job).coalesce();
    futures_lite::future::block_on(all)
}

/// The handlers of the node: those of the root, of the aggregator, and of each light slot.
fn data_model<'a, H: OnOffHooks, L: LevelControlHooks>(
    node: Node<'static>,
    mut rand: impl rand::Rng + Copy,
    lights: &'static [Light; SLOTS],
    on_off: &'a [on_off::OnOffHandler<'a, H, L>],
) -> impl DataModel + 'a {
    // The matchers are plain functions: the endpoint of each slot is written out.
    macro_rules! slot {
        ($chain:expr, $i:expr, $ep:literal) => {{
            $chain
                .chain(|e, c| e == $ep && c == desc::DescHandler::CLUSTER.id, Async(desc::DescHandler::new(Dataver::new_rand(&mut rand)).adapt()))
                .chain(|e, c| e == $ep && c == groups::GroupsHandler::CLUSTER.id, Async(groups::GroupsHandler::new(Dataver::new_rand(&mut rand)).adapt()))
                .chain(|e, c| e == $ep && c == <H as OnOffHooks>::CLUSTER.id, on_off::HandlerAsyncAdaptor(&on_off[$i]))
                .chain(
                    |e, c| e == $ep && c == Bridged::CLUSTER.id,
                    Async(bridged_device_basic_information::HandlerAdaptor(Bridged { dataver: Dataver::new_rand(&mut rand), light: &lights[$i] })),
                )
        }};
    }
    let chain = rs_matter::dm::endpoints::EthSysHandlerBuilder::new()
        .netif_diag(&rs_matter::dm::networks::SysNetifs)
        .build(rand)
        .chain(|e, c| e == 1 && c == desc::DescHandler::CLUSTER.id, Async(desc::DescHandler::new_aggregator(Dataver::new_rand(&mut rand)).adapt()));
    let chain = slot!(chain, 0, 2);
    let chain = slot!(chain, 1, 3);
    let chain = slot!(chain, 2, 4);
    let chain = slot!(chain, 3, 5);
    let chain = slot!(chain, 4, 6);
    let chain = slot!(chain, 5, 7);
    let chain = slot!(chain, 6, 8);
    let chain = slot!(chain, 7, 9);
    (node, chain)
}

/// The Matter service announced on the network: by Bonjour on a Mac (development), else by the
/// stack's own mDNS on port 5353, shared with the host's (Avahi on a NAS).
mod mdns {
    use rs_matter::crypto::Crypto;
    use rs_matter::error::Error;
    use rs_matter::Matter;

    #[cfg(target_os = "macos")]
    pub async fn run<C: Crypto>(matter: &Matter<'_>, _crypto: C) -> Result<(), Error> {
        rs_matter::transport::network::mdns::astro::AstroMdns::new().run(matter).await
    }

    #[cfg(not(target_os = "macos"))]
    pub async fn run<C: Crypto>(matter: &Matter<'_>, crypto: C) -> Result<(), Error> {
        use rs_matter::error::ErrorCode;
        use rs_matter::transport::network::mdns::builtin::{BuiltinMdns, Host};
        use rs_matter::transport::network::mdns::{MDNS_IPV4_BROADCAST_ADDR, MDNS_IPV6_BROADCAST_ADDR, MDNS_SOCKET_DEFAULT_BIND_ADDR};
        use socket2::{Domain, Protocol, Socket, Type};
        use std::net::UdpSocket;

        // The interface of the home network: one with an IPv4 address and an IPv6 one.
        let all = if_addrs::get_if_addrs().map_err(|_| ErrorCode::StdIoError)?;
        let (name, ipv4, index) = all
            .iter()
            .filter(|a| !a.is_loopback())
            .find_map(|a| match a.addr {
                if_addrs::IfAddr::V4(ref v4) if all.iter().any(|b| b.name == a.name && matches!(b.addr, if_addrs::IfAddr::V6(_))) => {
                    Some((a.name.clone(), v4.ip, a.index.unwrap_or(0)))
                }
                _ => None,
            })
            .ok_or(ErrorCode::StdIoError)?;
        let ipv6: Vec<rs_matter::transport::network::Ipv6Addr> = all
            .iter()
            .filter(|a| a.name == name)
            .filter_map(|a| match a.addr {
                if_addrs::IfAddr::V6(ref v6) => Some(v6.ip.octets().into()),
                _ => None,
            })
            .collect();
        tracing::info!("Matter: announced on {name} ({ipv4})");

        let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        socket.set_reuse_port(true)?;
        socket.set_only_v6(false)?;
        socket.bind(&MDNS_SOCKET_DEFAULT_BIND_ADDR.into())?;
        let socket = async_io::Async::<UdpSocket>::new_nonblocking(socket.into())?;
        socket.get_ref().join_multicast_v6(&MDNS_IPV6_BROADCAST_ADDR, index)?;
        socket.get_ref().join_multicast_v4(&MDNS_IPV4_BROADCAST_ADDR, &ipv4)?;
        let ipv4 = ipv4.octets().into();
        BuiltinMdns::new().run(&socket, &socket, &Host { hostname: "breezone", ip: ipv4, ipv6: &ipv6 }, Some(ipv4), Some(index), matter, crypto).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lights_are_offered_in_order_and_at_most_the_slots() {
        let light = vec!["light@1".to_string()];
        let relay = vec!["switch.relay@1".to_string()];
        let mut list: Vec<(i64, &[String])> = (1..=12).rev().map(|id| (id, light.as_slice())).collect();
        list.push((0, relay.as_slice()));
        assert_eq!(pick(list.into_iter()), (1..=SLOTS as i64).collect::<Vec<_>>());
    }
}
