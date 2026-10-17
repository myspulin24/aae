// Herní obrazovka: horní lišta, mapa s letovým plánem a boční panely.
import { useEffect, useRef, useState } from "react";

import { api } from "../api/client";
import type { GameDto } from "../api/bindings/GameDto";
import type { LegDto } from "../api/bindings/LegDto";
import type { RouteInfoDto } from "../api/bindings/RouteInfoDto";
import { airlineColor, kc, num } from "../format";
import { act, run, useApp } from "../store";
import { airportLabel, useWorld, type World } from "../world";
import { FinancePanel } from "./FinancePanel";
import { FleetPanel } from "./FleetPanel";
import { Gantt } from "./Gantt";
import { MapView } from "./MapView";
import { MarketPanel } from "./MarketPanel";
import { PlanEditor } from "./PlanEditor";
import { RivalsPanel } from "./RivalsPanel";
import { RoutesPanel } from "./RoutesPanel";
import { TopBar } from "./TopBar";

type Tab = "routes" | "fleet" | "market" | "finance" | "rivals";

const TABS: { id: Tab; label: string }[] = [
  { id: "routes", label: "Linky" },
  { id: "fleet", label: "Flotila" },
  { id: "market", label: "Burza" },
  { id: "finance", label: "Finance" },
  { id: "rivals", label: "Soupeři" },
];

export function Game() {
  const game = useApp((s) => s.game);
  const world = useWorld();
  const [tab, setTab] = useState<Tab>("routes");
  const [editing, setEditing] = useState<number | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [preview, setPreview] = useState<LegDto[] | null>(null);
  const pickRef = useRef<((airport: number) => void) | null>(null);

  if (!game || !world) return null;
  const aircraft = editing !== null ? game.my.fleet.find((p) => p.id === editing) : undefined;
  const openMarket = game.contracts.filter((c) => c.status === "offered").length;
  const needsAssign = game.contracts.some((c) => c.status === "awarded" && c.airline === game.me);

  return (
    <div className="game">
      <TopBar game={game} />
      <div className="game-main">
        <div className="game-left">
          <div className="map-area">
            <MapView
              game={game}
              world={world}
              selected={selected}
              preview={aircraft ? preview : null}
              picking={aircraft !== undefined}
              onAirport={(id) => {
                if (aircraft && pickRef.current) pickRef.current(id);
                else setSelected(selected === id ? null : id);
              }}
            />
            {selected !== null && !aircraft && (
              <AirportCard game={game} world={world} airport={selected} onClose={() => setSelected(null)} />
            )}
            <PauseBanner game={game} />
          </div>
          {aircraft ? (
            <PlanEditor
              key={aircraft.id}
              game={game}
              world={world}
              aircraft={aircraft}
              onClose={() => setEditing(null)}
              onPreview={setPreview}
              pickRef={pickRef}
            />
          ) : (
            <Gantt game={game} world={world} editing={editing} onEdit={setEditing} />
          )}
        </div>

        <aside className="game-side">
          <nav className="tabs">
            {TABS.map((t) => (
              <button key={t.id} className={tab === t.id ? "active" : ""} onClick={() => setTab(t.id)}>
                {t.label}
                {t.id === "market" && (needsAssign ? <span className="dot bad" /> : openMarket > 0 ? <span className="count">{openMarket}</span> : null)}
              </button>
            ))}
          </nav>
          <div className="side-body">
            {tab === "routes" && <RoutesPanel game={game} world={world} />}
            {tab === "fleet" && <FleetPanel game={game} world={world} onEdit={setEditing} />}
            {tab === "market" && <MarketPanel game={game} world={world} />}
            {tab === "finance" && <FinancePanel game={game} />}
            {tab === "rivals" && <RivalsPanel game={game} world={world} />}
          </div>
          <News />
        </aside>
      </div>
      <VoteCard game={game} />
      <Reconnecting />
      {game.clock.finished && <Finished game={game} world={world} />}
    </div>
  );
}

function News() {
  const news = useApp((s) => s.news);
  return (
    <section className="news">
      <h3>Zprávy</h3>
      <ul>
        {news.length === 0 && <li className="muted">Zatím nic nového.</li>}
        {news.map((n) => (
          <li key={n.seq} className={n.tone}>
            <span className="muted small">{n.time.split(" ").slice(-1)[0]}</span> {n.text}
          </li>
        ))}
      </ul>
    </section>
  );
}

