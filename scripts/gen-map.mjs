#!/usr/bin/env node
// Generátor mapy Evropy pro UI (React + SVG) z Natural Earth 1:50m admin 0 countries
// (public domain, https://www.naturalearthdata.com/about/terms-of-use/).
//
// Použití:
//   node scripts/gen-map.mjs <ne_50m_admin_0_countries.geojson> <src/map/europe.json>
//
// Zdrojový GeoJSON:
//   https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_50m_admin_0_countries.geojson
// Stažená data jsou nedůvěryhodný vstup: skript je jen čte (JSON.parse) a ověřuje strukturu.
//
// Postup: ořez na obdélník (Sutherland–Hodgman v lon/lat) → projekce → Douglas–Peucker
// → zaokrouhlení na 0,1 → vynechání degenerovaných kroužků a ostrovů pod MIN_AREA.
//
// Projekce (musí přesně sedět se src/map/project.ts, ta bere konstanty z výstupního JSON):
//   x = (lon − LON_MIN) * COS_LAT * SCALE
//   y = (LAT_MAX − lat) * SCALE

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";

const LON_MIN = -11;
const LON_MAX = 36;
const LAT_MIN = 33;
const LAT_MAX = 62;
const COS_LAT = 0.6691306063588582; // cos(48°)
const WIDTH = 1000;
const SCALE = WIDTH / ((LON_MAX - LON_MIN) * COS_LAT);
const HEIGHT = Math.ceil((LAT_MAX - LAT_MIN) * SCALE);

// Tolerance Douglas–Peucker v jednotkách po projekci. Zadání: 0,6; zvýšit jen tehdy,
// když by výstup přesáhl SIZE_LIMIT (a uvést to zde). Při 0,6 vychází soubor pod limitem.
const TOLERANCE = 0.6;
const MIN_AREA = 4; // jednotky², menší kroužky (ostrovy) vynechat
const SIZE_LIMIT = 250 * 1024;

const SOURCE = "Natural Earth 1:50m admin 0 countries (public domain)";

if (Math.abs(COS_LAT - Math.cos((48 * Math.PI) / 180)) > 1e-15) {
  throw new Error("COS_LAT nesouhlasí s cos(48°)");
}

const [inputArg, outputArg] = process.argv.slice(2);
if (!inputArg || !outputArg) {
  console.error("Použití: node scripts/gen-map.mjs <vstup.geojson> <výstup.json>");
  process.exit(2);
}

// ---------- geometrie ----------

/** Ořez otevřeného kroužku [[lon, lat], …] na obdélník (Sutherland–Hodgman). */
function clipRing(ring) {
  const edges = [
    { inside: (p) => p[0] >= LON_MIN, cross: (a, b) => atLon(a, b, LON_MIN) },
    { inside: (p) => p[0] <= LON_MAX, cross: (a, b) => atLon(a, b, LON_MAX) },
    { inside: (p) => p[1] >= LAT_MIN, cross: (a, b) => atLat(a, b, LAT_MIN) },
    { inside: (p) => p[1] <= LAT_MAX, cross: (a, b) => atLat(a, b, LAT_MAX) },
  ];
  let out = ring;
  for (const edge of edges) {
    if (out.length === 0) break;
    const input = out;
    out = [];
    let prev = input[input.length - 1];
    let prevIn = edge.inside(prev);
    for (const cur of input) {
      const curIn = edge.inside(cur);
      if (curIn) {
        if (!prevIn) out.push(edge.cross(prev, cur));
        out.push(cur);
      } else if (prevIn) {
        out.push(edge.cross(prev, cur));
      }
      prev = cur;
      prevIn = curIn;
    }
  }
  return out;
}

function atLon(a, b, lon) {
  const t = (lon - a[0]) / (b[0] - a[0]);
  return [lon, a[1] + t * (b[1] - a[1])];
}

function atLat(a, b, lat) {
  const t = (lat - a[1]) / (b[1] - a[1]);
  return [a[0] + t * (b[0] - a[0]), lat];
}

function project([lon, lat]) {
  return [(lon - LON_MIN) * COS_LAT * SCALE, (LAT_MAX - lat) * SCALE];
}

