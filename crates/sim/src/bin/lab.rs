//! Laboratoř ekonomiky: odsimuluje kampaň se 4 AI aerolinkami bez grafiky
//! a vypíše, jestli ekonomika dává smysl (návrh: Komponenty 2, úkol T4).
//!
//! Použití:
//!   lab [--seed N] [--runs K] [--days D | --years Y] [--speed 1|2|4] [--csv SOUBOR] [--hash]
//!
//! `--hash` vypíše jen kontrolní hash konečného stavu (test determinismu
//! přes dva samostatné procesy).

use std::collections::BTreeMap;
use std::io::Write;
use std::process::ExitCode;
use std::time::Instant;

use aae_sim::{CampaignRules, Controller, GameState, NewAirline, Speed, StepError, Strategy};

struct Args {
    seed: u64,
    runs: u64,
    days: Option<u64>,
    speed: Speed,
    csv: Option<String>,
    hash_only: bool,
    report_every: Option<u64>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        seed: 1,
        runs: 10,
        days: None,
        speed: Speed::X1,
        csv: None,
        hash_only: false,
        report_every: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} potřebuje hodnotu"));
        match flag.as_str() {
            "--seed" => args.seed = value()?.parse().map_err(|_| "--seed: číslo")?,
            "--runs" => args.runs = value()?.parse().map_err(|_| "--runs: číslo")?,
            "--days" => args.days = Some(value()?.parse().map_err(|_| "--days: číslo")?),
            "--years" => {
                let years: u64 = value()?.parse().map_err(|_| "--years: číslo")?;
                args.days = Some(years * 365 + years / 4 + 1);
            }
            "--speed" => {
                args.speed = match value()?.as_str() {
                    "1" => Speed::X1,
                    "2" => Speed::X2,
                    "4" => Speed::X4,
                    other => return Err(format!("--speed: 1, 2 nebo 4, ne „{other}“")),
                }
            }
            "--csv" => args.csv = Some(value()?),
            "--hash" => args.hash_only = true,
            "--report" => args.report_every = Some(value()?.parse().map_err(|_| "--report: počet dní")?),
            "-h" | "--help" => {
                return Err("lab [--seed N] [--runs K] [--days D | --years Y] [--speed 1|2|4] [--csv SOUBOR] [--hash]".into())
            }
            other => return Err(format!("neznámý přepínač „{other}“")),
        }
    }
    Ok(args)
}

const HOMES: [&str; 4] = ["PRG", "VIE", "MUC", "WAW"];

fn new_campaign(seed: u64, days: Option<u64>, speed: Speed) -> GameState {
    let mut rules = CampaignRules::default();
    if let Some(d) = days {
        rules.duration_days = d;
    }
    rules.initial_speed = speed;
    let world = aae_sim::data::World::europe_v1().expect("zabudovaná data");
    // Domovská letiště se mezi běhy střídají, aby výsledek strategie
    // nezávisel na jednom městě.
    let airlines = Strategy::ALL
        .iter()
        .enumerate()
        .map(|(i, s)| NewAirline {
            name: format!("AI {}", s.name()),
            home: world
                .airport_by_code(HOMES[(i + seed as usize) % HOMES.len()])
                .expect("letiště"),
            controller: Controller::Ai(*s),
        })
        .collect();
    GameState::with_world(world, rules, seed, airlines).expect("platné nastavení")
}

/// Stejné pořadí jako hostitelská smyčka: záměry AI na hranici kroku, pak krok.
/// S `report_every` vypisuje každých N dní včerejší tržby, náklady a hotovost.
fn run(state: &mut GameState, report_every: Option<u64>) {
    let mut rejects: BTreeMap<String, u64> = BTreeMap::new();
    loop {
        for (actor, intent) in state.ai_tick() {
            if let Err(reject) = state.apply(actor, intent) {
                let key = format!("{reject:?}");
                let key = key
                    .split(['{', '('])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                *rejects.entry(key).or_default() += 1;
            }
        }
        match state.step() {
            Ok(events) => {
                if let Some(every) = report_every {
                    let new_day = events
                        .iter()
                        .any(|e| matches!(e, aae_sim::Event::DayStarted { .. }));
                    if new_day && state.day() % every == 0 {
                        report(state);
                    }
                }
            }
            Err(StepError::Finished) => break,
            Err(StepError::Paused) => unreachable!("laboratoř nepauzuje"),
        }
    }
    if report_every.is_some() {
        println!("  odmítnuté záměry AI: {rejects:?}");
    }
}

