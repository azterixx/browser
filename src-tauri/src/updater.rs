// Updates come from GitHub releases: the workflow publishes a signed latest.json
// next to the installers, and tauri.conf.json points the updater at it.
//
// The app updates itself the way a browser should: it checks at startup, downloads
// quietly, and swaps the files in when you quit. Nothing is ever asked of the user,
// and the next launch is simply the new version.
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const FIRST_CHECK_AFTER: Duration = Duration::from_secs(10);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

// The downloaded installer waits here until the app quits.
#[derive(Default)]
pub struct Ready(Mutex<Option<(Update, Vec<u8>)>>);

#[derive(Serialize, Clone)]
pub struct Found {
    version: String,
    current: String,
    notes: Option<String>,
    date: Option<String>,
    staged: bool,
}

impl Found {
    fn of(u: &Update, staged: bool) -> Self {
        Found {
            version: u.version.clone(),
            current: u.current_version.clone(),
            notes: u.body.clone(),
            date: u.date.map(|d| d.to_string()),
            staged,
        }
    }
}

async fn look(app: &AppHandle) -> Result<Option<Update>, String> {
    app.updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<Found>, String> {
    if let Some((update, _)) = app.state::<Ready>().0.lock().unwrap().as_ref() {
        return Ok(Some(Found::of(update, true)));
    }
    Ok(look(&app).await?.map(|u| Found::of(&u, false)))
}

// The "update now" button: don't wait for a quit, swap and restart right away.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    if let Some((update, bytes)) = app.state::<Ready>().0.lock().unwrap().take() {
        log::info!("installing update {} now", update.version);
        update.install(bytes).map_err(|e| {
            log::error!("update install failed: {e}");
            e.to_string()
        })?;
        app.restart();
    }
    let update = look(&app).await?.ok_or("Already up to date")?;
    log::info!("downloading and installing update {}", update.version);
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| {
            log::error!("update install failed: {e}");
            e.to_string()
        })?;
    app.restart();
}

// Runs at startup and then a few times a day: find, download, park.
pub fn watch(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK_AFTER);
        loop {
            if app.state::<Ready>().0.lock().unwrap().is_none() {
                stage(&app);
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

fn stage(app: &AppHandle) {
    let update = match tauri::async_runtime::block_on(look(app)) {
        Ok(Some(u)) => u,
        Ok(None) => return log::info!("no update, this is the latest version"),
        Err(e) => return log::warn!("update check failed: {e}"),
    };
    log::info!("downloading update {}", update.version);
    let bytes = match tauri::async_runtime::block_on(update.download(|_, _| {}, || {})) {
        Ok(b) => b,
        Err(e) => return log::warn!("update download failed: {e}"),
    };
    log::info!("update {} is ready and installs when you quit", update.version);
    if let (Some(ui), Ok(json)) = (app.get_webview("ui"), serde_json::to_string(&Found::of(&update, true))) {
        let _ = ui.eval(format!("window.updateReady && window.updateReady({json})"));
    }
    *app.state::<Ready>().0.lock().unwrap() = Some((update, bytes));
}

// Called as the app exits. On Windows the installer takes over from here; on macOS
// and Linux the bundle is replaced in place, so the next launch is the new version.
pub fn install_on_quit(app: &AppHandle) {
    let Some((update, bytes)) = app.state::<Ready>().0.lock().unwrap().take() else { return };
    log::info!("installing update {} on quit", update.version);
    if let Err(e) = update.install(bytes) {
        log::error!("update install on quit failed: {e}");
    }
}
