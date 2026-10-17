// Typované volání příkazů jádra (Rust) a odběr událostí `aae://…`.
// Typy jsou vygenerované z Rustu (ts-rs, src/api/bindings).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { AppInfo } from "./bindings/AppInfo";
import type { CampaignInfoDto } from "./bindings/CampaignInfoDto";
import type { GameDto } from "./bindings/GameDto";
import type { LegInput } from "./bindings/LegInput";
import type { LobbyDto } from "./bindings/LobbyDto";
import type { NewsDto } from "./bindings/NewsDto";
import type { PlanPreviewDto } from "./bindings/PlanPreviewDto";
import type { ReplyDto } from "./bindings/ReplyDto";
import type { RouteInfoDto } from "./bindings/RouteInfoDto";
import type { RulesInput } from "./bindings/RulesInput";
import type { StatusDto } from "./bindings/StatusDto";
import type { StrategyDto } from "./bindings/StrategyDto";
import type { UiIntent } from "./bindings/UiIntent";
import type { UpdateDto } from "./bindings/UpdateDto";
import type { WaitingPeerDto } from "./bindings/WaitingPeerDto";

/** Text chyby z jádra (příkazy vrací české zprávy jako řetězec). */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  status: () => invoke<StatusDto>("session_status"),
  setName: (name: string) => invoke<void>("profile_set_name", { name }),
  campaigns: () => invoke<CampaignInfoDto[]>("campaigns_list"),
  openCampaignsDir: () => invoke<void>("open_campaigns_dir"),

  hostStart: (load: string | null = null) => invoke<void>("host_start", { load }),
  hostSetRules: (rules: RulesInput) => invoke<void>("host_set_rules", { rules }),
  hostSetAi: (ai: StrategyDto[]) => invoke<void>("host_set_ai", { ai }),
  hostSetCampaignName: (name: string) => invoke<void>("host_set_campaign_name", { name }),
  hostStartCampaign: () => invoke<void>("host_start_campaign"),
  hostAssign: (peer: string, airline: number) => invoke<void>("host_assign", { peer, airline }),
  hostRefuse: (peer: string) => invoke<void>("host_refuse", { peer }),
  hostPause: (on: boolean) => invoke<void>("host_pause", { on }),
  hostResumeAuto: () => invoke<void>("host_resume_auto"),
  hostSave: () => invoke<void>("host_save"),

  join: (invitation: string) => invoke<void>("join", { invitation }),
  lobbySetProfile: (name: string, home: number) =>
    invoke<void>("lobby_set_profile", { name, home }),
  intent: (intent: UiIntent) => invoke<number>("send_intent", { intent }),
  proposeSpeed: (speed: number) => invoke<void>("propose_speed", { speed }),
  vote: (yes: boolean) => invoke<void>("vote", { yes }),
  presence: (contract: number | null) => invoke<void>("presence", { contract }),

  routeInfo: (from: number, to: number) => invoke<RouteInfoDto>("route_info", { from, to }),
  planPreview: (aircraft: number | null, model: number, legs: LegInput[]) =>
    invoke<PlanPreviewDto>("plan_preview", { aircraft, model, legs }),
  planShuttle: (model: number, base: number, dest: number, start: number, until: number) =>
    invoke<LegInput[]>("plan_shuttle", { model, base, dest, start, until }),

  leave: () => invoke<void>("leave"),
  updateCheck: () => invoke<UpdateDto | null>("update_check"),
  updateInstall: () => invoke<void>("update_install"),
};

export interface Handlers {
  status: (s: StatusDto) => void;
  lobby: (l: LobbyDto) => void;
  game: (g: GameDto) => void;
  news: (n: NewsDto[]) => void;
  reply: (r: ReplyDto) => void;
  waiting: (w: WaitingPeerDto[]) => void;
  error: (text: string) => void;
}

export async function subscribe(h: Handlers): Promise<UnlistenFn> {
  const offs = await Promise.all([
    listen<StatusDto>("aae://status", (e) => h.status(e.payload)),
    listen<LobbyDto>("aae://lobby", (e) => h.lobby(e.payload)),
    listen<GameDto>("aae://game", (e) => h.game(e.payload)),
    listen<NewsDto[]>("aae://news", (e) => h.news(e.payload)),
    listen<ReplyDto>("aae://reply", (e) => h.reply(e.payload)),
    listen<WaitingPeerDto[]>("aae://waiting", (e) => h.waiting(e.payload)),
    listen<string>("aae://error", (e) => h.error(e.payload)),
  ]);
  return () => offs.forEach((off) => off());
}