fn report(state: &GameState) {
    print!("  den {:>4}:", state.day());
    for a in state.airlines.values() {
        print!(
            "  [{} fl {:>2} tržby {:>9} náklady {:>9} hotovost {:>12}]",
            &a.name[3..],
            a.fleet.len(),
            a.stats.revenue_yesterday.whole_kc(),
            a.stats.cost_yesterday.whole_kc(),
            a.cash.whole_kc(),
        );
    }
    println!();
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };

    if args.hash_only {
        let mut state = new_campaign(args.seed, args.days, args.speed);
        run(&mut state, args.report_every);
        println!("{:016x}", state.hash());
        return ExitCode::SUCCESS;
    }

    let mut csv = String::from(
        "seed;strategie;domov;hodnota_kc;flotila;cestujici;obsazenost_pct;bankroty;zakazky;chartery;sloty;lety_zrusene\n",
    );
    let mut wins: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut value_sum: BTreeMap<&'static str, i64> = BTreeMap::new();
    let mut bankruptcies: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut slowest = 0.0f64;

    for r in 0..args.runs {
        let seed = args.seed + r;
        let mut state = new_campaign(seed, args.days, args.speed);
        let started = Instant::now();
        run(&mut state, args.report_every);
        let secs = started.elapsed().as_secs_f64();
        slowest = slowest.max(secs);

        let mut best: Option<(i64, &'static str)> = None;
        println!(
            "běh {} (seed {seed}, {} dní, {:.2} s):",
            r + 1,
            state.day(),
            secs
        );
        for (id, a) in &state.airlines {
            let Controller::Ai(strategy) = a.controller else {
                continue;
            };
            let name = strategy.name();
            let value = state.firm_value(*id);
            let load = (a.stats.total_pax * 100)
                .checked_div(a.stats.total_seats)
                .unwrap_or(0);
            println!(
                "  {:<9} {:<4} hodnota {:>16}  flotila {:>3}  cestujících {:>9}  obsazenost {:>3} %  bankroty {}  zakázky {}  sloty {}",
                name,
                state.world.airport(a.home).code,
                value.to_string(),
                a.fleet.len(),
                a.stats.total_pax,
                load,
                a.generation,
                a.stats.contracts_won,
                a.stats.slots_won,
            );
            csv.push_str(&format!(
                "{seed};{name};{};{};{};{};{load};{};{};{};{};{}\n",
                state.world.airport(a.home).code,
                value.whole_kc(),
                a.fleet.len(),
                a.stats.total_pax,
                a.generation,
                a.stats.contracts_won,
                a.stats.charters_flown,
                a.stats.slots_won,
                a.stats.cancelled_flights,
            ));
            *value_sum.entry(name).or_default() += value.whole_kc();
            *bankruptcies.entry(name).or_default() += a.generation as u64;
            if best.is_none_or(|(v, _)| value.whole_kc() > v) {
                best = Some((value.whole_kc(), name));
            }
        }
        if let Some((_, name)) = best {
            *wins.entry(name).or_default() += 1;
        }
    }

    println!("\nSouhrn ({} běhů):", args.runs);
    for s in Strategy::ALL {
        let name = s.name();
        println!(
            "  {:<9} výher {:>3}  průměrná hodnota {:>16} Kč  bankrotů {}",
            name,
            wins.get(name).copied().unwrap_or(0),
            value_sum.get(name).copied().unwrap_or(0) / args.runs.max(1) as i64,
            bankruptcies.get(name).copied().unwrap_or(0),
        );
    }
    let top = wins.values().copied().max().unwrap_or(0);
    println!(
        "  nejčastější vítěz vyhrál {} % běhů; nejpomalejší běh {:.2} s",
        top * 100 / args.runs.max(1),
        slowest
    );

    if let Some(path) = args.csv {
        match std::fs::File::create(&path).and_then(|mut f| f.write_all(csv.as_bytes())) {
            Ok(()) => println!("CSV: {path}"),
            Err(e) => {
                eprintln!("nepodařilo se zapsat {path}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
