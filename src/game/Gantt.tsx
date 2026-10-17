// Letový plán flotily na 24 hodin: dnešní lety, změny od zítřka a čára „teď“.
import type { GameDto } from "../api/bindings/GameDto";
import type { LegDto } from "../api/bindings/LegDto";
import { airlineColor, hhmm } from "../format";
import type { World } from "../world";
import { useGameMinute } from "./clock";
import { blockMinutes } from "./flight";

const HOURS = [0, 3, 6, 9, 12, 15, 18, 21, 24];

function Segments({ legs, world, faded = false }: { legs: LegDto[]; world: World; faded?: boolean }) {
  const parts: { key: string; left: number; width: number; leg: LegDto }[] = [];
  legs.forEach((l, i) => {
    const end = Math.min(l.arr, 1440);
    parts.push({ key: `${i}a`, left: l.dep, width: end - l.dep, leg: l });
    if (l.arr > 1440) parts.push({ key: `${i}b`, left: 0, width: l.arr - 1440, leg: l });
  });
  return (
    <>
      {parts.map((p) => (
        <div
          key={p.key}
          className={`seg ${faded ? "faded" : ""}`}
          style={{ left: `${(p.left / 1440) * 100}%`, width: `${(p.width / 1440) * 100}%` }}
          title={`${world.airport(p.leg.from).code} ${hhmm(p.leg.dep)} → ${world.airport(p.leg.to).code} ${hhmm(p.leg.arr)} · ${p.leg.distanceKm} km`}
        >
          {p.width > 70 ? `${world.airport(p.leg.from).code}–${world.airport(p.leg.to).code}` : ""}
        </div>
      ))}
    </>
  );
}

export function Gantt({
  game,
  world,
  editing,
  onEdit,
}: {
  game: GameDto;
  world: World;
  editing: number | null;
  onEdit: (aircraft: number) => void;
}) {
  const minute = useGameMinute(2);
  const now = ((minute % 1440) / 1440) * 100;
  const color = airlineColor(game.me);

  return (
    <section className="gantt">
      <div className="gantt-head">
        <div className="gantt-label">
          <strong>Letový plán</strong> <span className="muted small">klikněte na letadlo</span>
        </div>
        <div className="gantt-track hours">
          {HOURS.map((h) => (
            <span key={h} style={{ left: `${(h / 24) * 100}%` }}>
              {h}:00
            </span>
          ))}
        </div>
      </div>
      <div className="gantt-rows" style={{ ["--seg" as string]: color }}>
        {game.my.fleet.length === 0 && (
          <p className="muted pad">Nemáte žádné letadlo. Pronajměte si ho v záložce Flotila.</p>
        )}
        {game.my.fleet.map((p) => {
          const t = world.type(p.model);
          const changed = JSON.stringify(p.schedule) !== JSON.stringify(p.today);
          const charter = p.charters.find((c) => c.day === game.clock.day);
          const util = Math.round((blockMinutes(p.today) / 1440) * 100);
          return (
            <button
              key={p.id}
              className={`gantt-row ${editing === p.id ? "editing" : ""}`}
              onClick={() => onEdit(p.id)}
            >
              <div className="gantt-label">
                <strong>{t.code}</strong> <span className="muted">#{p.id}</span>
                <span className="muted small"> · {util} % dne ve vzduchu</span>
                {charter && <span className="badge">charter</span>}
                {p.ownership === "owned" && <span className="badge">vlastní</span>}
              </div>
              <div className="gantt-track">
                {HOURS.slice(1, -1).map((h) => (
                  <i key={h} className="tick" style={{ left: `${(h / 24) * 100}%` }} />
                ))}
                {p.today.length === 0 && !changed && <span className="empty">bez plánu – stojí na zemi</span>}
                <Segments legs={p.today} world={world} />
                {changed && (
                  <div className="tomorrow" title="Nový plán platí od zítřka">
                    <Segments legs={p.schedule} world={world} faded />
                  </div>
                )}
                <i className="now" style={{ left: `${now}%` }} />
              </div>
            </button>
          );
        })}
      </div>
    </section>
  );
}
