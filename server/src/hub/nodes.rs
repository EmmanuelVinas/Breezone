//! The installations this server reaches through other nodes (ADR 0008): the live link to each
//! node, what it shares, and the commands sent to it.

use super::*;

impl Hub {
    // -----------------------------------------------------------------------------------
    // Federation: installations reached through other nodes (ADR 0008)
    // -----------------------------------------------------------------------------------

    /// Connection state of the nodes this server consumes.
    pub async fn node_statuses(&self) -> HashMap<i64, NodeStatus> {
        self.nodes.read().await.iter().map(|(id, n)| (*id, n.status.clone())).collect()
    }

    /// Installations each node shares with this one (as of its last answer).
    pub async fn node_offers(&self) -> HashMap<i64, Vec<OfferedInstallation>> {
        self.nodes.read().await.iter().map(|(id, n)| (*id, n.offered.clone())).collect()
    }

    /// What this server exposes to a share: its local installations among `ids`.
    pub async fn federated(&self, ids: &[i64]) -> Vec<FedInstallation> {
        let state = self.state.read().await;
        let mut out: Vec<FedInstallation> = ids.iter().filter_map(|id| state.get(id)).filter(|s| s.node_id().is_none() && s.inst.enabled).map(InstState::federated).collect();
        out.sort_by_key(|i| i.id);
        out
    }

    /// Reads the installations a node exposes, creates/removes the matching installations
    /// here, and updates their state.
    pub async fn poll_node(&self, node_id: i64) {
        let (client, node_name) = match self.nodes.read().await.get(&node_id) {
            Some(n) => (n.client.clone(), n.node.name.clone()),
            None => return,
        };
        // Commands sent while the snapshot is in flight make its values stale (see poll_one).
        let commands = self.node_commands(node_id).await;
        let started = Instant::now();
        let result = match client {
            Some(c) => c.snapshot().await,
            None => Err(NodeError::Rejected {
                status: 401,
                message: crate::text!("clé illisible : ajoutez de nouveau ce nœud", "unreadable key: add this node again", "clave ilegible: añada de nuevo este nodo"),
            }),
        };
        let latency = started.elapsed().as_millis() as u64;
        match result {
            Ok(snap) => self.node_ok(node_id, &node_name, snap, latency, &commands).await,
            Err(e) => self.node_failed(node_id, &node_name, e).await,
        }
    }

    async fn node_ok(&self, node_id: i64, node_name: &str, snap: FedSnapshot, latency: u64, commands: &HashMap<i64, u64>) {
        // 1. Node state.
        let now = db::now();
        let (was, persist, permission_changed) = {
            let mut nodes = self.nodes.write().await;
            let Some(n) = nodes.get_mut(&node_id) else { return };
            let was = n.status.state;
            let permission_changed = n.status.permission != snap.permission;
            n.failures = 0;
            n.offered = snap.installations.iter().map(|f| OfferedInstallation { remote_id: f.id, name: f.name.clone(), location: f.location.clone() }).collect();
            n.status = NodeStatus { state: "online", last_seen: Some(now), last_error: None, latency_ms: Some(latency), permission: snap.permission.clone() };
            n.node.permission = snap.permission.clone();
            let persist = was != "online" || permission_changed || now - n.persisted_at >= 60;
            if persist {
                n.persisted_at = now;
            }
            (was, persist, permission_changed)
        };
        if persist {
            if let Err(e) = db::set_node_seen(&self.db, node_id, Some(&snap.name), Some(&snap.permission), None).await {
                tracing::error!("cannot store node state: {e}");
            }
        }
        if was != "online" {
            self.log("connection", "info", None, None, crate::text!("Nœud « {0} » connecté", "Node “{0}” connected", "Nodo «{0}» conectado", node_name), None).await;
        }

        // 2. Installations exposed by the node.
        let list_changed = match self.sync_node_installations(node_id, node_name, &snap).await {
            Ok(changed) => changed,
            Err(e) => {
                tracing::error!("cannot update the installations of node {node_name}: {e:#}");
                false
            }
        };
        if list_changed || permission_changed || was != "online" {
            if let Err(e) = self.reload_installations().await {
                tracing::error!("cannot reload installations: {e:#}");
            }
        }

        // 3. Their state.
        let mut views = vec![];
        let mut records = vec![];
        {
            let mut state = self.state.write().await;
            for fed in snap.installations {
                let Some(s) = state.values_mut().find(|s| matches!(s.backend, Backend::Node { node_id: n, remote_id } if n == node_id && remote_id == fed.id)) else {
                    continue;
                };
                if !s.inst.enabled {
                    continue;
                }
                let stale = commands.get(&s.inst.id).is_some_and(|c| *c != s.commands);
                let before = (s.conn.clone(), s.zones.clone(), s.remote_meta.clone());
                s.remote_meta = fed.meta.into_iter().map(|m| ((m.system_id, m.zone_id), m)).collect();
                s.failures = 0;
                s.conn = ConnectionView {
                    state: match fed.connection.state.as_str() {
                        "online" => "online",
                        "offline" => "offline",
                        _ => "unknown",
                    },
                    last_seen: fed.connection.last_seen,
                    last_error: fed.connection.last_error.map(Text::plain),
                    latency_ms: Some(latency),
                };
                if stale {
                    // A command was sent meanwhile: keep the values it set.
                } else if s.conn.state == "online" {
                    s.record_demand = fed.record_demand;
                    records.push((s.inst.id, s.apply_zones(fed.zones, self.cfg.history_interval_secs, self.demand_max_gap())));
                } else {
                    s.zones = fed.zones;
                }
                let after = (&s.conn, &s.zones, &s.remote_meta);
                if before.0.state != after.0.state || before.0.last_error != after.0.last_error || &before.1 != after.1 || &before.2 != after.2 {
                    views.push(s.view());
                }
            }
        }
        for (id, zones) in records {
            self.record(id, zones).await;
        }
        for view in views {
            self.publish(view).await;
        }
    }

