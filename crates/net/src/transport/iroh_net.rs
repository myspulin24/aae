//! Spojení přes iroh: QUIC, průchod NATem, záložní relay n0.
//! Zprávy jdou jedním obousměrným streamem jako rámce `délka (u32 BE) + data`.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use iroh::endpoint::{Connection as IrohConnection, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, SecretKey};
use iroh_tickets::endpoint::EndpointTicket;
use tokio::sync::{mpsc, watch};

use super::{Connection, PathInfo, PathKind};
use crate::PeerKey;

pub const ALPN: &[u8] = b"aae/1";
const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Nový tajný klíč identity hráče (ukládá se do profilu, R6).
pub fn generate_secret() -> [u8; 32] {
    SecretKey::generate().to_bytes()
}

/// Uzel iroh jedné instance hry. Identita = trvalý tajný klíč z profilu (R6).
#[derive(Clone)]
pub struct IrohNode {
    endpoint: Endpoint,
}

impl IrohNode {
    /// Uzel přes veřejnou infrastrukturu n0 (relay, DNS discovery).
    pub async fn bind(secret: [u8; 32]) -> Result<IrohNode> {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::from_bytes(&secret))
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .context("nepodařilo se otevřít síťový uzel iroh")?;
        Ok(IrohNode { endpoint })
    }

    /// Uzel jen pro místní adresy, bez relay a bez internetu (testy, CI).
    pub async fn bind_local(secret: [u8; 32]) -> Result<IrohNode> {
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(SecretKey::from_bytes(&secret))
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .context("nepodařilo se otevřít místní uzel iroh")?;
        Ok(IrohNode { endpoint })
    }

    pub fn key(&self) -> PeerKey {
        PeerKey(*self.endpoint.id().as_bytes())
    }

    /// Pozvánka; počká, až uzel zná své adresy a relay (nejvýš 10 s).
    pub async fn ticket(&self) -> String {
        let _ = tokio::time::timeout(Duration::from_secs(10), self.endpoint.online()).await;
        EndpointTicket::new(self.endpoint.addr()).to_string()
    }

    /// Pozvánka z právě známých adres bez čekání na relay (místní uzel).
    pub fn ticket_now(&self) -> String {
        EndpointTicket::new(self.endpoint.addr()).to_string()
    }

    /// Příchozí spojení (hostitel).
    pub fn listen(&self) -> mpsc::UnboundedReceiver<Connection> {
        let (tx, rx) = mpsc::unbounded_channel();
        let endpoint = self.endpoint.clone();
        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let Ok(conn) = incoming.await else { return };
                    let Ok((send, recv)) = conn.accept_bi().await else {
                        return;
                    };
                    let _ = tx.send(wrap(conn, send, recv));
                });
            }
        });
        rx
    }

    /// Připojení k hostiteli podle pozvánky.
    pub async fn connect(&self, ticket: &str) -> Result<Connection> {
        let ticket: EndpointTicket = ticket
            .trim()
            .parse()
            .context("pozvánka není platná, zkopírujte ji celou")?;
        let addr = EndpointAddr::from(ticket);
        let conn = self
            .endpoint
            .connect(addr, ALPN)
            .await
            .context("nepodařilo se připojit k hostiteli")?;
        let (send, recv) = conn
            .open_bi()
            .await
            .context("nepodařilo se otevřít stream")?;
        Ok(wrap(conn, send, recv))
    }

    pub async fn close(&self) {
        self.endpoint.close().await;
    }
}

fn wrap(conn: IrohConnection, mut send: SendStream, mut recv: RecvStream) -> Connection {
    let peer = PeerKey(*conn.remote_id().as_bytes());
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (in_tx, in_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (path_tx, path_rx) = watch::channel(path_of(&conn));

    // Zápis: rámce z kanálu do streamu; zavřený kanál = konec spojení.
    let writer_conn = conn.clone();
    tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            let len = (frame.len() as u32).to_be_bytes();
            if send.write_all(&len).await.is_err() || send.write_all(&frame).await.is_err() {
                break;
            }
        }
        // Zavření spojení zahodí nepotvrzená data; poslední zpráva (konec hry)
        // musí nejdřív dojít.
        let _ = send.finish();
        let _ = tokio::time::timeout(Duration::from_secs(2), send.stopped()).await;
        writer_conn.close(0u32.into(), b"konec");
    });

    // Čtení: rámce ze streamu do kanálu.
    tokio::spawn(async move {
        while let Ok(frame) = read_frame(&mut recv).await {
            if in_tx.send(frame).is_err() {
                break;
            }
        }
    });

    // Typ cesty (přímá / relay) a odezva pro lobby.
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if conn.close_reason().is_some() || path_tx.send(path_of(&conn)).is_err() {
                break;
            }
        }
    });

    Connection {
        peer,
        local: false,
        tx: out_tx,
        rx: in_rx,
        path: path_rx,
    }
}

async fn read_frame(recv: &mut RecvStream) -> Result<Vec<u8>> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME {
        bail!("příliš velký rámec ({len} B)");
    }
    let mut frame = vec![0u8; len];
    recv.read_exact(&mut frame).await?;
    Ok(frame)
}

fn path_of(conn: &IrohConnection) -> PathInfo {
    let paths = conn.paths();
    match paths.iter().find(|p| p.is_selected()) {
        Some(p) => PathInfo {
            kind: if p.is_relay() {
                PathKind::Relay
            } else {
                PathKind::Direct
            },
            rtt_ms: p.rtt().as_millis() as u32,
        },
        None => PathInfo::UNKNOWN,
    }
}