/** Vzdálenost bodu p od úsečky ab. */
function segDist(p, a, b) {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  let t = len2 === 0 ? 0 : ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}

/** Douglas–Peucker na otevřené lomené čáře (iterativně, bez hluboké rekurze). */
function simplifyLine(pts, tol) {
  const n = pts.length;
  if (n <= 2) return pts.slice();
  const keep = new Uint8Array(n);
  keep[0] = keep[n - 1] = 1;
  const stack = [[0, n - 1]];
  while (stack.length) {
    const [i, j] = stack.pop();
    let maxD = -1;
    let idx = -1;
    for (let k = i + 1; k < j; k++) {
      const d = segDist(pts[k], pts[i], pts[j]);
      if (d > maxD) {
        maxD = d;
        idx = k;
      }
    }
    if (idx !== -1 && maxD > tol) {
      keep[idx] = 1;
      stack.push([i, idx], [idx, j]);
    }
  }
  return pts.filter((_, k) => keep[k]);
}

/** Douglas–Peucker na uzavřeném kroužku: rozdělí ho v nejvzdálenějším bodě od prvního. */
function simplifyRing(ring, tol) {
  const n = ring.length;
  if (n <= 3) return ring.slice();
  let far = 0;
  let farD = -1;
  for (let k = 1; k < n; k++) {
    const d = Math.hypot(ring[k][0] - ring[0][0], ring[k][1] - ring[0][1]);
    if (d > farD) {
      farD = d;
      far = k;
    }
  }
  const a = simplifyLine(ring.slice(0, far + 1), tol);
  const b = simplifyLine([...ring.slice(far), ring[0]], tol);
  return [...a, ...b.slice(1, -1)];
}

const round1 = (v) => {
  const r = Math.round(v * 10) / 10;
  return Object.is(r, -0) ? 0 : r;
};

/** Zaokrouhlí a odstraní po sobě jdoucí shodné body (včetně uzavírajícího). */
function roundRing(ring) {
  const out = [];
  for (const p of ring) {
    const q = [round1(p[0]), round1(p[1])];
    const last = out[out.length - 1];
    if (!last || last[0] !== q[0] || last[1] !== q[1]) out.push(q);
  }
  while (out.length > 1) {
    const f = out[0];
    const l = out[out.length - 1];
    if (f[0] === l[0] && f[1] === l[1]) out.pop();
    else break;
  }
  return out;
}

function ringArea(ring) {
  let s = 0;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    s += ring[j][0] * ring[i][1] - ring[i][0] * ring[j][1];
  }
  return Math.abs(s) / 2;
}

function bboxOverlaps(ring) {
  let minLon = Infinity, maxLon = -Infinity, minLat = Infinity, maxLat = -Infinity;
  for (const [lon, lat] of ring) {
    if (lon < minLon) minLon = lon;
    if (lon > maxLon) maxLon = lon;
    if (lat < minLat) minLat = lat;
    if (lat > maxLat) maxLat = lat;
  }
  return maxLon >= LON_MIN && minLon <= LON_MAX && maxLat >= LAT_MIN && minLat <= LAT_MAX;
}

// ---------- vstup ----------

function readRing(raw, where) {
  if (!Array.isArray(raw)) throw new Error(`${where}: kroužek není pole`);
  const ring = raw.map((pt) => {
    if (!Array.isArray(pt) || pt.length < 2) throw new Error(`${where}: neplatný bod`);
    const lon = pt[0];
    const lat = pt[1];
    if (typeof lon !== "number" || typeof lat !== "number" || !Number.isFinite(lon) || !Number.isFinite(lat)) {
      throw new Error(`${where}: souřadnice nejsou čísla`);
    }
    return [lon, lat];
  });
  // GeoJSON kroužek je uzavřený (poslední bod = první); pracujeme s otevřeným.
  if (ring.length > 1) {
    const f = ring[0];
    const l = ring[ring.length - 1];
    if (f[0] === l[0] && f[1] === l[1]) ring.pop();
  }
  return ring;
}

function polygonsOf(geometry, where) {
  if (!geometry || typeof geometry !== "object") return [];
  if (geometry.type === "Polygon") return [geometry.coordinates];
  if (geometry.type === "MultiPolygon") {
    if (!Array.isArray(geometry.coordinates)) throw new Error(`${where}: neplatný MultiPolygon`);
    return geometry.coordinates;
  }
  throw new Error(`${where}: nepodporovaný typ geometrie ${String(geometry.type)}`);
}

