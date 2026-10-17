// Seznam příkazů, které smí volat okno (oprávnění `app-commands`).
// Musí sedět s `generate_handler!` v `lib.rs`.
pub const COMMANDS: &[&str] = &[
    "app_info",
    "session_status",
    "profile_set_name",
    "campaigns_list",
    "host_start",
    "host_set_rules",
    "host_set_ai",
    "host_set_campaign_name",
    "host_start_campaign",
    "host_assign",
    "host_refuse",
    "host_pause",
    "host_resume_auto",
    "host_save",
    "join",
    "lobby_set_profile",
    "send_intent",
    "propose_speed",
    "vote",
    "presence",
    "route_info",
    "plan_preview",
    "plan_shuttle",
    "leave",
    "open_campaigns_dir",
    "update_check",
    "update_install",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri-build selhal");
}
