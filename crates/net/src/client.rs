//! Klient: pošle Hello, drží pohled na hru (Snapshot + Delta), posílá záměry
//! a heartbeat. Bez zprávy od hostitele déle než 3 s hlásí `Lost` (R5).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use aae_sim::{Event, Intent};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior};

use crate::host::{HEARTBEAT, LOST_AFTER};
use crate::protocol::{
    ClientMsg, EndReason, HostMsg, HostReject, LobbyView, PROTOCOL_VERSION, RefuseReason, decode,
    encode,
};
use crate::transport::{Connection, PathInfo};
use crate::view::View;

pub struct ClientConfig {
    pub app_version: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClientUpdate {
    Welcome { host_version: String },
    Lobby(LobbyView),
    View(Box<View>),
    Events(Vec<Event>),
    Ack(u64),
    Reject(u64, HostReject),
    Waiting,
    Refused(RefuseReason),
    Ended(EndReason),
    Path(PathInfo),
    Lost,
}

#[derive(Clone)]
pub struct ClientSender {
    tx: mpsc::UnboundedSender<ClientMsg>,
    next_id: Arc<AtomicU64>,
}

impl ClientSender {
    /// Pošle záměr; vrací jeho id pro spárování s Ack/Reject.
    pub fn intent(&self, intent: Intent) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(ClientMsg::Intent { id, intent });
        id
    }

    pub fn send(&self, msg: ClientMsg) {
        let _ = self.tx.send(msg);
    }
}

pub struct ClientHandle {
    pub sender: ClientSender,
    pub updates: mpsc::UnboundedReceiver<ClientUpdate>,
}

pub fn spawn_client(conn: Connection, cfg: ClientConfig) -> ClientHandle {
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    let (upd_tx, upd_rx) = mpsc::unbounded_channel();
    tokio::spawn(run(conn, cfg, out_rx, upd_tx));
    ClientHandle {
        sender: ClientSender {
            tx: out_tx,
            next_id: Arc::new(AtomicU64::new(1)),
        },
        updates: upd_rx,
    }
}

async fn run(
    mut conn: Connection,
    cfg: ClientConfig,
    mut out: mpsc::UnboundedReceiver<ClientMsg>,
    updates: mpsc::UnboundedSender<ClientUpdate>,
) {
    let hello = ClientMsg::Hello {
        app_version: cfg.app_version,
        protocol: PROTOCOL_VERSION,
        name: cfg.name,
        world_hash: crate::world_hash(),
    };
    if conn.tx.send(encode(&hello)).is_err() {
        let _ = updates.send(ClientUpdate::Lost);
        return;
    }
    let mut view: Option<View> = None;
    let mut last_seen = Instant::now();
    let mut beat = tokio::time::interval(HEARTBEAT);
    beat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut out_open = true;
    loop {
        tokio::select! {
            frame = conn.rx.recv() => {
                let Some(bytes) = frame else {
                    let _ = updates.send(ClientUpdate::Lost);
                    return;
                };
                last_seen = Instant::now();
                let Ok(msg) = decode::<HostMsg>(&bytes) else { continue };
                let update = match msg {
                    HostMsg::Welcome { host_version } => ClientUpdate::Welcome { host_version },
                    HostMsg::Lobby(l) => ClientUpdate::Lobby(l),
                    HostMsg::Snapshot(v) => {
                        view = Some(*v.clone());
                        ClientUpdate::View(v)
                    }
                    HostMsg::Delta(d) => match view.as_mut() {
                        Some(v) => {
                            v.apply(*d);
                            ClientUpdate::View(Box::new(v.clone()))
                        }
                        None => continue,
                    },
                    HostMsg::Events(e) => ClientUpdate::Events(e),
                    HostMsg::Ack { id } => ClientUpdate::Ack(id),
                    HostMsg::Reject { id, reason } => ClientUpdate::Reject(id, reason),
                    HostMsg::Waiting => ClientUpdate::Waiting,
                    HostMsg::Heartbeat => continue,
                    HostMsg::Refused(r) => {
                        let _ = updates.send(ClientUpdate::Refused(r));
                        return;
                    }
                    HostMsg::Ended(r) => {
                        let _ = updates.send(ClientUpdate::Ended(r));
                        return;
                    }
                };
                if updates.send(update).is_err() {
                    return;
                }
            }
            msg = out.recv(), if out_open => match msg {
                Some(msg) => {
                    let leaving = msg == ClientMsg::Leave;
                    let _ = conn.tx.send(encode(&msg));
                    if leaving {
                        return;
                    }
                }
                None => out_open = false,
            },
            _ = beat.tick() => {
                if last_seen.elapsed() > LOST_AFTER {
                    let _ = updates.send(ClientUpdate::Lost);
                    return;
                }
                let _ = conn.tx.send(encode(&ClientMsg::Heartbeat));
                let path = *conn.path.borrow();
                let _ = updates.send(ClientUpdate::Path(path));
            }
        }
    }
}