function textProp(props, key) {
  const v = props?.[key];
  return typeof v === "string" || typeof v === "number" ? String(v) : "";
}

const inputPath = resolve(inputArg);
const outputPath = resolve(outputArg);
const geo = JSON.parse(readFileSync(inputPath, "utf8"));
if (geo?.type !== "FeatureCollection" || !Array.isArray(geo.features)) {
  throw new Error("Vstup není GeoJSON FeatureCollection");
}

// ---------- zpracování ----------

const countries = [];
let ringCount = 0;
let pointCount = 0;

for (const [fi, feature] of geo.features.entries()) {
  const props = feature?.properties ?? {};
  const name = textProp(props, "NAME");
  const isoA2 = textProp(props, "ISO_A2");
  const iso = isoA2 && isoA2 !== "-99" ? isoA2 : textProp(props, "ADM0_A3");
  const where = `feature ${fi} (${name || "?"})`;
  if (!iso || !name) throw new Error(`${where}: chybí ISO_A2/ADM0_A3 nebo NAME`);

  const rings = [];
  for (const polygon of polygonsOf(feature.geometry, where)) {
    if (!Array.isArray(polygon) || polygon.length === 0) continue;
    const outer = readRing(polygon[0], where);
    if (!bboxOverlaps(outer)) continue; // polygon obdélník vůbec neprotíná
    for (const rawRing of polygon) {
      const clipped = clipRing(readRing(rawRing, where));
      if (clipped.length < 3) continue;
      const ring = roundRing(simplifyRing(clipped.map(project), TOLERANCE));
      if (ring.length < 3) continue;
      if (ringArea(ring) < MIN_AREA) continue;
      rings.push(ring);
    }
  }
  if (rings.length === 0) continue;

  const fmt = (v) => String(v);
  const d = rings
    .map((r) => `M${fmt(r[0][0])} ${fmt(r[0][1])}` + r.slice(1).map((p) => `L${fmt(p[0])} ${fmt(p[1])}`).join("") + "Z")
    .join("");
  countries.push({ iso, name, d });
  ringCount += rings.length;
  pointCount += rings.reduce((s, r) => s + r.length, 0);
}

countries.sort((a, b) => (a.iso < b.iso ? -1 : a.iso > b.iso ? 1 : a.name < b.name ? -1 : 1));

const seen = new Set();
for (const c of countries) {
  if (seen.has(c.iso)) console.warn(`Varování: duplicitní iso ${c.iso} (${c.name})`);
  seen.add(c.iso);
}

// ---------- výstup ----------

const projection = { lonMin: LON_MIN, latMax: LAT_MAX, cosLat: COS_LAT, scale: SCALE };
const text = [
  "{",
  `  "source": ${JSON.stringify(SOURCE)},`,
  `  "width": ${WIDTH},`,
  `  "height": ${HEIGHT},`,
  `  "projection": ${JSON.stringify(projection)},`,
  `  "countries": [`,
  countries.map((c) => `    ${JSON.stringify(c)}`).join(",\n"),
  "  ]",
  "}",
  "",
].join("\n");

const check = JSON.parse(text);
if (check.countries.length !== countries.length || check.projection.scale !== SCALE) {
  throw new Error("Výstupní JSON se nepodařilo ověřit");
}

mkdirSync(dirname(outputPath), { recursive: true });
writeFileSync(outputPath, text, "utf8");

const bytes = Buffer.byteLength(text, "utf8");
console.log(`Zemí: ${countries.length}, kroužků: ${ringCount}, bodů: ${pointCount}`);
console.log(`Mapa: ${WIDTH} × ${HEIGHT}, scale = ${SCALE}`);
console.log(`Soubor: ${outputPath} (${bytes} B, ${(bytes / 1024).toFixed(1)} kB)`);
if (bytes > SIZE_LIMIT) {
  console.warn(`Varování: soubor je větší než ${SIZE_LIMIT / 1024} kB, zvyš TOLERANCE.`);
  process.exitCode = 1;
}
