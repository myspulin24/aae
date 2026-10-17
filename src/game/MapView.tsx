// Mapa Evropy: státy, letiště, linky všech aerolinek a vlastní letadla.
// Kolečko myši přibližuje, tažení posouvá, dvojklik vrátí celou mapu.
import { useMemo, useRef, useState, type PointerEvent, type WheelEvent } from "react";

import type { GameDto } from "../api/bindings/GameDto";
import type { LegDto } from "../api/bindings/LegDto";
import { airlineColor } from "../format";
import map from "../map/europe.json";
import { MAP_HEIGHT, MAP_WIDTH } from "../map/project";
import type { World } from "../world";
import { useGameMinute } from "./clock";
import { planeAt } from "./flight";

interface View {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Výchozí výřez: všechna letiště s okrajem. */
function fullView(world: World): View {
  const ps = world.airports.map((a) => world.pos(a.id));
  const margin = 30;
  const x = Math.max(0, Math.min(...ps.map((p) => p.x)) - margin);
  const y = Math.max(0, Math.min(...ps.map((p) => p.y)) - margin);
  const w = Math.min(MAP_WIDTH, Math.max(...ps.map((p) => p.x)) + margin) - x;
  const h = Math.min(MAP_HEIGHT, Math.max(...ps.map((p) => p.y)) + margin) - y;
  return { x, y, w, h };
}

function Countries() {
  // Obrysy se nemění, vykreslí se jednou.
  return useMemo(
    () => (
      <g className="countries">
        {map.countries.map((c) => (
          <path key={c.iso + c.name} d={c.d} />
        ))}
      </g>
    ),
    [],
  );
}

export function MapView({
  game,
  world,
  selected,
  onAirport,
  preview,
  picking,
}: {
  game: GameDto;
  world: World;
  selected: number | null;
  onAirport: (id: number) => void;
  preview: LegDto[] | null;
  picking: boolean;
}) {
  const full = useMemo(() => fullView(world), [world]);
  const [view, setView] = useState<View>(full);
  const svgRef = useRef<SVGSVGElement>(null);
  const drag = useRef<{ cx: number; cy: number; view: View; moved: boolean } | null>(null);
  const suppressClick = useRef(false);
  const k = view.w / MAP_WIDTH;

  function toMap(clientX: number, clientY: number) {
    const svg = svgRef.current;
    const ctm = svg?.getScreenCTM();
    if (!svg || !ctm) return { x: 0, y: 0 };
    const p = svg.createSVGPoint();
    p.x = clientX;
    p.y = clientY;
    const r = p.matrixTransform(ctm.inverse());
    return { x: r.x, y: r.y };
  }

  function onWheel(e: WheelEvent) {
    const p = toMap(e.clientX, e.clientY);
    const factor = e.deltaY > 0 ? 1.18 : 1 / 1.18;
    const w = Math.min(MAP_WIDTH * 1.1, Math.max(120, view.w * factor));
    const s = w / view.w;
    setView({ x: p.x - (p.x - view.x) * s, y: p.y - (p.y - view.y) * s, w, h: view.h * s });
  }

  function onPointerDown(e: PointerEvent) {
    drag.current = { cx: e.clientX, cy: e.clientY, view, moved: false };
  }

  function onPointerMove(e: PointerEvent) {
    const d = drag.current;
    const svg = svgRef.current;
    if (!d || !svg || e.buttons === 0) return;
    const dx = e.clientX - d.cx;
    const dy = e.clientY - d.cy;
    if (!d.moved && Math.hypot(dx, dy) < 4) return;
    if (!d.moved) svg.setPointerCapture(e.pointerId);
    d.moved = true;
    const rect = svg.getBoundingClientRect();
    const scale = Math.min(rect.width / d.view.w, rect.height / d.view.h);
    setView({ ...d.view, x: d.view.x - dx / scale, y: d.view.y - dy / scale });
  }

  function onPointerUp() {
    suppressClick.current = drag.current?.moved ?? false;
    drag.current = null;
  }

  const me = game.me;
  const routes = useMemo(() => {
    const out: { key: string; airline: number; a: number; b: number; mine: boolean }[] = [];
    for (const al of game.airlines) {
      const seen = new Set<string>();
      for (const r of al.network) {
        const pair = r.from < r.to ? `${r.from}-${r.to}` : `${r.to}-${r.from}`;
        if (seen.has(pair)) continue;
        seen.add(pair);
        out.push({ key: `${al.id}:${pair}`, airline: al.id, a: r.from, b: r.to, mine: al.id === me });
      }
    }
    // Vlastní linky navrch.
    return out.sort((x, y) => Number(x.mine) - Number(y.mine));
  }, [game.airlines, me]);

  const myAirports = useMemo(() => {
    const s = new Set<number>([game.my.home]);
    for (const r of game.airlines.find((a) => a.me)?.network ?? []) {
      s.add(r.from);
      s.add(r.to);
    }
    return s;
  }, [game.airlines, game.my.home]);

  const homes = useMemo(() => new Map(game.airlines.map((a) => [a.home, a.id])), [game.airlines]);
  const weather = new Set(
    game.disruptions.filter((d) => d.day === game.clock.day && d.airport !== null).map((d) => d.airport),
  );

  return (
    <div className={`map-wrap ${picking ? "picking" : ""}`}>
      <svg
        ref={svgRef}
        className="map"
        viewBox={`${view.x} ${view.y} ${view.w} ${view.h}`}
        preserveAspectRatio="xMidYMid meet"
        onWheel={onWheel}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerLeave={onPointerUp}
        onDoubleClick={() => setView(full)}
      >
        <rect className="sea" x={-2000} y={-2000} width={5000} height={5000} />
        <Countries />

        <g className="routes">
          {routes.map((r) => {
            const a = world.pos(r.a);
            const b = world.pos(r.b);
            return (
              <line
                key={r.key}
                x1={a.x}
                y1={a.y}
                x2={b.x}
                y2={b.y}
                stroke={airlineColor(r.airline)}
                strokeWidth={(r.mine ? 2.2 : 1.1) * k}
                strokeOpacity={r.mine ? 0.9 : 0.35}
              />
            );
          })}
        </g>

        {preview && (
          <g className="routes preview">
            {preview.map((l, i) => {
              const a = world.pos(l.from);
              const b = world.pos(l.to);
              return (
                <line
                  key={i}
                  x1={a.x}
                  y1={a.y}
                  x2={b.x}
                  y2={b.y}
                  strokeWidth={2.4 * k}
                  strokeDasharray={`${6 * k} ${4 * k}`}
                />
              );
            })}
          </g>
        )}

        <g className="airports">
          {world.airports.map((a) => {
            const p = world.pos(a.id);
            const r = (2.2 + Math.sqrt(a.populationK) / 28) * k;
            const home = homes.get(a.id);
            const showLabel = k < 0.55 || a.populationK >= 1800 || myAirports.has(a.id) || selected === a.id;
            return (
              <g
                key={a.id}
                className={`airport ${selected === a.id ? "selected" : ""} ${myAirports.has(a.id) ? "mine" : ""}`}
                transform={`translate(${p.x} ${p.y})`}
                onClick={() => {
                  if (suppressClick.current) {
                    suppressClick.current = false;
                    return;
                  }
                  onAirport(a.id);
                }}
              >
                <circle className="hit" r={Math.max(r * 2.2, 7 * k)} />
                {home !== undefined && (
                  <circle className="home" r={r + 2.6 * k} stroke={airlineColor(home)} strokeWidth={1.6 * k} />
                )}
                {a.slotConstrained && <circle className="slot" r={r + 1.2 * k} strokeWidth={0.9 * k} />}
                {weather.has(a.id) && <circle className="weather" r={r + 4.5 * k} strokeWidth={1.4 * k} />}
                <circle className="dot" r={r} />
                {showLabel && (
                  <text x={r + 2 * k} y={3 * k} fontSize={9 * k}>
                    {k < 0.4 ? a.city : a.code}
                  </text>
                )}
              </g>
            );
          })}
        </g>

        <Planes game={game} world={world} k={k} />
      </svg>
      <div className="map-hint muted small">
        {picking
          ? "Klikněte na letiště, kam má letadlo letět dál."
          : "Kolečko přibližuje, tažení posouvá, dvojklik vrátí celou mapu."}
      </div>
    </div>
  );
}

function Planes({ game, world, k }: { game: GameDto; world: World; k: number }) {
  const minute = useGameMinute(30);
  const m = minute % 1440;
  const color = airlineColor(game.me);
  return (
    <g className="planes">
      {game.my.fleet.map((p) => {
        const legs = game.my.fleet.length > 0 ? p.today : [];
        const pos = planeAt(legs, p.base, m, world.pos);
        if (!pos.flying) return null;
        return (
          <g key={p.id} transform={`translate(${pos.x} ${pos.y}) rotate(${pos.angle}) scale(${k})`}>
            <path d="M7 0 L-5 -5 L-2.5 0 L-5 5 Z" fill={color} stroke="#0b1d33" strokeWidth={0.8} />
          </g>
        );
      })}
    </g>
  );
}