    async fn node_failed(&self, node_id: i64, node_name: &str, e: NodeError) {
        let message = e.text();
        let (changed, state_now) = {
            let mut nodes = self.nodes.write().await;
            let Some(n) = nodes.get_mut(&node_id) else { return };
            n.failures += 1;
            let was = n.status.state;
            // A refused key is final; a network error must repeat before the node is offline.
            if e.state() != "offline" || n.failures >= OFFLINE_AFTER_FAILURES {
                n.status.state = e.state();
            }
            n.status.last_error = Some(message.clone());
            n.status.latency_ms = None;
            (was != n.status.state, n.status.state)
        };
        if changed {
            let _ = db::set_node_seen(&self.db, node_id, None, None, Some(&message.store())).await;
            let text = match state_now {
                "revoked" => crate::text!(
                    "Nœud « {0} » : la clé de partage a été révoquée",
                    "Node “{0}”: the share key was revoked",
                    "Nodo «{0}»: la clave de uso compartido fue revocada",
                    node_name
                ),
                "expired" => crate::text!("Nœud « {0} » : le partage a expiré", "Node “{0}”: the share expired", "Nodo «{0}»: el uso compartido caducó", node_name),
                "pending" => crate::text!(
                    "Nœud « {0} » : en attente d'acceptation sur ce serveur (Réglages › Serveurs)",
                    "Node “{0}”: waiting for approval on that server (Settings › Servers)",
                    "Nodo «{0}»: pendiente de aceptación en ese servidor (Ajustes › Servidores)",
                    node_name
                ),
                "certificate" => crate::text!(
                    "Nœud « {0} » : {1} (Réglages › Serveurs pour l'examiner)",
                    "Node “{0}”: {1} (Settings › Servers to review it)",
                    "Nodo «{0}»: {1} (Ajustes › Servidores para revisarlo)",
                    node_name,
                    message.arg()
                ),
                _ => crate::text!("Nœud « {0} » injoignable : {1}", "Node “{0}” unreachable: {1}", "Nodo «{0}» inaccesible: {1}", node_name, message.arg()),
            };
            self.log("connection", "warn", None, None, text, None).await;
        }
        let mut views = vec![];
        {
            let mut state = self.state.write().await;
            for s in state.values_mut().filter(|s| s.node_id() == Some(node_id)) {
                s.failures += 1;
                let before = s.conn.clone();
                s.conn.last_error = Some(crate::text!("via « {0} » : {1}", "via “{0}”: {1}", "vía «{0}»: {1}", node_name, message.arg()));
                s.conn.latency_ms = None;
                if state_now != "online" {
                    s.conn.state = "offline";
                }
                if let Some(r) = s.node.as_mut() {
                    r.state = state_now;
                }
                if changed || before.state != s.conn.state {
                    views.push(s.view());
                }
            }
        }
        for view in views {
            self.publish(view).await;
        }
    }

