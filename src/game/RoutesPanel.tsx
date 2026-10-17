// Linky a ceny letenek: vytížení, tržby, ceny konkurence.
import { useState } from "react";

import type { GameDto } from "../api/bindings/GameDto";
import type { MyRouteDto } from "../api/bindings/MyRouteDto";
import { kc, num, percent } from "../format";
import { act } from "../store";
import type { World } from "../world";

function PriceInput({ route }: { route: MyRouteDto }) {
  const [value, setValue] = useState<string | null>(null);
  const shown = value ?? String(Math.round(route.price));

  function submit(price: number) {
    setValue(null);
    if (!Number.isFinite(price) || Math.round(price) === Math.round(route.price)) return;
    const clamped = Math.min(route.max, Math.max(route.min, Math.round(price / 10) * 10));
    void act({ type: "setPrice", from: route.from, to: route.to, price: clamped }, `Cena nastavena na ${kc(clamped)}.`);
  }

  return (
    <span className="price-edit">
      <button className="link" title="−10 %" onClick={() => submit(route.price * 0.9)}>
        −
      </button>
      <input
        inputMode="numeric"
        value={shown}
        onChange={(e) => setValue(e.target.value.replace(/[^\d]/g, ""))}
        onBlur={() => value !== null && submit(Number(value))}
        onKeyDown={(e) => {
          if (e.key === "Enter") submit(Number(shown));
          if (e.key === "Escape") setValue(null);
        }}
      />
      <button className="link" title="+10 %" onClick={() => submit(route.price * 1.1)}>
        +
      </button>
    </span>
  );
}

export function RoutesPanel({ game, world }: { game: GameDto; world: World }) {
  const routes = game.my.routes;
  const name = (id: number) => game.airlines.find((a) => a.id === id)?.name ?? "?";

  return (
    <div className="panel">
      <p className="muted small">
        Cena platí pro jeden směr. Poptávka se dělí mezi aerolinky podle ceny, služeb a časů odletů; příliš
        drahá linka letí poloprázdná. Čísla „včera“ jsou za celý den.
      </p>
      {routes.length === 0 ? (
        <p className="muted">Zatím nelétáte žádnou linku. Naplánujte lety v letovém plánu dole.</p>
      ) : (
        <table className="list routes-table">
          <thead>
            <tr>
              <th>Linka</th>
              <th>Cena (Kč)</th>
              <th title="Obsazenost včera">Včera</th>
              <th>Tržby včera</th>
              <th>Konkurence</th>
            </tr>
          </thead>
          <tbody>
            {routes.map((r) => {
              const y = r.yesterday;
              const cheapest = r.rivals.length > 0 ? Math.min(...r.rivals.map((x) => x.price)) : null;
              return (
                <tr key={`${r.from}-${r.to}`} className={r.served ? "" : "muted"}>
                  <td>
                    <strong>
                      {world.airport(r.from).code}→{world.airport(r.to).code}
                    </strong>
                    <div className="muted small">
                      {num(r.distanceKm)} km · ref. {kc(r.reference)}
                    </div>
                  </td>
                  <td>
                    <PriceInput route={r} />
                  </td>
                  <td>
                    {y ? (
                      <>
                        {percent(y.pax, y.seats)}
                        <div className="muted small">
                          {num(y.pax)}/{num(y.seats)}
                        </div>
                      </>
                    ) : (
                      "–"
                    )}
                  </td>
                  <td>{y ? kc(y.revenue) : "–"}</td>
                  <td title={r.rivals.map((x) => `${name(x.airline)}: ${kc(x.price)}`).join("\n")}>
                    {cheapest === null ? (
                      <span className="muted">nikdo</span>
                    ) : (
                      <>
                        {r.rivals.length}× od {kc(cheapest)}
                      </>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </div>
  );
}
