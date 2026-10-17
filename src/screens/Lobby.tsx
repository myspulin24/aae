import { useEffect, useState } from "react";

import { api } from "../api/client";
import type { PathDto } from "../api/bindings/PathDto";
import type { StrategyDto } from "../api/bindings/StrategyDto";
import { STRATEGY_HINTS, STRATEGY_NAMES, kcShort } from "../format";
import { run, toast, useApp } from "../store";
import { airportLabel, useWorld } from "../world";

const DURATIONS = [
  { days: 92, label: "3 měsíce" },
  { days: 365, label: "1 rok" },
  { days: 731, label: "2 roky" },
  { days: 1826, label: "5 let" },
];
const CAPITALS = [15_000_000, 25_000_000, 40_000_000];
const STRATEGIES: StrategyDto[] = ["levna", "premiova", "hub", "charter"];

export function pathText(p: PathDto): string {
  switch (p.kind) {
    case "local":
      return "hostitel";
    case "direct":
      return `přímé spojení · ${p.rttMs} ms`;
    case "relay":
      return `přes relay · ${p.rttMs} ms`;
    default:
      return "připojuje se…";
  }
}

export function Invitation({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      toast("Kopírování se nepovedlo, označte pozvánku a zkopírujte ji ručně.", "error");
    }
  }
  return (
    <div className="invitation">
      <textarea readOnly rows={3} value={text} onFocus={(e) => e.currentTarget.select()} />
      <button className="primary" onClick={() => void copy()}>
        {copied ? "Zkopírováno" : "Kopírovat pozvánku"}
      </button>
    </div>
  );
}