    /// Creates the installations a node exposes (unless their WebServer is already driven
    /// here) and deletes the ones it no longer exposes. Returns whether anything changed.
    async fn sync_node_installations(&self, node_id: i64, node_name: &str, snap: &FedSnapshot) -> anyhow::Result<bool> {
        let all = db::list_installations(&self.db).await?;
        // The same WebServer may also exist here as a local installation (same MAC): added by
        // hand, or by the Airzone account when it is connected on both servers. The local copy
        // only wins when this server actually reaches it; otherwise the shared copy is used.
        let reach: HashMap<i64, &'static str> = self.state.read().await.values().map(|s| (s.inst.id, s.conn.state)).collect();
        let mut local: HashMap<String, Local> = HashMap::new();
        for i in all.iter().filter(|i| i.is_local()) {
            let Some(mac) = i.mac.as_deref().map(normalize_mac) else { continue };
            let here = match reach.get(&i.id).copied() {
                _ if !i.enabled => Local::Unreachable,
                Some("online") => Local::Reached,
                Some("offline") => Local::Unreachable,
                _ => Local::Unknown,
            };
            let best = local.entry(mac).or_insert(Local::Unreachable);
            *best = (*best).max(here);
        }
        let Some(node) = db::node_by_id(&self.db, node_id).await? else { return Ok(false) };
        let mine: Vec<&Installation> = all.iter().filter(|i| i.node_id == Some(node_id)).collect();
        let excluded = db::node_exclusions(&self.db, node_id).await?;
        let mut changed = false;
        for fed in &snap.installations {
            if excluded.contains(&fed.id) {
                // Shared, but not wanted on this server.
                if let Some(i) = mine.iter().find(|i| i.remote_id == Some(fed.id)) {
                    db::delete_installation(&self.db, i.id).await?;
                    let text = crate::text!(
                        "« {0} » (nœud « {1} ») n'est plus affichée sur ce serveur",
                        "“{0}” (node “{1}”) is no longer shown on this server",
                        "«{0}» (nodo «{1}») ya no se muestra en este servidor",
                        i.name,
                        node_name
                    );
                    self.log("system", "info", None, None, text, None).await;
                    changed = true;
                }
                continue;
            }
            let here = fed.mac.as_deref().map(normalize_mac).and_then(|m| local.get(&m).copied());
            let existing = mine.iter().find(|i| i.remote_id == Some(fed.id));
            match (existing, here) {
                // Not known yet (first poll of the local copy): decide on a later poll.
                (_, Some(Local::Unknown)) => {}
                (None, here) if here != Some(Local::Reached) => {
                    let inst = db::insert_remote_installation(&self.db, &fed.name, node_id, fed.id, &node.url, fed.mac.as_deref(), fed.location.as_deref()).await?;
                    let text = crate::text!(
                        "Installation « {0} » ajoutée via le nœud « {1} »",
                        "Installation “{0}” added through node “{1}”",
                        "Instalación «{0}» añadida a través del nodo «{1}»",
                        fed.name,
                        node_name
                    );
                    self.log("system", "info", Some(inst.id), None, text, None).await;
                    if here == Some(Local::Unreachable) {
                        self.log(
                            "system",
                            "warn",
                            Some(inst.id),
                            None,
                            crate::text!(
                                "« {0} » existe aussi sur ce serveur, mais son WebServer n'y répond pas : elle est pilotée via le nœud « {1} ». Supprimez la copie locale si elle ne sert plus (Réglages › Installations)",
                                "“{0}” also exists on this server, but its WebServer does not answer here: it is driven through node “{1}”. Delete the local copy if it is no longer needed (Settings › Installations)",
                                "«{0}» también existe en este servidor, pero su WebServer no responde aquí: se controla a través del nodo «{1}». Elimine la copia local si ya no sirve (Ajustes › Instalaciones)",
                                fed.name,
                                node_name
                            ),
                            None,
                        )
                        .await;
                    }
                    changed = true;
                }
                (Some(i), Some(Local::Reached)) => {
                    // Its WebServer is on this network: drive it directly.
                    db::delete_installation(&self.db, i.id).await?;
                    changed = true;
                }
                (Some(i), _) if i.mac.is_none() && fed.mac.is_some() => {
                    db::set_installation_mac(&self.db, i.id, fed.mac.as_deref().unwrap_or_default()).await?;
                }
                _ => {}
            }
        }
        for i in mine.iter().filter(|i| !snap.installations.iter().any(|f| Some(f.id) == i.remote_id)) {
            db::delete_installation(&self.db, i.id).await?;
            let text = crate::text!(
                "« {0} » n'est plus partagée par le nœud « {1} »",
                "“{0}” is no longer shared by node “{1}”",
                "«{0}» ya no la comparte el nodo «{1}»",
                i.name,
                node_name
            );
            self.log("system", "warn", None, None, text, None).await;
            changed = true;
        }
        Ok(changed)
    }

