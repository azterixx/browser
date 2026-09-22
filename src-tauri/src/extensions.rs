// Chrome Web Store extensions: download, unpack, and run them in the tab webviews.
// macOS: WKWebExtensionController (Safari's engine, so Chrome-only APIs won't work).
// Windows: WebView2 loads the same unpacked folders by itself.

use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Clone)]
pub struct Info {
    pub id: String,
    pub name: String,
    pub version: String,
}

pub fn dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from(".")).join("extensions")
}

/// Accepts a Web Store link or a bare ID. IDs are 32 letters a–p.
pub fn parse_id(input: &str) -> Option<String> {
    input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find(|p| p.len() == 32 && p.bytes().all(|b| (b'a'..=b'p').contains(&b)))
        .map(str::to_string)
}

pub fn list(app: &AppHandle) -> Vec<Info> {
    let Ok(entries) = fs::read_dir(dir(app)) else { return Vec::new() };
    let mut out: Vec<Info> = entries
        .flatten()
        .filter_map(|e| read_info(&e.path()))
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub fn remove(app: &AppHandle, id: &str) -> Result<(), String> {
    if parse_id(id).as_deref() != Some(id) {
        return Err("bad id".into());
    }
    #[cfg(target_os = "macos")]
    mac::unload(app, id);
    fs::remove_dir_all(dir(app).join(id)).map_err(|e| e.to_string())
}

/// Blocking: downloads the CRX from the Web Store and unpacks it into `dir/<id>`.
pub fn install(app: &AppHandle, input: &str) -> Result<Info, String> {
    let id = parse_id(input).ok_or("Not a Chrome Web Store link or extension ID")?;
    let url = format!(
        "https://clients2.google.com/service/update2/crx?response=redirect&prodversion=140.0\
         &acceptformat=crx2,crx3&x=id%3D{id}%26installsource%3Dondemand%26uc"
    );
    let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new().tls_connector(Arc::new(tls)).build();
    let mut crx = Vec::new();
    agent
        .get(&url)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?
        .into_reader()
        .take(100 * 1024 * 1024)
        .read_to_end(&mut crx)
        .map_err(|e| e.to_string())?;

    let zip = crx_payload(&crx).ok_or("The store didn't return an extension (is it available?)")?;
    let target = dir(app).join(&id);
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| e.to_string())?;
    // extract() refuses paths that escape the target folder.
    archive.extract(&target).map_err(|e| e.to_string())?;

    let info = read_info(&target).ok_or("The package has no valid manifest.json")?;
    #[cfg(target_os = "macos")]
    mac::load_on_main(app, &target);
    Ok(info)
}

// CRX2/CRX3 = a small signed header in front of a regular ZIP.
fn crx_payload(data: &[u8]) -> Option<&[u8]> {
    let u32_at = |i: usize| Some(u32::from_le_bytes(data.get(i..i + 4)?.try_into().ok()?) as usize);
    if data.get(..4)? != b"Cr24" {
        return data.starts_with(b"PK\x03\x04").then_some(data);
    }
    let start = match u32_at(4)? {
        3 => 12 + u32_at(8)?,
        2 => 16 + u32_at(8)? + u32_at(12)?,
        _ => return None,
    };
    data.get(start..)
}

fn read_info(path: &Path) -> Option<Info> {
    let id = path.file_name()?.to_str()?.to_string();
    parse_id(&id).filter(|p| *p == id)?;
    let manifest: serde_json::Value = serde_json::from_slice(&fs::read(path.join("manifest.json")).ok()?).ok()?;
    let raw = manifest["name"].as_str().unwrap_or(&id).to_string();
    let name = localize(path, &manifest, &raw).unwrap_or(raw);
    let version = manifest["version"].as_str().unwrap_or("").to_string();
    Some(Info { id, name, version })
}

