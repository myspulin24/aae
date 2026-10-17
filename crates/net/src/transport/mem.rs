//! Síť v paměti: spojení s nastavitelným zpožděním a „vytažením kabelu“.
//! S `tokio::time::pause` běží testy deterministicky a bez skutečného čekání.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::{Notify, mpsc, watch};
use tokio::time::{Instant, sleep_until};

use super::{Connection, PathInfo, PathKind};
use crate::PeerKey;

/// Ovládání jednoho spojení v testu.
#[derive(Clone)]
pub struct Link {
    inner: Arc<LinkInner>,
}

struct LinkInner {
    latency: Mutex<Duration>,
    cut: AtomicBool,
    notify: Notify,
}

impl Link {
    pub fn set_latency(&self, latency: Duration) {
        *self.inner.latency.lock().expect("zámek zpoždění") = latency;
    }

    /// Přeruší spojení v obou směrech (zprávy na cestě se ztratí).
    pub fn cut(&self) {
        self.inner.cut.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    fn latency(&self) -> Duration {
        *self.inner.latency.lock().expect("zámek zpoždění")
    }

    fn is_cut(&self) -> bool {
        self.inner.cut.load(Ordering::SeqCst)
    }
}

/// Spojí `a` a `b` v paměti. Vrací stranu `a` (peer = b), stranu `b`
/// (peer = a) a ovládání. Každý směr má zpoždění `latency`.
pub fn pair(a: PeerKey, b: PeerKey, latency: Duration) -> (Connection, Connection, Link) {
    let link = Link {
        inner: Arc::new(LinkInner {
            latency: Mutex::new(latency),
            cut: AtomicBool::new(false),
            notify: Notify::new(),
        }),
    };
    let (a_out_tx, a_out_rx) = mpsc::unbounded_channel();
    let (b_out_tx, b_out_rx) = mpsc::unbounded_channel();
    let (a_in_tx, a_in_rx) = mpsc::unbounded_channel();
    let (b_in_tx, b_in_rx) = mpsc::unbounded_channel();
    tokio::spawn(pump(a_out_rx, b_in_tx, link.clone()));
    tokio::spawn(pump(b_out_rx, a_in_tx, link.clone()));
    let path = PathInfo {
        kind: PathKind::Direct,
        rtt_ms: (latency.as_millis() * 2) as u32,
    };
    let (_path_tx_a, path_a) = watch::channel(path);
    let (_path_tx_b, path_b) = watch::channel(path);
    let side_a = Connection {
        peer: b,
        local: false,
        tx: a_out_tx,
        rx: a_in_rx,
        path: path_a,
    };
    let side_b = Connection {
        peer: a,
        local: false,
        tx: b_out_tx,
        rx: b_in_rx,
        path: path_b,
    };
    (side_a, side_b, link)
}

/// Spojení hostitelova vlastního klienta ve stejném procesu (bez zpoždění).
pub fn local_pair(host: PeerKey) -> (Connection, Connection) {
    let (mut a, mut b, _link) = pair(host, host, Duration::ZERO);
    let (_t1, pa) = watch::channel(PathInfo::LOCAL);
    let (_t2, pb) = watch::channel(PathInfo::LOCAL);
    a.local = true;
    b.local = true;
    a.path = pa;
    b.path = pb;
    (a, b)
}

async fn pump(
    mut input: mpsc::UnboundedReceiver<Vec<u8>>,
    output: mpsc::UnboundedSender<Vec<u8>>,
    link: Link,
) {
    let mut queue: VecDeque<(Instant, Vec<u8>)> = VecDeque::new();
    let mut input_open = true;
    loop {
        if link.is_cut() {
            return;
        }
        let next = queue.front().map(|(t, _)| *t);
        if !input_open && next.is_none() {
            return;
        }
        tokio::select! {
            msg = input.recv(), if input_open => match msg {
                Some(m) => queue.push_back((Instant::now() + link.latency(), m)),
                None => input_open = false,
            },
            _ = async { sleep_until(next.expect("hlídá podmínka")).await }, if next.is_some() => {
                let (_, m) = queue.pop_front().expect("fronta není prázdná");
                if link.is_cut() || output.send(m).is_err() {
                    return;
                }
            }
            _ = link.inner.notify.notified() => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn delivers_in_order_after_latency() {
        let (mut a, mut b, _link) = pair(
            PeerKey([1; 32]),
            PeerKey([2; 32]),
            Duration::from_millis(250),
        );
        let started = Instant::now();
        a.tx.send(vec![1]).unwrap();
        a.tx.send(vec![2]).unwrap();
        assert_eq!(b.rx.recv().await, Some(vec![1]));
        assert_eq!(b.rx.recv().await, Some(vec![2]));
        assert!(started.elapsed() >= Duration::from_millis(250));
        b.tx.send(vec![3]).unwrap();
        assert_eq!(a.rx.recv().await, Some(vec![3]));
    }

    #[tokio::test(start_paused = true)]
    async fn cut_closes_both_directions() {
        let (a, mut b, link) = pair(
            PeerKey([1; 32]),
            PeerKey([2; 32]),
            Duration::from_millis(10),
        );
        link.cut();
        let _ = a.tx.send(vec![1]);
        assert_eq!(b.rx.recv().await, None);
    }
}
