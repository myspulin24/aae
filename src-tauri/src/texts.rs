//! České texty pro hráče: zprávy z událostí simulace, důvody odmítnutí
//! záměrů a konce spojení.

use aae_net::protocol::{EndReason, HostReject, RefuseReason, version_is_newer};
use aae_net::view::View;
use aae_sim::calendar::date_of_day;
use aae_sim::data::World;
use aae_sim::model::{AirlineId, AirportId, ContractId, Controller, PauseReason};
use aae_sim::schedule::ScheduleError;
use aae_sim::time::{day_of, minute_of_day};
use aae_sim::{Event, Reject};

use crate::dto::{NewsDto, Tone};

pub fn clock(minute_of_day: u64) -> String {
    format!("{:02}:{:02}", minute_of_day / 60, minute_of_day % 60)
}

pub fn date(day: u64) -> String {
    date_of_day(day).to_string()
}

fn airport(world: &World, id: AirportId) -> String {
    if world.is_valid_airport(id) {
        let a = world.airport(id);
        format!("{} ({})", a.city, a.code)
    } else {
        "?".into()
    }
}

fn code(world: &World, id: AirportId) -> String {
    if world.is_valid_airport(id) {
        world.airport(id).code.clone()
    } else {
        "?".into()
    }
}

pub fn schedule_error(world: &World, e: &ScheduleError) -> (Option<usize>, String) {
    match e {
        ScheduleError::TooManyLegs => (None, "Plán má víc než 24 letů za den.".into()),
        ScheduleError::DepartureOutOfDay { leg } => (
            Some(*leg),
            format!("Let {}: odlet musí být mezi 00:00 a 23:59.", leg + 1),
        ),
        ScheduleError::NotSorted { leg } => (
            Some(*leg),
            format!("Let {}: odlety musí jít po sobě v čase.", leg + 1),
        ),
        ScheduleError::UnknownAirport { leg } => {
            (Some(*leg), format!("Let {}: neznámé letiště.", leg + 1))
        }
        ScheduleError::SameAirport { leg } => (
            Some(*leg),
            format!(
                "Let {}: odlet a přílet nemůžou být na stejném letišti.",
                leg + 1
            ),
        ),
        ScheduleError::OutOfRange {
            leg,
            distance_km,
            range_km,
        } => (
            Some(*leg),
            format!(
                "Let {}: trasa má {distance_km} km, letadlo doletí nejvýš {range_km} km.",
                leg + 1
            ),
        ),
        ScheduleError::NotChained { leg } => (
            Some(*leg),
            format!(
                "Let {} musí začínat tam, kde skončil předchozí let.",
                leg + 1
            ),
        ),
        ScheduleError::TurnaroundTooShort { leg } => (
            Some(*leg),
            format!(
                "Let {}: letadlo po předchozím letu nestihne obrátku.",
                leg + 1
            ),
        ),
        ScheduleError::NotCyclic => (
            None,
            "Poslední let se musí vrátit na letiště prvního letu – plán se opakuje každý den."
                .into(),
        ),
        ScheduleError::CycleTooLong => (
            None,
            "Poslední let přiletí pozdě na zítřejší první odlet.".into(),
        ),
        ScheduleError::MissingSlots {
            airport: a,
            needed,
            owned,
        } => (
            None,
            format!(
                "{}: plán potřebuje {needed} odletů denně, máte {owned} slotů. Sloty se získávají v aukcích.",
                code(world, *a)
            ),
        ),
    }
}

