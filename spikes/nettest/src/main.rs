//! AAE – síťový test (vlna 1).
//!
//! Ověřuje, že se dva počítače spojí přes iroh bez nastavování routeru:
//! hostitel vytvoří pozvánku, kamarád ji vloží, program změří odezvu,
//! typ spojení (přímé / relay) a přenos dávky velikosti plného stavu hry.
//! Režim `lokalni` ověří spojení na jednom PC bez relay a bez internetu
//! (podklad pro rozhodnutí R14 v návrhu).

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, EndpointAddr, SecretKey};
use iroh_tickets::endpoint::EndpointTicket;

const ALPN: &[u8] = b"aae/nettest/1";
const PING_COUNT: u32 = 30;
const PING_INTERVAL: Duration = Duration::from_secs(1);
/// Odhad velikosti plného Snapshotu hry (návrh, Section 4 nález 3).
const PAYLOAD_BYTES: usize = 200 * 1024;
/// Kritérium „Stabilita“: aspoň 85 % potvrzení do 300 ms a žádné nad 1 s
/// (délka sběrného okna burzy).
const RTT_LIMIT: Duration = Duration::from_millis(300);
const RTT_SHARE_PERCENT: usize = 85;
const RTT_MAX: Duration = Duration::from_secs(1);

const KIND_PING: u8 = 1;
const KIND_PAYLOAD: u8 = 2;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("host") => host().await,
        Some("pripojit") => join(args.get(1).cloned()).await,
        Some("lokalni") => local_test().await,
        Some(other) => Err(anyhow::anyhow!(
            "neznámý příkaz „{other}“, použijte host, pripojit nebo lokalni"
        )),
        None => menu().await,
    };
    if let Err(err) = result {
        eprintln!("\nChyba: {err:#}");
    }
    wait_for_enter();
}

async fn menu() -> Result<()> {
    println!("AAE – síťový test");
    println!("  1) Hostovat (vytvořit pozvánku)");
    println!("  2) Připojit se (vložit pozvánku)");
    println!("  3) Lokální test na tomto PC (bez internetu)");
    match prompt("Volba [1/2/3]: ")?.as_str() {
        "1" => host().await,
        "2" => join(None).await,
        "3" => local_test().await,
        other => bail!("neplatná volba „{other}“"),
    }
}

// ---------------------------------------------------------------- hostitel

async fn host() -> Result<()> {
    let (secret, key_path) = load_or_create_key()?;
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("nepodařilo se otevřít síťový endpoint")?;

    println!("Zjišťuji adresy a relay server…");
    endpoint.online().await;

    let ticket = EndpointTicket::new(endpoint.addr()).to_string();
    println!("\nID hostitele: {} (stálé, klíč v {})", endpoint.id().fmt_short(), key_path.display());
    println!("\nPOZVÁNKA (pošlete ji kamarádovi celou):\n\n{ticket}\n");
    let ticket_file = exe_dir().join("pozvanka.txt");
    std::fs::write(&ticket_file, &ticket)
        .with_context(|| format!("nepodařilo se zapsat {}", ticket_file.display()))?;
    println!("Pozvánka je uložená v {}", ticket_file.display());
    if copy_to_clipboard(&ticket) {
        println!("…a zkopírovaná do schránky (Ctrl+V).");
    }
    println!("\nČekám na připojení. Ukončíte zavřením okna nebo Ctrl+C.\n");

    while let Some(incoming) = endpoint.accept().await {
        tokio::spawn(async move {
            let conn = match incoming.await {
                Ok(conn) => conn,
                Err(err) => {
                    eprintln!("Neúspěšné příchozí spojení: {err}");
                    return;
                }
            };
            let who = conn.remote_id().fmt_short();
            println!("[{who}] připojen · {}", describe_path(&conn));
            match serve(&conn).await {
                Ok(frames) => println!("[{who}] test dokončen, odpovězeno na {frames} zpráv · {}", describe_path(&conn)),
                Err(err) => println!("[{who}] spojení skončilo: {err:#}"),
            }
        });
    }
    Ok(())
}

/// Vrací každý přijatý rámec beze změny, dokud klient stream neukončí.
async fn serve(conn: &Connection) -> Result<u64> {
    let (mut send, mut recv) = conn.accept_bi().await?;
    let mut frames = 0u64;
    loop {
        let mut header = [0u8; 5];
        match recv.read_exact(&mut header).await {
            Ok(()) => {}
            // Klient zavřel stream: konec testu.
            Err(_) if frames > 0 => break,
            Err(err) => return Err(err.into()),
        }
        let len = u32::from_be_bytes(header[1..5].try_into()?) as usize;
        if len > PAYLOAD_BYTES {
            bail!("příliš velký rámec ({len} B)");
        }
        let mut body = vec![0u8; len];
        recv.read_exact(&mut body).await?;
        send.write_all(&header).await?;
        send.write_all(&body).await?;
        frames += 1;
    }
    send.finish()?;
    conn.closed().await;
    Ok(frames)
}

