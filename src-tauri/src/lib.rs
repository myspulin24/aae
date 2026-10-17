//! AAE – desktopová aplikace: příkazy pro okno a spuštění Tauri.
//!
//! Okno smí volat jen příkazy vyjmenované v `build.rs` (oprávnění
//! `app-commands`) a poslouchat události `aae://…` z [`session`].

mod dto;
mod profile;
mod saves;
mod session;
mod texts;

use std::path::PathBuf;

use aae_net::host::HostCommand;
use aae_net::protocol::ClientMsg;
use tauri::{AppHandle, Manager, State};

use dto::{
    AppInfo, CampaignInfoDto, LegInput, Phase, PlanPreviewDto, RouteInfoDto, RulesInput, StatusDto,
    StrategyDto, UiIntent, UpdateDto,
};
use profile::Profile;
use session::Runtime;

type Rt<'a> = State<'a, Runtime>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn app_info(rt: Rt<'_>) -> AppInfo {
    rt.info()
}

#[tauri::command]
fn session_status(rt: Rt<'_>) -> StatusDto {
    rt.status()
}

#[tauri::command]
fn profile_set_name(rt: Rt<'_>, name: String) -> Result<(), String> {
    rt.set_name(&name)
}

#[tauri::command]
fn campaigns_list(rt: Rt<'_>) -> Vec<CampaignInfoDto> {
    rt.campaigns()
}

#[tauri::command]
fn open_campaigns_dir(rt: Rt<'_>) -> Result<(), String> {
    let root = rt.campaigns_root();
    std::fs::create_dir_all(&root).map_err(err)?;
    #[cfg(windows)]
    std::process::Command::new("explorer")
        .arg(&root)
        .spawn()
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn host_start(rt: Rt<'_>, load: Option<String>) -> Result<(), String> {
    rt.host_start(load.map(PathBuf::from)).await
}

#[tauri::command]
fn host_set_rules(rt: Rt<'_>, rules: RulesInput) -> Result<(), String> {
    rt.host_set_rules(rules)
}

#[tauri::command]
fn host_set_ai(rt: Rt<'_>, ai: Vec<StrategyDto>) -> Result<(), String> {
    rt.host_set_ai(ai)
}

#[tauri::command]
fn host_set_campaign_name(rt: Rt<'_>, name: String) -> Result<(), String> {
    rt.host_set_campaign_name(name)
}

#[tauri::command]
fn host_start_campaign(rt: Rt<'_>) -> Result<(), String> {
    rt.host_command(HostCommand::Start)
}

#[tauri::command]
fn host_assign(rt: Rt<'_>, peer: String, airline: u8) -> Result<(), String> {
    let peer = rt.host_peer(&peer)?;
    rt.host_command(HostCommand::Assign {
        peer,
        airline: aae_sim::AirlineId(airline),
    })
}

#[tauri::command]
fn host_refuse(rt: Rt<'_>, peer: String) -> Result<(), String> {
    let peer = rt.host_peer(&peer)?;
    rt.host_command(HostCommand::Refuse { peer })
}

#[tauri::command]
fn host_pause(rt: Rt<'_>, on: bool) -> Result<(), String> {
    rt.host_command(HostCommand::TechnicalPause(on))
}

#[tauri::command]
fn host_resume_auto(rt: Rt<'_>) -> Result<(), String> {
    rt.host_command(HostCommand::ResumeAuto)
}

#[tauri::command]
async fn host_save(rt: Rt<'_>) -> Result<(), String> {
    rt.save_manual().await
}

#[tauri::command]
async fn join(rt: Rt<'_>, invitation: String) -> Result<(), String> {
    rt.join(invitation).await
}

#[tauri::command]
fn lobby_set_profile(rt: Rt<'_>, name: String, home: u16) -> Result<(), String> {
    rt.lobby_set_profile(name, home)
}

#[tauri::command]
fn send_intent(rt: Rt<'_>, intent: UiIntent) -> Result<u32, String> {
    rt.send_intent(intent)
}

#[tauri::command]
fn propose_speed(rt: Rt<'_>, speed: u8) -> Result<(), String> {
    rt.propose_speed(speed)
}

#[tauri::command]
fn vote(rt: Rt<'_>, yes: bool) -> Result<(), String> {
    rt.client_send(ClientMsg::Vote { yes })
}

#[tauri::command]
fn presence(rt: Rt<'_>, contract: Option<u32>) -> Result<(), String> {
    rt.presence(contract)
}

#[tauri::command]
fn route_info(rt: Rt<'_>, from: u16, to: u16) -> Result<RouteInfoDto, String> {
    rt.route_info(from, to)
}

#[tauri::command]
fn plan_preview(
    rt: Rt<'_>,
    aircraft: Option<u32>,
    model: u8,
    legs: Vec<LegInput>,
) -> Result<PlanPreviewDto, String> {
    rt.plan_preview(aircraft, model, legs)
}

#[tauri::command]
fn plan_shuttle(
    rt: Rt<'_>,
    model: u8,
    base: u16,
    dest: u16,
    start: u32,
    until: u32,
) -> Result<Vec<LegInput>, String> {
    rt.plan_shuttle(model, base, dest, start, until)
}

#[tauri::command]
async fn leave(rt: Rt<'_>) -> Result<(), String> {
    rt.leave().await;
    Ok(())
}

/// Aktualizace jen v menu (R: updater při startu a v lobby, nikdy během hry).
#[tauri::command]
async fn update_check(app: AppHandle) -> Result<Option<UpdateDto>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app.updater().map_err(err)?.check().await.map_err(err)?;
    Ok(update.map(|u| UpdateDto {
        version: u.version.clone(),
        notes: u.body.clone(),
    }))
}

#[tauri::command]
async fn update_install(app: AppHandle, rt: Rt<'_>) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    if rt.status().phase != Phase::Menu {
        return Err("Aktualizovat jde jen v hlavním menu.".into());
    }
    let update = app
        .updater()
        .map_err(err)?
        .check()
        .await
        .map_err(err)?
        .ok_or("Novější verze není k dispozici.")?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(err)?;
    app.restart();
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            // Proměnná AAE_KAMPANE přesměruje uložené kampaně jinam (kouřový test).
            let campaigns = match std::env::var_os("AAE_KAMPANE") {
                Some(dir) => PathBuf::from(dir),
                None => app
                    .path()
                    .document_dir()
                    .map(|d| d.join("AAE").join("Kampaně"))
                    .unwrap_or_else(|_| data.join("Kampaně")),
            };
            let rt = Runtime::new(app.handle().clone(), Profile::path(&data), campaigns)?;
            app.manage(rt);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Hostitel před zavřením okna kampaň uloží a rozloučí se s hosty.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle().clone();
                let rt = app.state::<Runtime>().inner().clone();
                if rt.status().phase != Phase::Menu {
                    api.prevent_close();
                    tauri::async_runtime::spawn(async move {
                        rt.leave().await;
                        app.exit(0);
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            session_status,
            profile_set_name,
            campaigns_list,
            open_campaigns_dir,
            host_start,
            host_set_rules,
            host_set_ai,
            host_set_campaign_name,
            host_start_campaign,
            host_assign,
            host_refuse,
            host_pause,
            host_resume_auto,
            host_save,
            join,
            lobby_set_profile,
            send_intent,
            propose_speed,
            vote,
            presence,
            route_info,
            plan_preview,
            plan_shuttle,
            leave,
            update_check,
            update_install,
        ])
        .run(tauri::generate_context!())
        .expect("AAE se nepodařilo spustit");
}