pub fn reject(world: &World, r: &Reject) -> String {
    match r {
        Reject::NotPermitted => "Na tohle nemáte právo.".into(),
        Reject::MarketClosedDuringPause => "Během pauzy je burza i aukce zavřená.".into(),
        Reject::CampaignFinished => "Kampaň už skončila.".into(),
        Reject::UnknownAirline => "Neznámá aerolinka.".into(),
        Reject::UnknownAircraft => "Tohle letadlo už ve flotile není.".into(),
        Reject::UnknownAircraftType => "Neznámý typ letadla.".into(),
        Reject::UnknownAirport => "Neznámé letiště.".into(),
        Reject::UnknownContract => "Zakázka už neexistuje.".into(),
        Reject::UnknownAuction => "Aukce už neexistuje.".into(),
        Reject::InvalidSchedule(e) => schedule_error(world, e).1,
        Reject::InsufficientFunds { needed } => {
            format!("Nemáte dost peněz, potřeba je {needed}.")
        }
        Reject::PriceOutOfBounds { min, max } => {
            format!("Cena musí být mezi {min} a {max}.")
        }
        Reject::InvalidServiceLevel => "Úroveň služeb musí být 1 až 3.".into(),
        Reject::LoanLimit { max } => format!("Úvěrový rámec je nejvýš {max}."),
        Reject::InvalidAmount => "Neplatná částka.".into(),
        Reject::AlreadyPaused => "Hra už je pozastavená.".into(),
        Reject::NotPaused => "Tahle pauza už neběží.".into(),
        Reject::SameSpeed => "Hra už běží touto rychlostí.".into(),
        Reject::ContractNotOffered => "Zakázka už není v nabídce.".into(),
        Reject::AlreadyCandidate => "O zakázku už se ucházíte.".into(),
        Reject::ContractNotYours => "Tahle zakázka vám nepatří.".into(),
        Reject::AircraftTooSmall => "Letadlo má na tuto zakázku málo míst.".into(),
        Reject::OutOfRange => "Letadlo trasu zakázky nedoletí.".into(),
        Reject::AircraftBusyThatDay => "Letadlo má na ten den jinou zakázku.".into(),
        Reject::ContractDayPassed => "Den zakázky už minul.".into(),
        Reject::AircraftHasCharter => {
            "Letadlo má naplánovaný charter, teď ho nejde vrátit ani prodat.".into()
        }
        Reject::AuctionClosed => "Aukce už skončila.".into(),
        Reject::BidTooLow { min } => format!("Příhoz je nízký, minimum je {min}."),
        Reject::AlreadyHighestBidder => "Váš příhoz je už nejvyšší.".into(),
    }
}

pub fn host_reject(world: &World, r: &HostReject) -> String {
    match r {
        HostReject::Sim(r) => reject(world, r),
        HostReject::PauseLimit => "Vyčerpali jste 3 pauzy na tento večer.".into(),
        HostReject::NotInGame => "Kampaň ještě nezačala.".into(),
        HostReject::VoteRunning => "Už probíhá hlasování o rychlosti.".into(),
        HostReject::NoVote => "Žádné hlasování neprobíhá.".into(),
    }
}

pub fn refuse(r: &RefuseReason) -> String {
    match r {
        RefuseReason::VersionMismatch { host, client } => {
            if version_is_newer(host, client) {
                format!(
                    "Hostitel má novější verzi AAE ({host}), vy máte {client}. Aktualizujte hru a připojte se znovu."
                )
            } else {
                format!(
                    "Máte novější verzi AAE ({client}) než hostitel ({host}). Aktualizovat musí hostitel."
                )
            }
        }
        RefuseReason::WorldMismatch => {
            "Hostitel má jiná herní data. Aktualizujte oba na stejnou verzi.".into()
        }
        RefuseReason::ProtocolMismatch => {
            "Hostitel používá jinou verzi síťového protokolu. Aktualizujte oba na stejnou verzi."
                .into()
        }
        RefuseReason::Refused => "Hostitel vaše připojení odmítl.".into(),
        RefuseReason::Full => "Hra je plná (nejvýš 4 hráči).".into(),
    }
}

pub fn ended(r: &EndReason) -> String {
    match r {
        EndReason::HostQuit => "Hostitel ukončil hru.".into(),
        EndReason::Refused => "Hostitel vás odpojil.".into(),
    }
}