export function Lobby() {
  const world = useWorld();
  const lobby = useApp((s) => s.lobby);
  const status = useApp((s) => s.status);
  const me = lobby?.players.find((p) => p.me);
  const [name, setName] = useState(me?.name ?? "");
  const [campaignName, setCampaignName] = useState(status.campaignName ?? "");

  useEffect(() => {
    if (me) setName(me.name);
  }, [me?.name]);

  if (!lobby || !world) return null;
  const host = status.host;
  const rules = lobby.rules;
  const freeSlots = lobby.maxAirlines - lobby.players.length - lobby.ai.length;
  const usedHomes = new Set(lobby.players.filter((p) => !p.me).map((p) => p.home));

  function setRules(patch: Partial<{ durationDays: number; startingCapital: number; initialSpeed: number; events: boolean }>) {
    void run(
      api.hostSetRules({
        durationDays: rules.durationDays,
        startingCapital: rules.startingCapital,
        initialSpeed: rules.initialSpeed,
        events: rules.events,
        ...patch,
      }),
    );
  }

  function setAi(ai: StrategyDto[]) {
    void run(api.hostSetAi(ai));
  }

  function setProfile(newName: string, home: number) {
    if (!newName.trim()) return;
    void run(api.lobbySetProfile(newName.trim(), home));
  }

  return (
    <div className="screen lobby">
      <header className="row spread">
        <div>
          <h1>{host ? "Vaše hra" : "Lobby"}</h1>
          <p className="muted">
            {host
              ? "Pošlete kamarádům pozvánku. Až budou všichni v seznamu, zahajte kampaň."
              : "Čeká se, až hostitel zahájí kampaň."}
          </p>
        </div>
        <button onClick={() => void run(api.leave())}>Odejít</button>
      </header>

      <div className="lobby-grid">
        <section className="card">
          {host && status.invitation && (
            <>
              <h2>Pozvánka</h2>
              <Invitation text={status.invitation} />
              <p className="muted small">
                Pozvánku pošlete třeba přes chat. Platí, dokud máte hru otevřenou.
              </p>
            </>
          )}

          <h2>Hráči ({lobby.players.length}/4)</h2>
          <table className="list">
            <tbody>
              {lobby.players.map((p) => (
                <tr key={p.key} className={p.me ? "me" : ""}>
                  <td>
                    <strong>{p.name || "…"}</strong> {p.host && <span className="badge">hostitel</span>}
                    {p.me && <span className="badge">vy</span>}
                  </td>
                  <td>{world.airport(p.home) ? airportLabel(world.airport(p.home)) : "–"}</td>
                  <td className={`path ${p.path.kind}`}>{p.connected ? pathText(p.path) : "odpojen"}</td>
                  <td className="muted">v{p.version}</td>
                </tr>
              ))}
            </tbody>
          </table>

          {me && (
            <>
              <h2>Vaše aerolinka</h2>
              <div className="form">
                <label>
                  Jméno
                  <input
                    value={name}
                    maxLength={24}
                    onChange={(e) => setName(e.target.value)}
                    onBlur={() => name.trim() !== me.name && setProfile(name, me.home)}
                    onKeyDown={(e) => e.key === "Enter" && setProfile(name, me.home)}
                  />
                </label>
                <label>
                  Domovské letiště
                  <select value={me.home} onChange={(e) => setProfile(name || me.name, Number(e.target.value))}>
                    {world.byCity.map((a) => (
                      <option key={a.id} value={a.id} disabled={usedHomes.has(a.id)}>
                        {airportLabel(a)}
                        {a.slotConstrained ? " – sloty" : ""}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <p className="muted small">
                Letiště označená „sloty“ jsou přetížená: odlety z nich potřebují sloty z aukcí.
              </p>
            </>
          )}
        </section>

        <section className="card">
          <h2>Pravidla kampaně</h2>
          {host && (
            <div className="form">
              <label>
                Název kampaně
                <input
                  value={campaignName}
                  maxLength={60}
                  onChange={(e) => setCampaignName(e.target.value)}
                  onBlur={() => void run(api.hostSetCampaignName(campaignName))}
                />
              </label>
            </div>
          )}
          <div className="form">
            <label>
              Délka
              <select
                disabled={!host}
                value={rules.durationDays}
                onChange={(e) => setRules({ durationDays: Number(e.target.value) })}
              >
                {DURATIONS.map((d) => (
                  <option key={d.days} value={d.days}>
                    {d.label}
                  </option>
                ))}
                {!DURATIONS.some((d) => d.days === rules.durationDays) && (
                  <option value={rules.durationDays}>{rules.durationDays} dní</option>
                )}
              </select>
            </label>
            <label>
              Počáteční kapitál
              <select
                disabled={!host}
                value={rules.startingCapital}
                onChange={(e) => setRules({ startingCapital: Number(e.target.value) })}
              >
                {CAPITALS.map((c) => (
                  <option key={c} value={c}>
                    {kcShort(c)}
                  </option>
                ))}
                {!CAPITALS.includes(rules.startingCapital) && (
                  <option value={rules.startingCapital}>{kcShort(rules.startingCapital)}</option>
                )}
              </select>
            </label>
            <label>
              Rychlost na začátku
              <select
                disabled={!host}
                value={rules.initialSpeed}
                onChange={(e) => setRules({ initialSpeed: Number(e.target.value) })}
              >
                <option value={1}>1× (den = 90 s)</option>
                <option value={2}>2×</option>
                <option value={4}>4×</option>
              </select>
            </label>
            <label className="check">
              <input
                type="checkbox"
                disabled={!host}
                checked={rules.events}
                onChange={(e) => setRules({ events: e.target.checked })}
              />
              Události (stávky, počasí)
            </label>
          </div>
          <p className="muted small">Na začátku dostane každý {rules.startingFleet} v pronájmu.</p>

          <h2>Soupeři řízení počítačem ({lobby.ai.length})</h2>
          <ul className="ai-list">
            {lobby.ai.map((s, i) => (
              <li key={i}>
                <span>
                  <strong>{STRATEGY_NAMES[s]}</strong> <span className="muted">– {STRATEGY_HINTS[s]}</span>
                </span>
                {host && (
                  <button className="link" onClick={() => setAi(lobby.ai.filter((_, j) => j !== i))}>
                    Odebrat
                  </button>
                )}
              </li>
            ))}
          </ul>
          {host && freeSlots > 0 && (
            <div className="row wrap">
              {STRATEGIES.map((s) => (
                <button key={s} onClick={() => setAi([...lobby.ai, s])}>
                  + {STRATEGY_NAMES[s]}
                </button>
              ))}
            </div>
          )}
          {host && (
            <div className="start">
              <button
                className="primary big"
                disabled={lobby.players.length + lobby.ai.length < 2}
                onClick={() => void run(api.hostStartCampaign())}
              >
                Zahájit kampaň
              </button>
              {lobby.players.length + lobby.ai.length < 2 && (
                <p className="muted small">Přidejte aspoň jednoho soupeře (hráče nebo počítač).</p>
              )}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