    /// Keeps one live link per node (started for new nodes, stopped for removed ones). A link
    /// survives both servers going down: it retries until the other one answers again, and
    /// starts again with this server.
    pub async fn run_node_links(self: Arc<Self>) {
        // Aborted on drop: stopping this task stops every link.
        struct Link(tokio::task::JoinHandle<()>);
        impl Drop for Link {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let mut links: HashMap<i64, Link> = HashMap::new();
        let mut ticker = tokio::time::interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            let ids: Vec<i64> = self.nodes.read().await.keys().copied().collect();
            links.retain(|id, link| ids.contains(id) && !link.0.is_finished());
            for id in ids {
                links.entry(id).or_insert_with(|| Link(tokio::spawn(self.clone().node_link(id))));
            }
        }
    }

    /// Follows one node: asks for its content "once it changes" in a loop (long polling over the
    /// signed API). Nodes that cannot wait (up to 0.5.x) are left to the periodic poller.
    async fn node_link(self: Arc<Self>, node_id: i64) {
        let mut since: Option<String> = None;
        let mut failures: u32 = 0;
        let mut latency: u64 = 0;
        // `caps_rev` of this server last told to the node (ADR 0012): told again when it changes
        // (a module installed…) or when the node's own changes. Always told when the link opens.
        let mut told: Option<String> = None;
        loop {
            let (client, name) = match self.nodes.read().await.get(&node_id) {
                Some(n) => (n.client.clone(), n.node.name.clone()),
                None => return,
            };
            // Unreadable key: the poller reports it; check again later.
            let Some(client) = client else {
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            };
            let mine = Capabilities::local();
            if told.as_deref() != Some(mine.caps_rev.as_str()) {
                match client.hello(&mine).await {
                    Ok(peer) => {
                        self.set_peer_caps(node_id, &name, peer.unwrap_or_else(Capabilities::legacy)).await;
                        told = Some(mine.caps_rev.clone());
                    }
                    // Tried again at the next turn; reading the node does not wait for it.
                    Err(e) => tracing::debug!("capabilities of node {name} not exchanged: {e}"),
                }
            }
            let commands = self.node_commands(node_id).await;
            let started = Instant::now();
            match client.watch(since.as_deref(), self.cfg.node_watch_secs).await {
                // Nothing new during the wait: the node is alive, its content unchanged.
                Ok(None) => {
                    failures = 0;
                    self.node_alive(node_id, latency).await;
                }
                Ok(Some(snap)) => {
                    failures = 0;
                    let Some(revision) = snap.revision.clone() else {
                        // Older node: periodic reads (by the poller), and a new try later on in
                        // case it was updated.
                        self.set_watching(node_id, false).await;
                        tokio::time::sleep(Duration::from_secs(600)).await;
                        continue;
                    };
                    // Only an answer that did not wait measures the round trip: the first read
                    // (no `since`), or one that came back at once. An answer sent because the
                    // content changed during the wait would count the wait as latency.
                    if since.is_none() || started.elapsed() < Duration::from_secs(2) {
                        latency = started.elapsed().as_millis() as u64;
                    }
                    self.set_watching(node_id, true).await;
                    // The node's capabilities changed (update, rollback, module): exchange again.
                    if let Some(rev) = snap.caps_rev.as_deref() {
                        let known = self.nodes.read().await.get(&node_id).and_then(|n| n.peer_caps.as_ref().map(|c| c.caps_rev.clone()));
                        if known.as_deref() != Some(rev) {
                            told = None;
                        }
                    }
                    // A command sent while waiting may make this content stale: read again at once.
                    let stale = self.node_commands(node_id).await != commands;
                    self.node_ok(node_id, &name, snap, latency, &commands).await;
                    since = if stale { None } else { Some(revision) };
                }
                Err(e) => {
                    self.set_watching(node_id, false).await;
                    self.node_failed(node_id, &name, e).await;
                    since = None;
                    failures += 1;
                    let pause = node_retry_secs(failures).min(self.cfg.node_retry_max_secs.max(1));
                    tokio::time::sleep(Duration::from_secs(pause)).await;
                }
            }
        }
    }

