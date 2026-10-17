// Burza zakázek a aukce slotů. Během pauzy jsou obě zavřené.
import { useState } from "react";

import { api } from "../api/client";
import type { ContractDto } from "../api/bindings/ContractDto";
import type { GameDto } from "../api/bindings/GameDto";
import { airlineColor, hhmm, kc, num } from "../format";
import { act } from "../store";
import type { World } from "../world";
import { useGameMinute } from "./clock";

function when(minute: number, now: number, speed: number): string {
  const left = minute - now;
  if (left <= 0) return "teď";
  const realSec = left / (16 * speed);
  if (realSec < 90) return `za ~${Math.ceil(realSec)} s`;
  if (Math.floor(minute / 1440) === Math.floor(now / 1440)) return `v ${hhmm(minute)}`;
  return `${Math.floor(minute / 1440) - Math.floor(now / 1440) === 1 ? "zítra" : "později"} v ${hhmm(minute)}`;
}

function AssignCharter({ c, game, world }: { c: ContractDto; game: GameDto; world: World }) {
  const eligible = game.my.fleet.filter((p) => {
    const t = world.type(p.model);
    return t.seats >= c.pax && t.rangeKm >= c.distanceKm && !p.charters.some((x) => x.day === c.day);
  });
  const [pick, setPick] = useState<number | null>(eligible[0]?.id ?? null);
  if (eligible.length === 0) {
    return <span className="bad small">Žádné vhodné letadlo (místa, dolet, volný den).</span>;
  }
  return (
    <span className="row">
      <select value={pick ?? ""} onChange={(e) => setPick(Number(e.target.value))}>
        {eligible.map((p) => (
          <option key={p.id} value={p.id}>
            {world.type(p.model).code} #{p.id}
          </option>
        ))}
      </select>
      <button
        className="primary"
        disabled={pick === null}
        onClick={() =>
          pick !== null &&
          void act({ type: "assignCharter", contract: c.id, aircraft: pick }, "Letadlo přiřazeno k zakázce.")
        }
      >
        Přiřadit
      </button>
    </span>
  );
}

