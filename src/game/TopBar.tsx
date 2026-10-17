import { api } from "../api/client";
import { ConfirmButton } from "../components/ConfirmButton";
import type { GameDto } from "../api/bindings/GameDto";
import { hhmm, kc } from "../format";
import { pathText } from "../screens/Lobby";
import { act, run, useApp } from "../store";
import { useGameMinute } from "./clock";

export function TopBar({ game }: { game: GameDto }) {
  const status = useApp((s) => s.status);
  const minute = useGameMinute(4);
  const c = game.clock;
  const my = game.my;
  const rank = game.airlines.find((a) => a.me)?.rank ?? 0;
  const result = my.stats.revenueYesterday - my.stats.costYesterday;

  return (
    <header className="topbar">
      <div className="logo small">AAE</div>
      <div className="clock">
        <span className="date">{c.date}</span>
        <span className="time">{hhmm(minute % 1440)}</span>
        <span className="muted small">
          den {c.day + 1}/{c.durationDays}
        </span>
      </div>

      <div className="speed" role="group" aria-label="Rychlost hry">
        {[1, 2, 4].map((s) => (
          <button
            key={s}
            className={c.speed === s ? "active" : ""}
            title={
              s < c.speed
                ? "Zpomalit (platí hned)"
                : s > c.speed
                  ? "Navrhnout zrychlení (hlasují všichni)"
                  : "Aktuální rychlost"
            }
            disabled={c.speed === s || c.finished || game.vote !== null}
            onClick={() => void run(api.proposeSpeed(s))}
          >
            {s}×
          </button>
        ))}
      </div>

      {c.myPause ? (
        <button className="primary" onClick={() => void act({ type: "resume", airline: null })}>
          ▶ Pokračovat
        </button>
      ) : (
        <button
          disabled={c.pausesLeft === 0 || c.finished}
          title="Hráčská pauza: nejvýš 5 minut, 3× za večer"
          onClick={() => void act({ type: "pause" })}
        >
          ❚❚ Pauza ({c.pausesLeft})
        </button>
      )}

      <div className="stat">
        <span className="muted small">Hotovost</span>
        <strong className={my.cash < 0 ? "bad" : ""}>{kc(my.cash)}</strong>
      </div>
      <div className="stat">
        <span className="muted small">Včera</span>
        <strong className={result < 0 ? "bad" : "good"}>{kc(result)}</strong>
      </div>
      <div className="stat">
        <span className="muted small">Palivo</span>
        <strong>{game.fuelPerL.toFixed(2).replace(".", ",")} Kč/l</strong>
      </div>
      <div className="stat">
        <span className="muted small">Pořadí</span>
        <strong>
          {rank}. z {game.airlines.length}
        </strong>
      </div>

      <div className="spacer" />
      <div className={`path ${status.path.kind}`} title="Spojení s hostitelem">
        {status.host ? "hostitel" : pathText(status.path)}
      </div>
      {status.saved && <div className="muted small">{status.saved}</div>}
      <ConfirmButton
        confirm={status.host ? "Uložit a ukončit?" : "Opravdu odejít?"}
        title={
          status.host
            ? "Kampaň se uloží a hosté se odpojí."
            : "Vrátit se můžete stejnou pozvánkou."
        }
        onConfirm={() => void run(api.leave())}
      >
        Odejít
      </ConfirmButton>
    </header>
  );
}