    /// Records what a node knows (ADR 0012), and notes in the activity feed when its version
    /// changed (update or rollback on that side).
    async fn set_peer_caps(&self, node_id: i64, name: &str, caps: Capabilities) {
        let before = {
            let mut nodes = self.nodes.write().await;
            let Some(n) = nodes.get_mut(&node_id) else { return };
            if n.peer_caps.as_ref() == Some(&caps) {
                return;
            }
            n.peer_caps.replace(caps.clone())
        };
        match serde_json::to_string(&caps) {
            Ok(json) => {
                if let Err(e) = db::set_node_caps(&self.db, node_id, &json).await {
                    tracing::error!("cannot store the capabilities of node {name}: {e}");
                }
            }
            Err(e) => tracing::error!("capabilities of node {name}: {e}"),
        }
        let compatibility = Capabilities::local().compatibility(&caps);
        if compatibility.state == "outdated" {
            self.log(
                "connection",
                "warn",
                None,
                None,
                crate::text!(
                    "« {0} » et ce serveur ne se comprennent plus : mettez à jour le plus ancien des deux",
                    "“{0}” and this server no longer understand each other: update the older one",
                    "«{0}» y este servidor ya no se entienden: actualice el más antiguo",
                    name
                ),
                None,
            )
            .await;
        } else if before.is_some_and(|b| b.breezone != caps.breezone) && !caps.breezone.is_empty() {
            self.log(
                "connection",
                "info",
                None,
                None,
                crate::text!("« {0} » est passé à Breezone {1}", "“{0}” moved to Breezone {1}", "«{0}» pasó a Breezone {1}", name, caps.breezone),
                None,
            )
            .await;
        }
    }

    /// Before sending something to a node: does it say it can do it (ADR 0012)? A node that
    /// predates the exchange is assumed to do what 0.10.0 did.
    async fn node_can(&self, node_id: i64, node_name: &str, feature: &str, version: u32) -> ApiResult<()> {
        let caps = self.nodes.read().await.get(&node_id).and_then(|n| n.peer_caps.clone()).unwrap_or_else(Capabilities::legacy);
        if caps.has_feature(feature, version) {
            return Ok(());
        }
        let current = if caps.breezone.is_empty() { "≤ 0.10.0".to_string() } else { caps.breezone.clone() };
        Err(ApiError::new(
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            crate::text!(
                "« {0} » (Breezone {1}) ne sait pas encore le faire : mettez-le à jour",
                "“{0}” (Breezone {1}) cannot do this yet: update it",
                "«{0}» (Breezone {1}) aún no sabe hacerlo: actualícelo",
                node_name,
                current
            ),
        )
        .with_detail("code", "unsupported")
        .with_detail("needs", format!("{feature}@{version}")))
    }

    /// The node answered "nothing new": still online, nothing else to update.
    async fn node_alive(&self, node_id: i64, latency: u64) {
        let now = db::now();
        let persist = {
            let mut nodes = self.nodes.write().await;
            let Some(n) = nodes.get_mut(&node_id) else { return };
            n.failures = 0;
            n.status.state = "online";
            n.status.last_seen = Some(now);
            n.status.last_error = None;
            n.status.latency_ms = Some(latency);
            let persist = now - n.persisted_at >= 60;
            if persist {
                n.persisted_at = now;
            }
            persist
        };
        if persist {
            if let Err(e) = db::set_node_seen(&self.db, node_id, None, None, None).await {
                tracing::error!("cannot store node state: {e}");
            }
        }
    }