// ------------------------------------------------------------------ klient

async fn join(ticket: Option<String>) -> Result<()> {
    let ticket = match ticket {
        Some(t) => t,
        None => prompt("Vložte pozvánku (pravé tlačítko myši nebo Ctrl+V) a Enter:\n")?,
    };
    let ticket: EndpointTicket = ticket
        .trim()
        .parse()
        .context("pozvánka není platná, zkopírujte ji celou")?;

    let mut addr = EndpointAddr::from(ticket);
    // Ladicí režim: z pozvánky zahodí přímé adresy, takže spojení musí začít
    // přes relay. Ověří dostupnost relay serverů i z jedné sítě.
    if std::env::var_os("AAE_JEN_RELAY").is_some() {
        addr.addrs.retain(|a| a.is_relay());
        println!("Ladicí režim: spojení začne přes relay.");
    }

    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("nepodařilo se otevřít síťový endpoint")?;
    println!("Připojuji se k hostiteli…");
    let started = Instant::now();
    let conn = endpoint
        .connect(addr, ALPN)
        .await
        .context("nepodařilo se připojit k hostiteli")?;
    let connect_time = started.elapsed();
    println!("Připojeno za {} · {}\n", ms(connect_time), describe_path(&conn));

    let report = run_client_test(&conn, PING_COUNT, PING_INTERVAL).await?;
    conn.close(0u32.into(), b"hotovo");
    endpoint.close().await;

    let summary = report.summary(connect_time);
    println!("\n{summary}");
    let result_file = exe_dir().join("vysledek-site.txt");
    std::fs::write(&result_file, &summary)
        .with_context(|| format!("nepodařilo se zapsat {}", result_file.display()))?;
    println!("Výsledek je uložený v {} – pošlete ho prosím zpátky.", result_file.display());
    Ok(())
}

struct Report {
    rtts: Vec<Duration>,
    relay_pings: u32,
    direct_pings: u32,
    final_path: String,
    payload_roundtrip: Duration,
}

impl Report {
    fn summary(&self, connect_time: Duration) -> String {
        let min = self.rtts.iter().min().copied().unwrap_or_default();
        let max = self.rtts.iter().max().copied().unwrap_or_default();
        let avg = self.rtts.iter().sum::<Duration>() / self.rtts.len().max(1) as u32;
        let under_limit = self.rtts.iter().filter(|r| **r < RTT_LIMIT).count();
        let share_ok = under_limit * 100 >= self.rtts.len() * RTT_SHARE_PERCENT;
        let verdict = if share_ok && max <= RTT_MAX { "SPLNĚNO" } else { "NESPLNĚNO" };
        format!(
            "AAE síťový test – výsledek\n\
             Připojení trvalo: {}\n\
             Odezva (ping) min / průměr / max: {} / {} / {}\n\
             Pingů pod {} ms: {}/{} ({} %)\n\
             Kritérium (aspoň {RTT_SHARE_PERCENT} % pod {} ms a žádný nad {} ms): {verdict}\n\
             Typ spojení během testu: přímé {}×, relay {}×\n\
             Typ spojení na konci: {}\n\
             Dávka {} kB tam a zpět: {}\n",
            ms(connect_time),
            ms(min),
            ms(avg),
            ms(max),
            RTT_LIMIT.as_millis(),
            under_limit,
            self.rtts.len(),
            under_limit * 100 / self.rtts.len().max(1),
            RTT_LIMIT.as_millis(),
            RTT_MAX.as_millis(),
            self.direct_pings,
            self.relay_pings,
            self.final_path,
            PAYLOAD_BYTES / 1024,
            ms(self.payload_roundtrip),
        )
    }
}

/// Posílá pingy a jednu velkou dávku po jednom obousměrném streamu.
async fn run_client_test(conn: &Connection, pings: u32, interval: Duration) -> Result<Report> {
    let (mut send, mut recv) = conn.open_bi().await?;
    let mut report = Report {
        rtts: Vec::new(),
        relay_pings: 0,
        direct_pings: 0,
        final_path: String::new(),
        payload_roundtrip: Duration::ZERO,
    };

    for seq in 1..=pings {
        let rtt = roundtrip(&mut send, &mut recv, KIND_PING, &seq.to_be_bytes()).await?;
        let path = describe_path(conn);
        if path.starts_with("relay") {
            report.relay_pings += 1;
        } else {
            report.direct_pings += 1;
        }
        println!("ping {seq:>2}/{pings}: {:>7} · {path}", ms(rtt));
        report.rtts.push(rtt);
        tokio::time::sleep(interval).await;
    }

    println!("Posílám dávku {} kB (velikost plného stavu hry)…", PAYLOAD_BYTES / 1024);
    let payload = vec![0xA5u8; PAYLOAD_BYTES];
    report.payload_roundtrip = roundtrip(&mut send, &mut recv, KIND_PAYLOAD, &payload).await?;
    report.final_path = describe_path(conn);

    send.finish()?;
    Ok(report)
}

