// Poloha letadla na mapě podle dnešního plánu a herní minuty.
import type { LegDto } from "../api/bindings/LegDto";

export interface Point {
  x: number;
  y: number;
}

export interface PlanePos extends Point {
  /** Úhel natočení ve stupních (0 = na východ). */
  angle: number;
  flying: boolean;
  leg: LegDto | null;
  /** Kde letadlo stojí (když neletí). */
  airport: number | null;
}

/**
 * Kde je letadlo v minutě dne `minute` (může být desetinná).
 * Let přes půlnoc (`arr` > 1440) se po půlnoci dokončí ze včerejška.
 */
export function planeAt(
  legs: LegDto[],
  base: number,
  minute: number,
  at: (airport: number) => Point,
): PlanePos {
  for (const leg of legs) {
    for (const m of [minute, minute + 1440]) {
      if (leg.dep <= m && m < leg.arr) {
        const a = at(leg.from);
        const b = at(leg.to);
        const t = (m - leg.dep) / Math.max(1, leg.arr - leg.dep);
        return {
          x: a.x + (b.x - a.x) * t,
          y: a.y + (b.y - a.y) * t,
          angle: (Math.atan2(b.y - a.y, b.x - a.x) * 180) / Math.PI,
          flying: true,
          leg,
          airport: null,
        };
      }
    }
  }
  let airport = base;
  if (legs.length > 0) {
    const landed = legs.filter((l) => l.arr <= minute);
    airport = landed.length > 0 ? landed[landed.length - 1].to : legs[0].from;
  }
  const p = at(airport);
  return { ...p, angle: 0, flying: false, leg: null, airport };
}

/** Kolik minut denně letadlo letí (blokový čas). */
export function blockMinutes(legs: LegDto[]): number {
  return legs.reduce((sum, l) => sum + (l.arr - l.dep), 0);
}
