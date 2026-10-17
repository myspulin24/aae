// Pořadí aerolinek, soupeři a ovládání hostitele (pauza, čekárna, uložení).
import { useState } from "react";

import { api } from "../api/client";
import type { GameDto } from "../api/bindings/GameDto";
import { STRATEGY_NAMES, airlineColor } from "../format";
import { Invitation } from "../screens/Lobby";
import { run, useApp } from "../store";
import type { World } from "../world";

export function RivalsPanel({ game, world }: { game: GameDto; world: World }) {
  const status = useApp((s) => s.status);
  const waiting = useApp((s) => s.waiting);
  const [assign, setAssign] = useState<Record<string, number>>({});
  const technical = game.clock.pauses.some((p) => p.kind === "technical");
  const auto = game.clock.pauses.some((p) => p.kind === "auto");

  return (
    <div className="panel">
      <h3>Pořadí</h3>
      <table className="list">
        <tbody>
          {game.airlines.map((a) => (
            <tr key={a.id} className={a.me ? "me" : ""}>
              <td>{a.rank}.</td>
              <td>
                <span className="swatch" style={{ background: airlineColor(a.id) }} />
                <strong>{a.name}</strong>
                <div className="muted small">
                  {a.human ? (a.online ? "hráč · online" : "hráč · offline") : `počítač · ${STRATEGY_NAMES[a.strategy ?? "hub"]}`}{" "}
                  · domov {world.airport(a.home).code}
                </div>
              </td>
              <td className="small">
                {a.fleet.map((f) => `${f.count}× ${world.type(f.model).code}`).join(", ") || "bez letadel"}
                <div className="muted">{new Set(a.network.map((r) => [r.from, r.to].sort().join("-"))).size} linek</div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="muted small">
        Pořadí je podle hodnoty firmy. Hotovost, úvěry a obsazenost soupeřů nevidíte – jen jejich linky, ceny a flotilu.
      </p>

      {status.host && (
        <>
          <h3>Hostitel</h3>
          <div className="row wrap">
            <button onClick={() => void run(api.hostPause(!technical))}>
              {technical ? "▶ Spustit hru" : "❚❚ Technická pauza"}
            </button>
            {auto && (
              <button className="primary" onClick={() => void run(api.hostResumeAuto())}>
                Pokračovat bez odpojených
              </button>
            )}
            <button onClick={() => void run(api.hostSave(), "Kampaň uložena.")}>Uložit teď</button>
          </div>
          {status.campaignName && <p className="muted small">Kampaň: {status.campaignName}</p>}

          {waiting.length > 0 && (
            <>
              <h3>Čekají na vpuštění</h3>
              {waiting.map((w) => {
                const target = assign[w.key] ?? game.airlines.find((a) => !a.human)?.id ?? game.airlines[0].id;
                return (
                  <div key={w.key} className="waiting-peer">
                    <strong>{w.name}</strong> <span className="muted small">v{w.version}</span>
                    <div className="row wrap">
                      <select value={target} onChange={(e) => setAssign({ ...assign, [w.key]: Number(e.target.value) })}>
                        {game.airlines.map((a) => (
                          <option key={a.id} value={a.id}>
                            {a.name} {a.human ? "(hráč – převod klíče)" : "(počítač)"}
                          </option>
                        ))}
                      </select>
                      <button className="primary" onClick={() => void run(api.hostAssign(w.key, target))}>
                        Vpustit
                      </button>
                      <button onClick={() => void run(api.hostRefuse(w.key))}>Odmítnout</button>
                    </div>
                  </div>
                );
              })}
            </>
          )}

          {status.invitation && (
            <>
              <h3>Pozvánka</h3>
              <Invitation text={status.invitation} />
            </>
          )}
        </>
      )}
    </div>
  );
}
