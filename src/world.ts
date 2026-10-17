// Herní svět pro okno: letiště s polohou na mapě a typy letadel.
import { useMemo } from "react";

import type { AircraftTypeDto } from "./api/bindings/AircraftTypeDto";
import type { AirportDto } from "./api/bindings/AirportDto";
import type { WorldDto } from "./api/bindings/WorldDto";
import type { Point } from "./game/flight";
import { project } from "./map/project";
import { useApp } from "./store";

export interface World {
  airports: AirportDto[];
  types: AircraftTypeDto[];
  airport: (id: number) => AirportDto;
  type: (id: number) => AircraftTypeDto;
  pos: (id: number) => Point;
  /** Vzdálenost po kouli v km (pro řazení; přesnou počítá jádro). */
  km: (a: number, b: number) => number;
  /** Letiště seřazená podle města. */
  byCity: AirportDto[];
}

export function makeWorld(w: WorldDto): World {
  const positions = w.airports.map((a) => project(a.lat, a.lon));
  const rad = Math.PI / 180;
  return {
    airports: w.airports,
    types: w.aircraftTypes,
    airport: (id) => w.airports[id],
    type: (id) => w.aircraftTypes[id],
    pos: (id) => positions[id] ?? { x: 0, y: 0 },
    km: (ia, ib) => {
      const a = w.airports[ia];
      const b = w.airports[ib];
      const dLat = (b.lat - a.lat) * rad;
      const dLon = (b.lon - a.lon) * rad;
      const h =
        Math.sin(dLat / 2) ** 2 + Math.cos(a.lat * rad) * Math.cos(b.lat * rad) * Math.sin(dLon / 2) ** 2;
      return 2 * 6371 * Math.asin(Math.sqrt(h));
    },
    byCity: [...w.airports].sort((a, b) => a.city.localeCompare(b.city, "cs")),
  };
}

export function useWorld(): World | null {
  const info = useApp((s) => s.info);
  return useMemo(() => (info ? makeWorld(info.world) : null), [info]);
}

export function airportLabel(a: AirportDto): string {
  return `${a.city} (${a.code})`;
}
