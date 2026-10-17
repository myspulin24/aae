import { describe, expect, it } from "vitest";

import { blockMinutes, planeAt } from "./flight";
import { ceil5, hhmm, kc, parseHhmm } from "../format";

const at = (id: number) => ({ x: id * 100, y: 0 });
const legs = [
  { from: 1, to: 2, dep: 420, arr: 500, distanceKm: 300 },
  { from: 2, to: 1, dep: 540, arr: 620, distanceKm: 300 },
  { from: 1, to: 3, dep: 1380, arr: 1500, distanceKm: 900 },
];

describe("planeAt", () => {
  it("stojí na letišti prvního letu před prvním odletem", () => {
    const p = planeAt(legs, 9, 300, at);
    expect(p.flying).toBe(false);
    expect(p.airport).toBe(1);
  });

  it("letí mezi letišti podle uplynulého času", () => {
    const p = planeAt(legs, 9, 460, at);
    expect(p.flying).toBe(true);
    expect(p.x).toBeCloseTo(150);
  });

  it("po přistání stojí v cíli", () => {
    expect(planeAt(legs, 9, 510, at).airport).toBe(2);
  });

  it("noční let po půlnoci pokračuje", () => {
    const p = planeAt(legs, 9, 30, at);
    expect(p.flying).toBe(true);
    expect(p.leg?.to).toBe(3);
  });

  it("bez plánu stojí na základně", () => {
    expect(planeAt([], 4, 600, at).airport).toBe(4);
  });

  it("sčítá blokový čas", () => {
    expect(blockMinutes(legs)).toBe(80 + 80 + 120);
  });
});

describe("formát", () => {
  it("peníze s mezerami", () => {
    expect(kc(18412000)).toBe("18 412 000 Kč");
    expect(kc(-1500)).toBe("−1 500 Kč");
  });

  it("čas tam a zpět", () => {
    expect(hhmm(425)).toBe("07:05");
    expect(hhmm(1500)).toBe("01:00");
    expect(parseHhmm("07:05")).toBe(425);
    expect(parseHhmm("24:00")).toBeNull();
    expect(ceil5(421)).toBe(425);
  });
});