function AirportCard({
  game,
  world,
  airport,
  onClose,
}: {
  game: GameDto;
  world: World;
  airport: number;
  onClose: () => void;
}) {
  const a = world.airport(airport);
  const home = game.my.home;
  const [info, setInfo] = useState<RouteInfoDto | null>(null);
  useEffect(() => {
    if (airport === home) {
      setInfo(null);
      return;
    }
    let alive = true;
    api.routeInfo(home, airport).then(
      (i) => alive && setInfo(i),
      () => alive && setInfo(null),
    );
    return () => {
      alive = false;
    };
  }, [airport, home]);

  const serving = game.airlines.filter((al) => al.network.some((r) => r.from === airport || r.to === airport));
  const weather = game.disruptions.filter((d) => d.airport === airport && d.day >= game.clock.day);

  return (
    <div className="airport-card">
      <header className="row spread">
        <strong>{airportLabel(a)}</strong>
        <button className="link" onClick={onClose}>
          ✕
        </button>
      </header>
      <p className="small muted">
        {a.country} · {num(a.populationK * 1000)} obyvatel{a.slotConstrained ? " · přetížené – potřeba sloty" : ""}
      </p>
      {info && (
        <p className="small">
          Z {world.airport(home).code}: {num(info.distanceKm)} km, ~{num(info.demand)}/{num(info.demandBack)} cestujících
          denně (tam/zpět), referenční cena {kc(info.reference)}.
        </p>
      )}
      {serving.length > 0 && (
        <p className="small">
          Létají sem:{" "}
          {serving.map((al) => (
            <span key={al.id} style={{ color: airlineColor(al.id) }}>
              {al.name}{" "}
            </span>
          ))}
        </p>
      )}
      {weather.map((d, i) => (
        <p key={i} className="small bad">
          {d.date}: {d.text}
        </p>
      ))}
    </div>
  );
}

function PauseBanner({ game }: { game: GameDto }) {
  const host = useApp((s) => s.status.host);
  const c = game.clock;
  if (!c.paused || c.finished) return null;
  return (
    <div className="pause-banner">
      <strong>Hra stojí</strong>
      <span>{c.pauses.map((p) => p.text).join(" · ")}</span>
      {c.pauses.map((p) => {
        if (p.kind === "player" && (p.airline === game.me || host)) {
          return (
            <button
              key={`p${p.airline}`}
              className="primary"
              onClick={() => void act({ type: "resume", airline: p.airline === game.me ? null : p.airline })}
            >
              {p.airline === game.me ? "Pokračovat" : "Ukončit pauzu"}
            </button>
          );
        }
        if (p.kind === "technical" && host) {
          return (
            <button key="t" className="primary" onClick={() => void run(api.hostPause(false))}>
              Spustit hru
            </button>
          );
        }
        if (p.kind === "auto" && host) {
          return (
            <button key="a" onClick={() => void run(api.hostResumeAuto())}>
              Pokračovat bez odpojených
            </button>
          );
        }
        return null;
      })}
    </div>
  );
}

function VoteCard({ game }: { game: GameDto }) {
  const v = game.vote;
  if (!v) return null;
  const proposer = game.airlines.find((a) => a.id === v.proposer)?.name ?? "?";
  return (
    <div className="vote-card">
      <strong>
        {proposer} navrhuje zrychlit na {v.speed}×
      </strong>
      <span className="muted small">
        souhlasí {v.yes.length} z {v.needed.length}
      </span>
      {v.waitingForMe ? (
        <div className="row">
          <button className="primary" onClick={() => void run(api.vote(true))}>
            Ano
          </button>
          <button onClick={() => void run(api.vote(false))}>Ne</button>
        </div>
      ) : (
        <span className="small">čeká se na ostatní…</span>
      )}
    </div>
  );
}

function Reconnecting() {
  const status = useApp((s) => s.status);
  if (status.phase !== "reconnecting") return null;
  return (
    <div className="overlay">
      <div className="card center">
        <div className="spinner" />
        <h2>Hostitel odpojen</h2>
        <p>{status.message}</p>
        <button onClick={() => void run(api.leave())}>Zpět do menu</button>
      </div>
    </div>
  );
}

function Finished({ game, world }: { game: GameDto; world: World }) {
  const winner = game.airlines[0];
  return (
    <div className="overlay">
      <div className="card center finished">
        <h2>Kampaň skončila</h2>
        <p>
          Vítězí <strong style={{ color: airlineColor(winner.id) }}>{winner.name}</strong>
          {winner.me ? " – to jste vy!" : "."}
        </p>
        <ol>
          {game.airlines.map((a) => (
            <li key={a.id} className={a.me ? "me" : ""}>
              {a.name} <span className="muted small">({world.airport(a.home).code})</span>
            </li>
          ))}
        </ol>
        <button className="primary" onClick={() => void run(api.leave())}>
          Zpět do menu
        </button>
      </div>
    </div>
  );
}