async fn roundtrip(
    send: &mut iroh::endpoint::SendStream,
    recv: &mut iroh::endpoint::RecvStream,
    kind: u8,
    body: &[u8],
) -> Result<Duration> {
    let mut header = [0u8; 5];
    header[0] = kind;
    header[1..5].copy_from_slice(&(body.len() as u32).to_be_bytes());
    let started = Instant::now();
    send.write_all(&header).await?;
    send.write_all(body).await?;
    let mut echo_header = [0u8; 5];
    recv.read_exact(&mut echo_header).await?;
    let mut echo = vec![0u8; body.len()];
    recv.read_exact(&mut echo).await?;
    let elapsed = started.elapsed();
    if echo_header != header || echo != body {
        bail!("hostitel vrátil jiná data, než dostal");
    }
    Ok(elapsed)
}

// ------------------------------------------------------- lokální test (R14)

/// Dva endpointy v jednom procesu, bez relay a bez discovery, spojené jen
/// přes lokální IP adresy. Ověřuje, že integrační test nad skutečným iroh
/// půjde spustit v CI bez internetu.
async fn local_test() -> Result<()> {
    println!("Lokální test: hostitel i klient na tomto PC, bez relay a bez internetu.");
    let server = Endpoint::builder(presets::Minimal)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("nepodařilo se otevřít endpoint hostitele")?;
    let server_addr = server.addr();
    let server_task = tokio::spawn({
        let server = server.clone();
        async move {
            let incoming = server.accept().await.context("hostitel byl zavřen")?;
            let conn = incoming.await?;
            serve(&conn).await
        }
    });

    let client = Endpoint::builder(presets::Minimal)
        .bind()
        .await
        .context("nepodařilo se otevřít endpoint klienta")?;
    let started = Instant::now();
    let conn = client
        .connect(server_addr, ALPN)
        .await
        .context("klient se nespojil s hostitelem přes lokální adresy")?;
    let connect_time = started.elapsed();
    println!("Připojeno za {} · {}", ms(connect_time), describe_path(&conn));

    let report = run_client_test(&conn, 10, Duration::from_millis(50)).await?;
    conn.close(0u32.into(), b"hotovo");
    let frames = server_task.await??;
    client.close().await;
    server.close().await;

    println!("\n{}Hostitel odpověděl na {frames} zpráv.", report.summary(connect_time));
    if report.relay_pings > 0 {
        bail!("lokální spojení použilo relay, i když bylo vypnuté");
    }
    println!("VÝSLEDEK: iroh funguje lokálně bez relay – integrační test v CI je proveditelný.");
    Ok(())
}

// ------------------------------------------------------------- pomocníci

/// Popis vybrané cesty spojení: přímé (IP), nebo přes relay, s odhadem RTT.
fn describe_path(conn: &Connection) -> String {
    let paths = conn.paths();
    match paths.iter().find(|p| p.is_selected()) {
        Some(p) if p.is_relay() => format!("relay, RTT {}", ms(p.rtt())),
        Some(p) => format!("přímé, RTT {}", ms(p.rtt())),
        None => "cesta zatím nevybrána".to_string(),
    }
}

/// Tajný klíč hostitele je trvalý, aby pozvánka nesla stále stejné ID
/// (návrh: „Pozvánka je stabilní“).
fn load_or_create_key() -> Result<(SecretKey, PathBuf)> {
    let dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(exe_dir)
        .join("aae");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("nettest-host.key");
    if let Ok(bytes) = std::fs::read(&path) {
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("poškozený klíč v {}", path.display()))?;
        return Ok((SecretKey::from_bytes(&bytes), path));
    }
    let key = SecretKey::generate();
    std::fs::write(&path, key.to_bytes())?;
    Ok((key, path))
}

fn copy_to_clipboard(text: &str) -> bool {
    use std::process::{Command, Stdio};
    // Automatizované běhy nesmí přepsat schránku uživatele.
    if std::env::var_os("AAE_BEZ_SCHRANKY").is_some() {
        return false;
    }
    let Ok(mut child) = Command::new("clip").stdin(Stdio::piped()).spawn() else {
        return false;
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    written && child.wait().is_ok_and(|s| s.success())
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn prompt(question: &str) -> Result<String> {
    print!("{question}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// Při spuštění dvojklikem by se okno po skončení hned zavřelo.
fn wait_for_enter() {
    print!("\nStiskněte Enter pro ukončení…");
    let _ = io::stdout().flush();
    let _ = io::stdin().lock().read_line(&mut String::new());
}

fn ms(d: Duration) -> String {
    format!("{:.0} ms", d.as_secs_f64() * 1000.0)
}
