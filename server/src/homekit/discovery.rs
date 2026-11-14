//! The HomeKit accessories of the local network, from mDNS (`_hap._tcp`).
//!
//! The question is asked from a port of our own, not 5353 (a "legacy unicast" query, RFC 6762,
//! 6.7): responders answer it directly to that port. It works next to the system's own responder
//! (mDNSResponder, Avahi on a NAS), which holds port 5353 and would get the answers otherwise.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;

const SERVICE: &str = "_hap._tcp.local";
const PTR: u16 = 12;
const TXT: u16 = 16;
const SRV: u16 = 33;
const A: u16 = 1;

/// An accessory as it announces itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Announced {
    /// Its name ("Module Control").
    pub name: String,
    /// Its pairing id ("A1:B8:D6:5C:39:A6"), stable.
    pub id: String,
    /// "ip:port".
    pub host: String,
    /// "Legrand Gateway".
    pub model: Option<String>,
    /// Paired with a controller already (`sf` 0).
    pub paired: bool,
    /// Category (2: bridge, 14: window covering…).
    pub category: Option<u32>,
    /// Feature flags: 1, Apple authentication (certified accessories).
    pub flags: u32,
}

fn query(names: &[(&str, u16)]) -> Vec<u8> {
    let mut out = vec![0, 0, 0, 0, 0, names.len() as u8, 0, 0, 0, 0, 0, 0];
    for (name, kind) in names {
        for label in name.split('.') {
            out.push(label.len() as u8);
            out.extend_from_slice(label.as_bytes());
        }
        out.push(0);
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
    }
    out
}

/// A name at `i` (labels, or a pointer to them), and where the bytes after it start.
fn name(msg: &[u8], mut i: usize) -> Option<(String, usize)> {
    let mut labels = vec![];
    let mut end = None;
    for _ in 0..64 {
        let len = *msg.get(i)? as usize;
        if len == 0 {
            return Some((labels.join("."), end.unwrap_or(i + 1)));
        }
        if len & 0xc0 == 0xc0 {
            let pointer = ((len & 0x3f) << 8) | *msg.get(i + 1)? as usize;
            end.get_or_insert(i + 2);
            i = pointer;
            continue;
        }
        labels.push(String::from_utf8_lossy(msg.get(i + 1..i + 1 + len)?).into_owned());
        i += 1 + len;
    }
    None
}

#[derive(Default)]
struct Records {
    /// Instances of the service.
    instances: Vec<String>,
    /// Instance → (host name, port).
    srv: HashMap<String, (String, u16)>,
    /// Instance → TXT keys.
    txt: HashMap<String, HashMap<String, String>>,
    /// Host name → address.
    a: HashMap<String, Ipv4Addr>,
    /// Instance → the address that answered for it (when its own address is not given).
    via: HashMap<String, Ipv4Addr>,
    /// Who answered: the follow-up questions go to them too.
    responders: Vec<Ipv4Addr>,
}

fn read(msg: &[u8], records: &mut Records, from: Option<Ipv4Addr>) -> Option<()> {
    if let Some(ip) = from.filter(|ip| !records.responders.contains(ip)) {
        records.responders.push(ip);
    }
    let count = |at: usize| u16::from_be_bytes([msg[at], msg[at + 1]]) as usize;
    if msg.len() < 12 {
        return None;
    }
    let (questions, answers) = (count(4), count(6) + count(8) + count(10));
    let mut i = 12;
    for _ in 0..questions {
        i = name(msg, i)?.1 + 4;
    }
    for _ in 0..answers {
        let (owner, at) = name(msg, i)?;
        let kind = u16::from_be_bytes([*msg.get(at)?, *msg.get(at + 1)?]);
        let len = u16::from_be_bytes([*msg.get(at + 8)?, *msg.get(at + 9)?]) as usize;
        let data = at + 10;
        let rdata = msg.get(data..data + len)?;
        let owner_l = owner.to_ascii_lowercase();
        match kind {
            PTR if owner_l == SERVICE.to_ascii_lowercase() => {
                let instance = name(msg, data)?.0;
                if !records.instances.contains(&instance) {
                    records.instances.push(instance);
                }
            }
            SRV if rdata.len() >= 7 => {
                let port = u16::from_be_bytes([rdata[4], rdata[5]]);
                if let Some(ip) = from {
                    records.via.insert(owner_l.clone(), ip);
                }
                records.srv.insert(owner_l, (name(msg, data + 6)?.0.to_ascii_lowercase(), port));
            }
            TXT => {
                let mut keys = HashMap::new();
                let mut j = 0;
                while j < rdata.len() {
                    let l = rdata[j] as usize;
                    let entry = String::from_utf8_lossy(rdata.get(j + 1..j + 1 + l)?).into_owned();
                    if let Some((k, v)) = entry.split_once('=') {
                        keys.insert(k.to_ascii_lowercase(), v.to_string());
                    }
                    j += 1 + l;
                }
                records.txt.insert(owner_l, keys);
            }
            A if rdata.len() == 4 => {
                records.a.insert(owner_l, Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3]));
            }
            _ => {}
        }
        i = data + len;
    }
    Some(())
}

