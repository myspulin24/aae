// Formátování čísel, peněz a času pro češtinu.

/** 18 412 000 Kč (celé koruny, mezery mezi tisíci). */
export function kc(value: number): string {
  const sign = value < 0 ? "−" : "";
  const digits = Math.round(Math.abs(value)).toString();
  return `${sign}${digits.replace(/\B(?=(\d{3})+(?!\d))/g, " ")} Kč`;
}

/** Zkrácené peníze: 18,4 mil. Kč / 950 tis. Kč. */
export function kcShort(value: number): string {
  const abs = Math.abs(value);
  const sign = value < 0 ? "−" : "";
  if (abs >= 1_000_000_000) return `${sign}${(abs / 1_000_000_000).toFixed(1).replace(".", ",")} mld. Kč`;
  if (abs >= 1_000_000) return `${sign}${(abs / 1_000_000).toFixed(1).replace(".", ",")} mil. Kč`;
  if (abs >= 10_000) return `${sign}${Math.round(abs / 1000)} tis. Kč`;
  return kc(value);
}

export function num(value: number): string {
  return Math.round(value)
    .toString()
    .replace(/\B(?=(\d{3})+(?!\d))/g, " ");
}

export function percent(part: number, whole: number): string {
  if (whole <= 0) return "–";
  return `${Math.round((part / whole) * 100)} %`;
}

/** Minuta dne → „07:05“ (přes půlnoc pokračuje dalším dnem). */
export function hhmm(minute: number): string {
  const m = ((Math.round(minute) % 1440) + 1440) % 1440;
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
}

/** „07:05“ → minuta dne, nebo null. */
export function parseHhmm(text: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return h * 60 + min;
}

/** Zaokrouhlí minutu nahoru na celých 5 minut. */
export function ceil5(minute: number): number {
  return Math.ceil(minute / 5) * 5;
}

/** Délka v minutách → „1 h 25 min“. */
export function duration(minutes: number): string {
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes % 60);
  if (h === 0) return `${m} min`;
  if (m === 0) return `${h} h`;
  return `${h} h ${m} min`;
}

/** Čas od teď v ms → „před 5 min“. */
export function ago(unixMs: number, now = Date.now()): string {
  const s = Math.max(0, (now - unixMs) / 1000);
  if (s < 60) return "právě teď";
  if (s < 3600) return `před ${Math.floor(s / 60)} min`;
  if (s < 86400) return `před ${Math.floor(s / 3600)} h`;
  const d = Math.floor(s / 86400);
  return d === 1 ? "včera" : `před ${d} dny`;
}

export const STRATEGY_NAMES: Record<string, string> = {
  levna: "Levná",
  premiova: "Prémiová",
  hub: "Hub",
  charter: "Charter",
};

export const STRATEGY_HINTS: Record<string, string> = {
  levna: "nízké ceny, hodně letů",
  premiova: "vyšší ceny a služby, větší letadla",
  hub: "síť z domovského letiště, sloty",
  charter: "zakázky z burzy",
};

/** Barvy aerolinek podle id. */
export const AIRLINE_COLORS = [
  "#4fc3f7",
  "#ffb74d",
  "#81c784",
  "#e57373",
  "#ba68c8",
  "#fff176",
  "#4db6ac",
  "#f06292",
];

export function airlineColor(id: number): string {
  return AIRLINE_COLORS[id % AIRLINE_COLORS.length];
}
