// Projekce mapy Evropy (src/map/europe.json, generuje scripts/gen-map.mjs).
// Konstanty jsou jen v JSON, aby letiště i obrysy států používaly stejnou projekci:
//   x = (lon − lonMin) * cosLat * scale
//   y = (latMax − lat) * scale
import map from "./europe.json";

const { lonMin, latMax, cosLat, scale } = map.projection;

export const MAP_WIDTH: number = map.width;
export const MAP_HEIGHT: number = map.height;

export function project(lat: number, lon: number): { x: number; y: number } {
  return {
    x: (lon - lonMin) * cosLat * scale,
    y: (latMax - lat) * scale,
  };
}
