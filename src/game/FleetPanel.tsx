// Flotila: vlastní letadla (plán, vrácení, prodej) a nová letadla na trhu.
import type { GameDto } from "../api/bindings/GameDto";
import { ConfirmButton } from "../components/ConfirmButton";
import { duration, kc, num } from "../format";
import { act } from "../store";
import type { World } from "../world";
import { blockMinutes } from "./flight";

export function FleetPanel({
  game,
  world,
  onEdit,
}: {
  game: GameDto;
  world: World;
  onEdit: (aircraft: number) => void;
}) {
  const my = game.my;
  return (
    <div className="panel">
      <h3>Vaše letadla ({my.fleet.length})</h3>
      <table className="list">
        <tbody>
          {my.fleet.map((p) => {
            const t = world.type(p.model);
            const busy = p.charters.length > 0;
            return (
              <tr key={p.id}>
                <td>
                  <strong>{t.name}</strong> <span className="muted">#{p.id}</span>
                  <div className="muted small">
                    {t.seats} míst · {p.schedule.length} letů denně · {duration(blockMinutes(p.schedule))} ve vzduchu
                  </div>
                </td>
                <td className="small">
                  {p.ownership === "leased" ? (
                    <>
                      pronájem {kc(t.leasePerDay)}/den
                      <div className="muted">záloha {kc(p.value)}</div>
                    </>
                  ) : (
                    <>
                      vlastní
                      <div className="muted">hodnota {kc(p.value)}</div>
                    </>
                  )}
                </td>
                <td className="right nowrap">
                  <button onClick={() => onEdit(p.id)}>Plán</button>{" "}
                  {p.ownership === "leased" ? (
                    <ConfirmButton
                      confirm="Vrátit?"
                      disabled={busy}
                      title={busy ? "Má naplánovaný charter" : "Vrátí letadlo, záloha se vrátí"}
                      onConfirm={() => void act({ type: "return", aircraft: p.id }, "Letadlo vráceno.")}
                    >
                      Vrátit
                    </ConfirmButton>
                  ) : (
                    <ConfirmButton
                      confirm={`Prodat za ${kc(p.value * 0.8)}?`}
                      disabled={busy}
                      onConfirm={() => void act({ type: "sell", aircraft: p.id }, "Letadlo prodáno.")}
                    >
                      Prodat
                    </ConfirmButton>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      <h3>Nové letadlo</h3>
      <p className="muted small">
        Pronájem: záloha za 30 dní předem (vrátí se), pak denní nájem. Koupě: celá cena hned, letadlo ztrácí 6 %
        hodnoty ročně a zvyšuje úvěrový rámec. Každé letadlo stojí navíc 30 000 Kč denně na režii.
      </p>
      <table className="list">
        <thead>
          <tr>
            <th>Typ</th>
            <th>Míst</th>
            <th>Dolet</th>
            <th>Nájem/den</th>
            <th>Cena</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {world.types.map((t) => (
            <tr key={t.id}>
              <td>
                <strong>{t.name}</strong>
                <div className="muted small">
                  {num(t.speedKmh)} km/h · posádka {kc(t.crewPerHour)}/h
                </div>
              </td>
              <td>{t.seats}</td>
              <td>{num(t.rangeKm)} km</td>
              <td>{kc(t.leasePerDay)}</td>
              <td>{kc(t.price)}</td>
              <td className="right nowrap">
                <button
                  disabled={my.cash < t.deposit}
                  title={`Záloha ${kc(t.deposit)}`}
                  onClick={() => void act({ type: "lease", model: t.id }, `${t.name} pronajato.`)}
                >
                  Pronajmout
                </button>{" "}
                <ConfirmButton
                  confirm={`Koupit za ${kc(t.price)}?`}
                  disabled={my.cash < t.price}
                  onConfirm={() => void act({ type: "buy", model: t.id }, `${t.name} koupeno.`)}
                >
                  Koupit
                </ConfirmButton>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