async fn listen(socket: &UdpSocket, within: Duration, records: &mut Records) {
    let deadline = tokio::time::Instant::now() + within;
    let mut buf = [0u8; 9000];
    while let Ok(Ok((n, from))) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await {
        let ip = match from.ip() {
            std::net::IpAddr::V4(ip) => Some(ip),
            _ => None,
        };
        let _ = read(&buf[..n], records, ip);
    }
}

/// A follow-up question, to the group and to everyone who answered.
async fn ask(socket: &UdpSocket, group: SocketAddr, records: &Records, questions: &[(&str, u16)]) {
    let q = query(questions);
    let _ = socket.send_to(&q, group).await;
    for ip in &records.responders {
        let _ = socket.send_to(&q, SocketAddr::from((*ip, 5353))).await;
    }
}

/// Asks the network for its HomeKit accessories, and waits `within` for the answers: the mDNS
/// group, and each address of `hosts` directly (it finds them where multicast does not go, as from
/// a Docker bridge network).
pub async fn browse(within: Duration, hosts: &[Ipv4Addr]) -> Vec<Announced> {
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await else { return vec![] };
    let group = SocketAddr::from((Ipv4Addr::new(224, 0, 0, 251), 5353));
    let mut records = Records::default();
    let question = query(&[(SERVICE, PTR)]);
    let _ = socket.send_to(&question, group).await;
    for host in hosts {
        let _ = socket.send_to(&question, SocketAddr::from((*host, 5353))).await;
    }
    listen(&socket, within, &mut records).await;
    // Some answer the service only: ask them their address and description.
    let missing: Vec<String> =
        records.instances.iter().filter(|i| !records.srv.contains_key(&i.to_ascii_lowercase()) || !records.txt.contains_key(&i.to_ascii_lowercase())).cloned().collect();
    if !missing.is_empty() {
        let questions: Vec<(&str, u16)> = missing.iter().flat_map(|i| [(i.as_str(), SRV), (i.as_str(), TXT)]).collect();
        ask(&socket, group, &records, &questions).await;
        listen(&socket, within / 2, &mut records).await;
    }
    let names: Vec<String> = records.srv.iter().filter(|(i, (h, _))| !records.a.contains_key(h) && !records.via.contains_key(*i)).map(|(_, (h, _))| h.clone()).collect();
    if !names.is_empty() {
        let questions: Vec<(&str, u16)> = names.iter().map(|h| (h.as_str(), A)).collect();
        ask(&socket, group, &records, &questions).await;
        listen(&socket, within / 2, &mut records).await;
    }
    let mut out: Vec<Announced> = records
        .instances
        .iter()
        .filter_map(|instance| {
            let key = instance.to_ascii_lowercase();
            let (host, port) = records.srv.get(&key)?;
            let ip = records.a.get(host).or_else(|| records.via.get(&key))?;
            let txt = records.txt.get(&key)?;
            Some(Announced {
                name: instance.strip_suffix(&format!(".{SERVICE}")).unwrap_or(instance).to_string(),
                id: txt.get("id")?.clone(),
                host: format!("{ip}:{port}"),
                model: txt.get("md").map(|m| m.trim_end_matches('\0').trim().to_string()),
                paired: txt.get("sf").is_some_and(|sf| sf.trim() == "0"),
                category: txt.get("ci").and_then(|c| c.trim().parse().ok()),
                flags: txt.get("ff").and_then(|f| f.trim().parse().ok()).unwrap_or(0),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer as a Legrand gateway gives it: the service, its address and description, with
    /// names compressed.
    fn answer() -> Vec<u8> {
        let mut m = vec![0, 0, 0x84, 0, 0, 0, 0, 4, 0, 0, 0, 0];
        // PTR _hap._tcp.local → Module Control._hap._tcp.local
        let service_at = m.len();
        for l in ["_hap", "_tcp", "local"] {
            m.push(l.len() as u8);
            m.extend_from_slice(l.as_bytes());
        }
        m.push(0);
        m.extend_from_slice(&[0, 12, 0, 1, 0, 0, 0, 120]);
        let instance = b"Module Control";
        m.extend_from_slice(&((instance.len() + 3) as u16).to_be_bytes());
        let instance_at = m.len();
        m.push(instance.len() as u8);
        m.extend_from_slice(instance);
        m.extend_from_slice(&[0xc0, service_at as u8]);
        // SRV → Module-Control.local:5001
        m.extend_from_slice(&[0xc0, instance_at as u8, 0, 33, 0x80, 1, 0, 0, 0, 120]);
        let host = b"Module-Control";
        m.extend_from_slice(&((6 + 1 + host.len() + 7) as u16).to_be_bytes());
        m.extend_from_slice(&[0, 0, 0, 0, 0x13, 0x89]);
        let host_at = m.len();
        m.push(host.len() as u8);
        m.extend_from_slice(host);
        m.extend_from_slice(&[5]);
        m.extend_from_slice(b"local");
        m.push(0);
        // TXT
        m.extend_from_slice(&[0xc0, instance_at as u8, 0, 16, 0x80, 1, 0, 0, 0, 120]);
        let txt: Vec<&[u8]> = vec![b"id=A1:B8:D6:5C:39:A6", b"md=Legrand Gateway", b"sf=1", b"ci=2", b"ff=1"];
        let len: usize = txt.iter().map(|t| t.len() + 1).sum();
        m.extend_from_slice(&(len as u16).to_be_bytes());
        for t in txt {
            m.push(t.len() as u8);
            m.extend_from_slice(t);
        }
        // A
        m.extend_from_slice(&[0xc0, host_at as u8, 0, 1, 0x80, 1, 0, 0, 0, 120, 0, 4, 192, 168, 86, 25]);
        m
    }

    #[test]
    fn reads_an_accessory_from_its_answer() {
        let mut records = Records::default();
        read(&answer(), &mut records, None).unwrap();
        assert_eq!(records.instances, vec!["Module Control._hap._tcp.local"]);
        assert_eq!(records.srv["module control._hap._tcp.local"], ("module-control.local".to_string(), 5001));
        assert_eq!(records.txt["module control._hap._tcp.local"]["md"], "Legrand Gateway");
        assert_eq!(records.a["module-control.local"], Ipv4Addr::new(192, 168, 86, 25));
        assert!(read(&[0, 1], &mut Records::default(), None).is_none());
    }

    /// The answers of a real Legrand gateway and Ecometer.
    #[test]
    fn reads_real_answers() {
        let mut records = Records::default();
        read(&hex::decode("000084000001000100000004045f686170045f746370056c6f63616c00000c0001c00c000c00010000000a0013104c656772616e642045636f6d65746572c00cc02d002100010000000a0019000000001389104c656772616e642045636f6d65746572c016c02d001000010000000a006a0463233d310473233d310466663d310473663d310670763d312e31146d643d4c656772616e642045636f6d65746572001469643d45383a32393a31433a38353a35463a4533136e615f746b6e3d3832326430333039343135370463693d320b73683d2f344f4d46673d3dc052000100010000000a0004c0a8561cc052001c00010000000a0010fe80000000000000020474fffe1266b6").unwrap(), &mut records, None).unwrap();
        read(&hex::decode("000084000001000100000004045f686170045f746370056c6f63616c00000c0001c00c000c00010000000a00110e4d6f64756c6520436f6e74726f6cc00cc02d002100010000000a00170000000013890e4d6f64756c6520436f6e74726f6cc016c02d001000010000000a006d0663233d3131390673233d3134360466663d310473663d310670763d312e31136d643d4c656772616e642047617465776179001469643d41313a42383a44363a35433a33393a4136136e615f746b6e3d3231616636383761316332330463693d320b73683d6276347064673d3dc050000100010000000a0004c0a85619c050001c00010000000a0010fe80000000000000020474fffe4351ec").unwrap(), &mut records, None).unwrap();
        println!("{:?} {:?} {:?} {:?}", records.instances, records.srv, records.txt, records.a);
        assert_eq!(records.instances.len(), 2);
        assert_eq!(records.a.len(), 2);
    }

    #[test]
    fn asks_in_dns_format() {
        let q = query(&[(SERVICE, PTR)]);
        assert_eq!(&q[..6], &[0, 0, 0, 0, 0, 1]);
        assert_eq!(&q[12..17], b"\x04_hap");
        assert_eq!(&q[q.len() - 4..], &[0, 12, 0, 1]);
    }
}