// "__MSG_appName__" -> _locales/<default_locale>/messages.json["appName"].message
fn localize(path: &Path, manifest: &serde_json::Value, value: &str) -> Option<String> {
    let key = value.strip_prefix("__MSG_")?.strip_suffix("__")?.to_lowercase();
    let locale = manifest["default_locale"].as_str().unwrap_or("en");
    let file = fs::read(path.join("_locales").join(locale).join("messages.json")).ok()?;
    let messages: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(&file).ok()?;
    messages
        .iter()
        .find(|(k, _)| k.to_lowercase() == key)
        .and_then(|(_, v)| v["message"].as_str())
        .map(str::to_string)
}

#[cfg(target_os = "macos")]
pub mod mac {
    use std::{cell::RefCell, collections::HashMap, path::Path};

    use block2::RcBlock;
    use objc2::{rc::Retained, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSError, NSString, NSURL};
    use objc2_web_kit::{
        WKWebExtension, WKWebExtensionContext, WKWebExtensionContextPermissionStatus, WKWebExtensionController,
        WKWebExtensionControllerConfiguration, WKWebViewConfiguration,
    };
    use tauri::AppHandle;

    thread_local! {
        // WebKit objects live on the main thread only.
        static CONTROLLER: RefCell<Option<Retained<WKWebExtensionController>>> = const { RefCell::new(None) };
        static CONTEXTS: RefCell<HashMap<String, Retained<WKWebExtensionContext>>> = RefCell::new(HashMap::new());
    }

    fn controller(mtm: MainThreadMarker) -> Retained<WKWebExtensionController> {
        CONTROLLER.with(|c| {
            c.borrow_mut()
                .get_or_insert_with(|| unsafe {
                    let config = WKWebExtensionControllerConfiguration::defaultConfiguration(mtm);
                    WKWebExtensionController::initWithConfiguration(WKWebExtensionController::alloc(mtm), &config)
                })
                .clone()
        })
    }

    /// Main thread only. Loads every installed extension.
    pub fn init(app: &AppHandle) {
        let Ok(entries) = std::fs::read_dir(super::dir(app)) else { return };
        for e in entries.flatten() {
            load(&e.path());
        }
    }

    /// Config for a new tab webview, wired to the extension controller. Main thread only.
    pub fn tab_config() -> Option<Retained<WKWebViewConfiguration>> {
        let mtm = MainThreadMarker::new()?;
        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        unsafe { config.setWebExtensionController(Some(&controller(mtm))) };
        Some(config)
    }

    pub fn load_on_main(app: &AppHandle, path: &Path) {
        let path = path.to_path_buf();
        let _ = app.run_on_main_thread(move || load(&path));
    }

    fn load(path: &Path) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Some(id) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { return };
        let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&path.to_string_lossy()), true);
        let handler = RcBlock::new(move |ext: *mut WKWebExtension, err: *mut NSError| unsafe {
            let Some(ext) = ext.as_ref() else {
                if let Some(e) = err.as_ref() {
                    eprintln!("extension {id}: {}", e.localizedDescription());
                }
                return;
            };
            let ctx = WKWebExtensionContext::contextForExtension(ext);
            // Installing from the store is the user's consent: grant what the manifest asks for.
            let granted = WKWebExtensionContextPermissionStatus::GrantedExplicitly;
            for p in ext.requestedPermissions().iter() {
                ctx.setPermissionStatus_forPermission(granted, &p);
            }
            for m in ext.allRequestedMatchPatterns().iter() {
                ctx.setPermissionStatus_forMatchPattern(granted, &m);
            }
            match controller(mtm).loadExtensionContext_error(&ctx) {
                Ok(()) => {
                    CONTEXTS.with(|c| c.borrow_mut().insert(id.clone(), ctx));
                }
                Err(e) => eprintln!("extension {id}: {}", e.localizedDescription()),
            }
        });
        unsafe { WKWebExtension::extensionWithResourceBaseURL_completionHandler(&url, &handler, mtm) };
    }

    pub fn unload(app: &AppHandle, id: &str) {
        let id = id.to_string();
        let _ = app.run_on_main_thread(move || {
            let Some(mtm) = MainThreadMarker::new() else { return };
            if let Some(ctx) = CONTEXTS.with(|c| c.borrow_mut().remove(&id)) {
                let _ = unsafe { controller(mtm).unloadExtensionContext_error(&ctx) };
            }
        });
    }
}
