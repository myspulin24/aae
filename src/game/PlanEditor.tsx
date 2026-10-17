// Editor denního plánu jednoho letadla. Lety jdou po sobě, plán se každý
// den opakuje a platí od zítřka. Kontrolu dělá jádro (stejná pravidla jako
// simulace), letiště jde přidat i kliknutím na mapu.
import { useEffect, useMemo, useState, type MutableRefObject } from "react";

import { api, errorText } from "../api/client";
import type { AircraftDto } from "../api/bindings/AircraftDto";
import type { GameDto } from "../api/bindings/GameDto";
import type { LegInput } from "../api/bindings/LegInput";
import type { PlanPreviewDto } from "../api/bindings/PlanPreviewDto";
import type { RouteInfoDto } from "../api/bindings/RouteInfoDto";
import { ConfirmButton } from "../components/ConfirmButton";
import { TimeInput } from "../components/TimeInput";
import { ceil5, duration, hhmm, kc, num } from "../format";
import { act, toast } from "../store";
import { airportLabel, type World } from "../world";
import { blockMinutes } from "./flight";

export function PlanEditor({
  game,
  world,
  aircraft,
  onClose,
  onPreview,
  pickRef,
}: {
  game: GameDto;
  world: World;
  aircraft: AircraftDto;
  onClose: () => void;
  onPreview: (legs: PlanPreviewDto["legs"] | null) => void;
  pickRef: MutableRefObject<((airport: number) => void) | null>;
}) {
  const type = world.type(aircraft.model);
  const [legs, setLegs] = useState<LegInput[]>(() =>
    aircraft.schedule.map((l) => ({ from: l.from, to: l.to, dep: l.dep })),
  );
  const [start, setStart] = useState<number>(aircraft.schedule[0]?.from ?? aircraft.base);
  const [preview, setPreview] = useState<PlanPreviewDto | null>(null);
  // Pro které lety platí poslední kontrola (do odpovědi jádra je plán neověřený).
  const [checked, setChecked] = useState<LegInput[] | null>(null);
  const [focus, setFocus] = useState<number | null>(null);
  const [info, setInfo] = useState<RouteInfoDto | null>(null);
  const [shuttleDest, setShuttleDest] = useState<number | null>(null);
  const [shuttleFrom, setShuttleFrom] = useState(360);
  const [shuttleUntil, setShuttleUntil] = useState(1410);

  const origin = legs[0]?.from ?? start;
  const tail = legs.length > 0 ? legs[legs.length - 1].to : start;

  // Kontrola v jádře po každé změně.
  useEffect(() => {
    let alive = true;
    const t = window.setTimeout(() => {
      api.planPreview(aircraft.id, aircraft.model, legs).then(
        (p) => {
          if (!alive) return;
          setPreview(p);
          setChecked(legs);
          onPreview(p.legs);
        },
        (e) => alive && toast(errorText(e), "error"),
      );
    }, 120);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [legs, aircraft.id, aircraft.model]);

  useEffect(() => () => onPreview(null), []);

  // Informace o lince vybraného letu (poptávka, konkurence).
  const focusLeg = focus !== null ? legs[focus] : legs[legs.length - 1];
  useEffect(() => {
    if (!focusLeg) {
      setInfo(null);
      return;
    }
    let alive = true;
    api.routeInfo(focusLeg.from, focusLeg.to).then(
      (i) => alive && setInfo(i),
      () => alive && setInfo(null),
    );
    return () => {
      alive = false;
    };
  }, [focusLeg?.from, focusLeg?.to]);

  function nextDeparture(): number {
    const last = preview?.legs[preview.legs.length - 1];
    if (legs.length === 0) return 360;
    if (last && preview?.legs.length === legs.length) return ceil5(last.arr + type.turnaroundMin);
    return ceil5(legs[legs.length - 1].dep + 90);
  }

  function addLeg(to: number) {
    const from = legs.length > 0 ? legs[legs.length - 1].to : start;
    if (to === from) {
      toast("Letadlo už na tomhle letišti stojí. Vyberte jiný cíl.", "error");
      return;
    }
    const dep = nextDeparture();
    if (dep >= 1440) {
      toast("Další let se do dne nevejde.", "error");
      return;
    }
    setLegs([...legs, { from, to, dep }]);
    setFocus(null);
  }

  // Kliknutí na letiště v mapě přidá let.
  pickRef.current = addLeg;
  useEffect(
    () => () => {
      pickRef.current = null;
    },
    [pickRef],
  );

  function update(i: number, patch: Partial<LegInput>) {
    setLegs(legs.map((l, j) => (j === i ? { ...l, ...patch } : l)));
  }

  function remove(i: number) {
    setLegs(legs.filter((_, j) => j !== i));
    setFocus(null);
  }

  async function shuttle() {
    if (shuttleDest === null) return;
    const s = shuttleFrom;
    const u = shuttleUntil;
    try {
      const generated = await api.planShuttle(aircraft.model, origin, shuttleDest, s, u <= s ? u + 1440 : u);
      setLegs(generated);
    } catch (e) {
      toast(errorText(e), "error");
    }
  }

  function save() {
    void act({ type: "setSchedule", aircraft: aircraft.id, legs }, "Plán uložen. Platí od zítřka.");
    onClose();
  }

  // Cíle seřazené podle vzdálenosti, jen v doletu.
  const destinations = useMemo(
    () =>
      world.airports
        .filter((a) => a.id !== tail)
        .map((a) => ({ a, km: world.km(tail, a.id) }))
        .filter((x) => x.km <= type.rangeKm * 1.02)
        .sort((x, y) => x.km - y.km),
    [world, tail, type.rangeKm],
  );
  const shuttleTargets = useMemo(
    () =>
      world.airports
        .filter((a) => a.id !== origin)
        .map((a) => ({ a, km: world.km(origin, a.id) }))
        .filter((x) => x.km <= type.rangeKm * 1.02)
        .sort((x, y) => x.km - y.km),
    [world, origin, type.rangeKm],
  );

  const pl = preview?.legs ?? [];
  const fresh = checked === legs;
  const valid = fresh && preview !== null && preview.error === null;
  const block = blockMinutes(pl);
  const slotNeeds = preview?.slots ?? [];

  return (
    <section className="plan-editor">
      <header className="row spread">
        <div>
          <h2>
            Plán letadla {type.name} <span className="muted">#{aircraft.id}</span>
          </h2>
          <p className="muted small">
            {type.seats} míst · dolet {num(type.rangeKm)} km · obrátka {type.turnaroundMin} min · plán se opakuje
            každý den a platí od zítřka
          </p>
        </div>
        <button onClick={onClose}>Zavřít</button>
      </header>

      <div className="plan-body">
        <div className="plan-legs">
          {legs.length === 0 && (
            <div className="form inline">
              <label>
                Výchozí letiště
                <select value={start} onChange={(e) => setStart(Number(e.target.value))}>
                  {world.byCity.map((a) => (
                    <option key={a.id} value={a.id}>
                      {airportLabel(a)}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          )}
          <table className="list legs">
            <thead>
              <tr>
                <th>#</th>
                <th>Odkud</th>
                <th>Kam</th>
                <th>Odlet</th>
                <th>Přílet</th>
                <th>km</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {legs.map((l, i) => {
                const p = pl[i];
                const bad = preview?.errorLeg === i;
                return (
                  <tr
                    key={i}
                    className={`${bad ? "bad-row" : ""} ${focus === i ? "focus" : ""}`}
                    onClick={() => setFocus(i)}
                  >
                    <td>{i + 1}</td>
                    <td>{world.airport(l.from).code}</td>
                    <td>
                      <select value={l.to} onChange={(e) => update(i, { to: Number(e.target.value) })}>
                        {world.byCity.map((a) => (
                          <option key={a.id} value={a.id}>
                            {a.code} {a.city}
                          </option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <TimeInput label={`Odlet letu ${i + 1}`} value={l.dep} onChange={(dep) => update(i, { dep })} />
                    </td>
                    <td className="muted">{p ? hhmm(p.arr) + (p.arr >= 1440 ? " +1" : "") : "–"}</td>
                    <td className="muted">{p ? num(p.distanceKm) : "–"}</td>
                    <td>
                      <button className="link" title="Odebrat let" onClick={() => remove(i)}>
                        ✕
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>

          <div className="row wrap">
            <select
              value=""
              onChange={(e) => e.target.value !== "" && addLeg(Number(e.target.value))}
              aria-label="Přidat let"
            >
              <option value="">+ Přidat let z {world.airport(tail).code} do…</option>
              {destinations.map(({ a, km }) => (
                <option key={a.id} value={a.id}>
                  {a.code} {a.city} · {num(km)} km
                </option>
              ))}
            </select>
            {legs.length > 0 && tail !== origin && (
              <button onClick={() => addLeg(origin)}>Zpět do {world.airport(origin).code}</button>
            )}
            <span className="muted small">nebo klikněte na letiště v mapě</span>
          </div>

          <div className="shuttle">
            <strong>Kyvadlo</strong>
            <span>
              {world.airport(origin).code} ⇄
            </span>
            <select value={shuttleDest ?? ""} onChange={(e) => setShuttleDest(e.target.value === "" ? null : Number(e.target.value))}>
              <option value="">cíl…</option>
              {shuttleTargets.map(({ a, km }) => (
                <option key={a.id} value={a.id}>
                  {a.code} {a.city} · {num(km)} km
                </option>
              ))}
            </select>
            <label>
              od <TimeInput value={shuttleFrom} onChange={setShuttleFrom} />
            </label>
            <label>
              poslední přílet{" "}
              <TimeInput value={shuttleUntil} onChange={setShuttleUntil} />
            </label>
            <button disabled={shuttleDest === null} onClick={() => void shuttle()}>
              Vyplnit
            </button>
          </div>
        </div>

        <aside className="plan-side">
          {!fresh && legs.length > 0 ? (
            <p className="muted">Kontroluji plán…</p>
          ) : preview?.error ? (
            <p className="error">{preview.error}</p>
          ) : legs.length === 0 ? (
            <p className="muted">Prázdný plán: letadlo zůstane stát (nájem a režie se platí dál).</p>
          ) : (
            <p className="ok">
              Plán je v pořádku: {legs.length} letů, ve vzduchu {duration(block)} denně.
            </p>
          )}
          {slotNeeds.length > 0 && (
            <ul className="slots">
              {slotNeeds.map((s) => (
                <li key={s.airport} className={s.used > s.owned ? "bad" : ""}>
                  Sloty {world.airport(s.airport).code}: potřeba {s.used}, máte {s.owned}
                </li>
              ))}
            </ul>
          )}

          {info && focusLeg && (
            <div className="route-info">
              <h3>
                {world.airport(info.from).code} → {world.airport(info.to).code}
              </h3>
              <dl>
                <dt>Vzdálenost</dt>
                <dd>{num(info.distanceKm)} km</dd>
                <dt>Poptávka tento měsíc</dt>
                <dd>
                  ~{num(info.demand)} / {num(info.demandBack)} cestujících denně (tam / zpět)
                </dd>
                <dt>Referenční cena</dt>
                <dd>{kc(info.reference)}</dd>
                <dt>Vaše cena</dt>
                <dd>{info.myPrice !== null ? kc(info.myPrice) : "referenční (nastavíte v Linkách)"}</dd>
                <dt>Konkurence</dt>
                <dd>
                  {info.competitors.length === 0
                    ? "nikdo"
                    : info.competitors
                        .map((c) => `${game.airlines.find((a) => a.id === c.airline)?.name ?? "?"} ${kc(c.price)}`)
                        .join(", ")}
                </dd>
              </dl>
              <p className="muted small">
                Poptávku sdílí všichni, kdo linku létají; podíl závisí na ceně, službách a času odletu.
              </p>
            </div>
          )}
        </aside>
      </div>

      <footer className="row spread">
        <ConfirmButton
          confirm="Opravdu vymazat celý plán?"
          disabled={legs.length === 0}
          onConfirm={() => setLegs([])}
        >
          Vymazat plán
        </ConfirmButton>
        <div className="row">
          <button onClick={onClose}>Zrušit</button>
          <button className="primary" disabled={!valid} onClick={save}>
            Uložit plán
          </button>
        </div>
      </footer>
    </section>
  );
}