    async fn node_commands(&self, node_id: i64) -> HashMap<i64, u64> {
        self.state.read().await.values().filter(|s| s.node_id() == Some(node_id)).map(|s| (s.inst.id, s.commands)).collect()
    }

    async fn set_watching(&self, node_id: i64, watching: bool) {
        if let Some(n) = self.nodes.write().await.get_mut(&node_id) {
            n.watching = watching;
        }
    }

    // -----------------------------------------------------------------------------------
    // Commands to a node
    // -----------------------------------------------------------------------------------

    /// Has the node owning an installation change its Airzone schedules.
    pub async fn schedule_remote(&self, node_id: i64, remote_id: i64, op: &crate::airzone_schedules::ScheduleOp, actor: &str) -> ApiResult<()> {
        let (client, node_name) = {
            let nodes = self.nodes.read().await;
            let n = nodes.get(&node_id).ok_or_else(|| ApiError::not_found("node not found"))?;
            (n.client.clone(), n.node.name.clone())
        };
        let client = client.ok_or_else(|| {
            ApiError::bad_gateway(crate::text!(
                "clé du nœud « {0} » illisible : ajoutez-le de nouveau",
                "unreadable key of node “{0}”: add it again",
                "clave del nodo «{0}» ilegible: añádalo de nuevo",
                node_name
            ))
        })?;
        self.node_can(node_id, &node_name, "federation.schedules", 1).await?;
        client.schedule(remote_id, op, actor).await?;
        Ok(())
    }

    /// Sends a command to the node exposing the installation; it validates and applies it.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn control_remote(
        &self,
        id: i64,
        node_id: i64,
        remote_id: i64,
        inst_name: &str,
        system_id: i64,
        zone_id: i64,
        cmd: &ZoneCommand,
        actor: &str,
    ) -> ApiResult<ZoneView> {
        let (client, node_name) = {
            let nodes = self.nodes.read().await;
            let n = nodes.get(&node_id).ok_or_else(|| ApiError::not_found("node not found"))?;
            (n.client.clone(), n.node.name.clone())
        };
        let client = client.ok_or_else(|| {
            ApiError::bad_gateway(crate::text!(
                "clé du nœud « {0} » illisible : ajoutez-le de nouveau",
                "unreadable key of node “{0}”: add it again",
                "clave del nodo «{0}» ilegible: añádalo de nuevo",
                node_name
            ))
        })?;
        self.node_can(node_id, &node_name, "federation.control", 1).await?;
        let zone_label = self.zone(id, system_id, zone_id).await.map(|z| z.name).unwrap_or_else(|| format!("Zone {zone_id}"));
        let data = Some(json!({ "system_id": system_id, "zone_id": zone_id, "command": cmd, "node": node_name }));
        match client.control(remote_id, system_id, zone_id, cmd, actor).await {
            Ok(fed) => {
                let view = {
                    let mut state = self.state.write().await;
                    state.get_mut(&id).map(|s| {
                        s.zones = fed.zones;
                        s.view()
                    })
                };
                let text = crate::text!("{0} ({1}, via {2}) : {3}", "{0} ({1}, via {2}): {3}", "{0} ({1}, vía {2}): {3}", zone_label, inst_name, node_name, cmd.describe().arg());
                self.log("command", "info", Some(id), Some(actor), text, data).await;
                let view = view.ok_or_else(|| ApiError::not_found("installation not found"))?;
                self.publish(view.clone()).await;
                view.zones.into_iter().find(|z| z.system_id == system_id && z.zone_id == zone_id).ok_or_else(|| ApiError::not_found("zone not found"))
            }
            Err(e) => {
                let text = crate::text!(
                    "Échec de la commande sur {0} ({1}, via {2}) : {3}",
                    "Command failed on {0} ({1}, via {2}): {3}",
                    "Fallo de la orden en {0} ({1}, vía {2}): {3}",
                    zone_label,
                    inst_name,
                    node_name,
                    e.text().arg()
                );
                self.log("command", "error", Some(id), Some(actor), text, data).await;
                Err(e.into())
            }
        }
    }
}