export function MarketPanel({ game, world }: { game: GameDto; world: World }) {
  const minute = useGameMinute(2);
  const now = Math.floor(minute);
  const closed = game.clock.paused;
  const name = (id: number) => game.airlines.find((a) => a.id === id)?.name ?? "?";
  const offered = game.contracts.filter((c) => c.status === "offered");
  const mine = game.contracts.filter(
    (c) => c.airline === game.me && (c.status === "awarded" || c.status === "assigned"),
  );
  const auctions = game.auctions.filter((a) => a.status === "open");
  const [bids, setBids] = useState<Record<number, string>>({});

  return (
    <div className="panel">
      {closed && <div className="banner warn">Hra stojí – burza i aukce jsou zavřené.</div>}

      {mine.length > 0 && (
        <>
          <h3>Vaše zakázky</h3>
          <table className="list">
            <tbody>
              {mine.map((c) => (
                <tr key={c.id}>
                  <td>
                    <strong>
                      {world.airport(c.from).code}→{world.airport(c.to).code}
                    </strong>{" "}
                    <span className="muted small">
                      {c.pax} cest. · {num(c.distanceKm)} km · {c.date}
                    </span>
                    <div className="small">odměna {kc(c.reward)}</div>
                  </td>
                  <td className="right">
                    {c.status === "assigned" ? (
                      <span className="good small">letí letadlem #{c.aircraft}</span>
                    ) : (
                      <AssignCharter c={c} game={game} world={world} />
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="muted small">
            Nepřiřazená zakázka do jejího dne propadne s penále. Letadlo ten den neletí svůj běžný plán.
          </p>
        </>
      )}

      <h3>Burza zakázek</h3>
      {offered.length === 0 ? (
        <p className="muted">Teď nic. Nové zakázky přibývají během dne.</p>
      ) : (
        <table className="list">
          <tbody>
            {offered.map((c) => {
              const applied = c.candidates.includes(game.me);
              const others = c.watching.filter((a) => a !== game.me);
              return (
                <tr
                  key={c.id}
                  onMouseEnter={() => void api.presence(c.id).catch(() => undefined)}
                  onMouseLeave={() => void api.presence(null).catch(() => undefined)}
                >
                  <td>
                    <strong>
                      {world.airport(c.from).code}→{world.airport(c.to).code}
                    </strong>{" "}
                    <span className="muted small">
                      {world.airport(c.from).city} – {world.airport(c.to).city}
                    </span>
                    <div className="small">
                      {c.pax} cestujících · {num(c.distanceKm)} km · let {c.date}
                    </div>
                    <div className="muted small">
                      nabídka končí {when(c.offerUntil, now, game.clock.speed)}
                      {c.windowDue !== null && ` · los ${when(c.windowDue, now, game.clock.speed)}`}
                      {c.candidates.length > 0 && ` · zájemci: ${c.candidates.length}`}
                    </div>
                    {others.length > 0 && (
                      <div className="watching small">
                        {others.map((a) => (
                          <span key={a} style={{ color: airlineColor(a) }}>
                            ● {name(a)} se dívá{" "}
                          </span>
                        ))}
                      </div>
                    )}
                  </td>
                  <td className="right nowrap">
                    <div>
                      <strong>{kc(c.reward)}</strong>
                    </div>
                    {applied ? (
                      <span className="good small">přihlášeno</span>
                    ) : (
                      <button
                        className="primary"
                        disabled={closed}
                        onClick={() =>
                          void act({ type: "takeContract", contract: c.id }, "Přihlášeno – o zakázce rozhodne los.")
                        }
                      >
                        Vzít
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}

      <h3>Aukce slotů</h3>
      <p className="muted small">
        Slot = jeden denní odlet z přetíženého letiště (LHR, CDG, FRA, AMS). Pozdní příhoz prodlouží aukci o pár
        sekund.
      </p>
      {auctions.length === 0 ? (
        <p className="muted">Žádná aukce neběží.</p>
      ) : (
        <table className="list">
          <tbody>
            {auctions.map((a) => {
              const leading = a.highBidder === game.me;
              const bid = bids[a.id] || String(Math.round(a.minBid));
              return (
                <tr key={a.id}>
                  <td>
                    <strong>{world.airport(a.airport).city}</strong> ({world.airport(a.airport).code})
                    <div className="small">
                      {a.highBid !== null && a.highBidder !== null ? (
                        <>
                          vede{" "}
                          <span style={{ color: airlineColor(a.highBidder) }}>
                            {leading ? "vaše nabídka" : name(a.highBidder)}
                          </span>{" "}
                          {kc(a.highBid)}
                        </>
                      ) : (
                        <span className="muted">zatím bez příhozu</span>
                      )}
                    </div>
                    <div className="muted small">
                      končí {when(a.effectiveEnd, now, game.clock.speed)}
                      {a.protected && " (prodlouženo)"} · minimum {kc(a.minBid)}
                    </div>
                  </td>
                  <td className="right nowrap">
                    <input
                      className="bid"
                      inputMode="numeric"
                      value={bid}
                      onChange={(e) => setBids({ ...bids, [a.id]: e.target.value.replace(/[^\d]/g, "") })}
                    />{" "}
                    <button
                      className="primary"
                      disabled={closed || leading || Number(bid) < a.minBid || Number(bid) > game.my.cash}
                      onClick={() => {
                        void act({ type: "bid", auction: a.id, amount: Number(bid) }, "Příhoz přijat.");
                        setBids({ ...bids, [a.id]: "" });
                      }}
                    >
                      Přihodit
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      {game.my.slots.length > 0 && (
        <p className="small">
          Vaše sloty:{" "}
          {game.my.slots.map((s) => `${world.airport(s.airport).code} ${s.used}/${s.owned}`).join(", ")}
        </p>
      )}
    </div>
  );
}
