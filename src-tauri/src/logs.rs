// One log file per run, kept next to the OS's other app logs:
//   macOS    ~/Library/Logs/cc.atlantix.browser
//   Windows  %LOCALAPPDATA%\cc.atlantix.browser\logs
//   Linux    ~/.local/share/cc.atlantix.browser/logs
// A white window that dies leaves its reason here, on the user's own machine.
use tauri::{AppHandle, Manager};
use tauri_plugin_log::{Target, TargetKind, TimezoneStrategy};

const KEEP_FILES: usize = 5;
const MAX_FILE: u128 = 4 * 1024 * 1024;

pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_log::Builder::new()
        .targets([
            Target::new(TargetKind::LogDir { file_name: None }),
            Target::new(TargetKind::Stdout),
        ])
        .max_file_size(MAX_FILE)
        .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(KEEP_FILES))
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .level(log::LevelFilter::Info)
        // The webview engine is noisy about pages it cannot reach; that is not our bug.
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn)
        .build()
}

// panic = "abort" in release means a panic kills the app with no message.
// This writes the message and the place first, so the log explains the crash.
pub fn catch_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown place".into());
        log::error!("panic at {place}: {}", info);
        previous(info);
    }));
}

pub fn folder(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path().app_log_dir().map_err(|e| e.to_string())
}

// Opens the folder in Finder / Explorer / the desktop's file manager.
#[tauri::command]
pub async fn open_logs(app: AppHandle) -> Result<(), String> {
    let dir = folder(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(windows)]
    let opener = "explorer";
    #[cfg(not(any(target_os = "macos", windows)))]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
