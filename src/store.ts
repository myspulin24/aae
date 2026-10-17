// Stav okna: co poslalo jádro (relace, lobby, hra, zprávy) a krátké hlášky.
import { useSyncExternalStore } from "react";

import { api, errorText, subscribe } from "./api/client";
import type { AppInfo } from "./api/bindings/AppInfo";
import type { GameDto } from "./api/bindings/GameDto";
import type { LobbyDto } from "./api/bindings/LobbyDto";
import type { NewsDto } from "./api/bindings/NewsDto";
import type { ReplyDto } from "./api/bindings/ReplyDto";
import type { StatusDto } from "./api/bindings/StatusDto";
import type { UiIntent } from "./api/bindings/UiIntent";
import type { WaitingPeerDto } from "./api/bindings/WaitingPeerDto";

export interface Toast {
  id: number;
  tone: "ok" | "error" | "info";
  text: string;
}

export interface AppState {
  info: AppInfo | null;
  status: StatusDto;
  lobby: LobbyDto | null;
  game: GameDto | null;
  /** performance.now() při příchodu posledního stavu hry (plynulé hodiny). */
  gameAt: number;
  news: NewsDto[];
  waiting: WaitingPeerDto[];
  toasts: Toast[];
}

const MENU: StatusDto = {
  phase: "menu",
  host: false,
  invitation: null,
  message: null,
  path: { kind: "unknown", rttMs: 0 },
  hostVersion: null,
  campaignName: null,
  saved: null,
};

let state: AppState = {
  info: null,
  status: MENU,
  lobby: null,
  game: null,
  gameAt: 0,
  news: [],
  waiting: [],
  toasts: [],
};

const listeners = new Set<() => void>();

function set(patch: Partial<AppState>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}

export function getState(): AppState {
  return state;
}

export function useApp<T>(select: (s: AppState) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => select(state),
  );
}

// ------------------------------------------------------------ hlášky

let toastId = 0;

export function toast(text: string, tone: Toast["tone"] = "info") {
  const id = ++toastId;
  set({ toasts: [...state.toasts, { id, tone, text }].slice(-5) });
  window.setTimeout(() => dismiss(id), tone === "error" ? 6000 : 3500);
}

export function dismiss(id: number) {
  set({ toasts: state.toasts.filter((t) => t.id !== id) });
}

// ------------------------------------------------------------ záměry

/** Čekající záměry: id → text pro úspěch. */
const pending = new Map<number, string | null>();
/** Odpovědi, které přišly dřív, než příkaz vrátil id. */
const early = new Map<number, ReplyDto>();

function onReply(r: ReplyDto) {
  if (!pending.has(r.id)) {
    early.set(r.id, r);
    if (early.size > 50) early.delete(early.keys().next().value as number);
    return;
  }
  const okText = pending.get(r.id);
  pending.delete(r.id);
  if (!r.ok) toast(r.text ?? "Hostitel záměr odmítl.", "error");
  else if (okText) toast(okText, "ok");
}

/** Pošle záměr hostiteli; odmítnutí se ukáže jako hláška. */
export async function act(intent: UiIntent, okText: string | null = null): Promise<void> {
  try {
    const id = await api.intent(intent);
    pending.set(id, okText);
    const r = early.get(id);
    if (r) {
      early.delete(id);
      onReply(r);
    }
  } catch (e) {
    toast(errorText(e), "error");
  }
}

/** Zavolá příkaz a chybu ukáže jako hlášku. */
export async function run<T>(p: Promise<T>, okText: string | null = null): Promise<T | undefined> {
  try {
    const v = await p;
    if (okText) toast(okText, "ok");
    return v;
  } catch (e) {
    toast(errorText(e), "error");
    return undefined;
  }
}

// ------------------------------------------------------------ start

export async function start(): Promise<void> {
  await subscribe({
    status: (status) => {
      if (status.phase === "menu" || status.phase === "connecting") {
        set({ status, lobby: null, game: null, news: [], waiting: [] });
      } else {
        set({ status });
      }
    },
    lobby: (lobby) => set({ lobby }),
    game: (game) => set({ game, gameAt: performance.now() }),
    news: (items) => set({ news: [...items.reverse(), ...state.news].slice(0, 200) }),
    reply: onReply,
    waiting: (waiting) => set({ waiting }),
    error: (text) => toast(text, "error"),
  });
  const [info, status] = await Promise.all([api.appInfo(), api.status()]);
  set({ info, status });
}

/** Jen pro testy: nastaví stav napřímo. */
export function setForTest(patch: Partial<AppState>) {
  set(patch);
}
