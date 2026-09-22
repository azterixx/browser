// Updates come from GitHub releases: the workflow publishes a signed latest.json
// next to the installers, and tauri.conf.json points the updater at it.
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK_AFTER: Duration = Duration::from_secs(10);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Serialize, Clone)]
pub struct Found {
    version: String,
    current: String,
    notes: Option<String>,
    date: Option<String>,
}

async fn look(app: &AppHandle) -> Result<Option<Found>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    Ok(update.map(|u| Found {
        version: u.version.clone(),
        current: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    }))
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<Found>, String> {
    look(&app).await
}

// Downloads, replaces the app and restarts it. The call never returns on success.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Already up to date")?;
    log::info!("installing update {}", update.version);
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| {
            log::error!("update install failed: {e}");
            e.to_string()
        })?;
    app.restart();
}

// A quiet check in the background; the toolbar shows the toast when one is found.
pub fn watch(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_AFTER);
        loop {
            match tauri::async_runtime::block_on(look(&app)) {
                Ok(Some(found)) => {
                    log::info!("update {} is available", found.version);
                    if let (Some(ui), Ok(json)) = (app.get_webview("ui"), serde_json::to_string(&found)) {
                        let _ = ui.eval(format!("window.updateReady && window.updateReady({json})"));
                    }
                }
                Ok(None) => log::info!("no update, this is the latest version"),
                Err(e) => log::warn!("update check failed: {e}"),
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}