pub fn pause_reason(view: Option<&View>, p: &PauseReason) -> String {
    match p {
        PauseReason::Player(a) => format!("pauza hráče {}", airline_name(view, *a)),
        PauseReason::Technical => "technická pauza hostitele".into(),
        PauseReason::Auto => "čeká se na odpojené hráče".into(),
    }
}

fn airline_name(view: Option<&View>, id: AirlineId) -> String {
    view.and_then(|v| v.airlines.get(&id))
        .map(|a| a.airline.name.clone())
        .unwrap_or_else(|| format!("aerolinka {}", id.0 + 1))
}

fn contract_route(world: &World, view: Option<&View>, id: ContractId) -> String {
    view.and_then(|v| v.contracts.iter().find(|c| c.id == id))
        .map(|c| format!("{} → {}", code(world, c.from), code(world, c.to)))
        .unwrap_or_else(|| format!("č. {}", id.0))
}

/// Zprávy pro hráče z událostí. Část událostí se nehlásí (hráčovy vlastní
/// akce, které potvrzuje Ack, a šum jako změny ceny paliva).
pub fn news(world: &World, view: Option<&View>, events: &[Event], seq: &mut u32) -> Vec<NewsDto> {
    let me = view.map(|v| v.me);
    let now = view.map(|v| v.header.now).unwrap_or(0);
    let mine = |a: &AirlineId| Some(*a) == me;
    let name = |a: AirlineId| airline_name(view, a);
    let serves = |from: AirportId, to: AirportId| {
        view.and_then(|v| v.airlines.get(&v.me))
            .map(|a| a.network.contains(&(from, to)))
            .unwrap_or(false)
    };
    let mut out = Vec::new();
    for e in events {
        let item: Option<(Tone, String)> = match e {
            Event::DayStarted { .. } => None,
            Event::CampaignEnded { .. } => Some((Tone::Good, "Kampaň skončila.".into())),
            Event::Paused(p) => {
                Some((Tone::Warn, format!("Hra stojí: {}.", pause_reason(view, p))))
            }
            Event::Resumed(p) => Some((
                Tone::Info,
                format!("Konec pauzy ({}).", pause_reason(view, p)),
            )),
            Event::SpeedChanged(s) => Some((
                Tone::Info,
                format!("Rychlost hry je teď {}×.", s.multiplier()),
            )),
            Event::FlightDeparted { .. } => None,
            Event::FlightCancelled {
                from,
                to,
                compensation,
                ..
            } => Some((
                Tone::Bad,
                format!(
                    "Zrušený let {} → {}, odškodné cestujícím {compensation}.",
                    code(world, *from),
                    code(world, *to)
                ),
            )),
            Event::CharterFlown {
                contract, reward, ..
            } => Some((
                Tone::Good,
                format!(
                    "Charter {} odlétán, odměna {reward}.",
                    contract_route(world, view, *contract)
                ),
            )),
            Event::ContractOffered { contract } => view
                .and_then(|v| v.contracts.iter().find(|c| c.id == *contract))
                .map(|c| {
                    (
                        Tone::Info,
                        format!(
                            "Nová zakázka: {} → {}, {} cestujících, {}, odměna {}.",
                            code(world, c.from),
                            code(world, c.to),
                            c.pax,
                            date(c.day),
                            c.reward
                        ),
                    )
                })
                .or(Some((Tone::Info, "Nová zakázka na burze.".into()))),
            Event::ContractWindowOpened { .. } | Event::ContractCandidate { .. } => None,
            Event::ContractAwarded {
                contract,
                winner,
                candidates,
            } => {
                let route = contract_route(world, view, *contract);
                if mine(winner) {
                    Some((
                        Tone::Good,
                        format!("Zakázku {route} jste získali. Přiřaďte jí letadlo."),
                    ))
                } else if me.is_some_and(|m| candidates.contains(&m)) {
                    Some((
                        Tone::Warn,
                        format!(
                            "Zakázku {route} vylosovala {} ({} zájemci).",
                            name(*winner),
                            candidates.len()
                        ),
                    ))
                } else {
                    Some((
                        Tone::Info,
                        format!("Zakázku {route} získala {}.", name(*winner)),
                    ))
                }
            }
            Event::ContractExpired { .. } => None,
            Event::ContractFailed {
                contract, penalty, ..
            } => Some((
                Tone::Bad,
                format!(
                    "Zakázka {} propadla, penále {penalty}.",
                    contract_route(world, view, *contract)
                ),
            )),
            Event::CharterAssigned { .. } => None,
            Event::AuctionOpened { airport: a, .. } => Some((
                Tone::Info,
                format!("Začala aukce slotu na letišti {}.", airport(world, *a)),
            )),
            Event::BidPlaced {
                auction,
                by,
                amount,
                ..
            } => {
                if mine(by) {
                    None
                } else {
                    let place = view
                        .and_then(|v| v.auctions.iter().find(|x| x.id == *auction))
                        .map(|x| code(world, x.airport))
                        .unwrap_or_default();
                    Some((
                        Tone::Warn,
                        format!("{} přihazuje {amount} na slot {place}.", name(*by)),
                    ))
                }
            }
            Event::AuctionWon {
                airport: a,
                winner,
                price,
                ..
            } => {
                if mine(winner) {
                    Some((
                        Tone::Good,
                        format!("Vyhráli jste slot {} za {price}.", code(world, *a)),
                    ))
                } else {
                    Some((
                        Tone::Info,
                        format!(
                            "Slot {} získala {} za {price}.",
                            code(world, *a),
                            name(*winner)
                        ),
                    ))
                }
            }
            Event::AuctionNoBids { .. } => None,
            Event::FuelPriceChanged { .. } => None,
            Event::DisruptionAnnounced { description, .. } => {
                Some((Tone::Warn, format!("Výhled: {description}.")))
            }
            Event::AircraftAdded { airline, .. } => (!mine(airline))
                .then(|| (Tone::Info, format!("{} rozšiřuje flotilu.", name(*airline)))),
            Event::AircraftRemoved { airline, .. } => (!mine(airline))
                .then(|| (Tone::Info, format!("{} zmenšuje flotilu.", name(*airline)))),
            Event::ScheduleSet { .. } => None,
            Event::PriceSet {
                airline,
                from,
                to,
                price,
            } => (!mine(airline) && serves(*from, *to)).then(|| {
                (
                    Tone::Warn,
                    format!(
                        "{} mění cenu na lince {} → {} na {price}.",
                        name(*airline),
                        code(world, *from),
                        code(world, *to)
                    ),
                )
            }),
            Event::LoanChanged { .. } => None,
            Event::BankruptcyWarning { negative_days, .. } => {
                let limit = view.map(|v| v.rules.bankruptcy_days).unwrap_or(7);
                Some((
                    Tone::Bad,
                    format!(
                        "Pozor: hotovost je záporná už {negative_days}. den. Po {limit} dnech přijde bankrot."
                    ),
                ))
            }
            Event::Bankruptcy {
                airline,
                new_capital,
            } => {
                if mine(airline) {
                    Some((
                        Tone::Bad,
                        format!(
                            "Vaše aerolinka zkrachovala. Začínáte znovu s kapitálem {new_capital}."
                        ),
                    ))
                } else {
                    Some((
                        Tone::Info,
                        format!("{} zkrachovala a začíná znovu.", name(*airline)),
                    ))
                }
            }
            Event::ControllerChanged {
                airline,
                controller,
            } => Some((
                Tone::Info,
                match controller {
                    Controller::Human => format!("Aerolinku {} převzal hráč.", name(*airline)),
                    Controller::Ai(_) => format!("Aerolinku {} teď řídí počítač.", name(*airline)),
                },
            )),
        };
        if let Some((tone, text)) = item {
            *seq += 1;
            out.push(NewsDto {
                seq: *seq,
                day: day_of(now) as u32,
                time: format!("{} {}", date(day_of(now)), clock(minute_of_day(now))),
                tone,
                text,
            });
        }
    }
    out
}

