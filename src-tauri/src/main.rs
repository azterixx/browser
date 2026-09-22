#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod extensions;
mod logs;
mod updater;

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    webview::{DownloadEvent, NewWindowResponse, PageLoadEvent},
    AppHandle, LogicalPosition, LogicalSize, Manager, Url, WebviewBuilder, WebviewUrl, Window,
    WindowEvent,
};

// Height of the strip with the island. Must match --strip-h in ui/style.css.
const TOP_H: f64 = 56.0;
// Auto-hide: the invisible hover zone along the top edge.
const EDGE_H: f64 = 6.0;
// Chrome layout: tab row + address row. Must match --chrome-h in ui/style.css.
const CHROME_H: f64 = 86.0;
// Arc layout: the vertical tab sidebar. Must match --sidebar-w in ui/style.css.
const SIDEBAR_W: f64 = 240.0;
// WKWebView's default UA has no "Safari" token, so Google & co. serve their legacy pages.
// WebView2 on Windows already sends a normal Edge UA.
#[cfg(target_os = "macos")]
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const SUSPEND_CHECK: Duration = Duration::from_secs(30);
// How often the session and history are written to disk.
const SAVE_EVERY: Duration = Duration::from_secs(3);
const HISTORY_MAX: usize = 5000;
const CLOSED_MAX: usize = 25;
const DOWNLOADS_MAX: usize = 200;
const ZOOM_STEPS: &[f64] = &[0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

#[derive(Serialize, Clone)]
struct TabInfo {
    id: u32,
    url: String,
    title: String,
    loading: bool,
    suspended: bool,
    // The page's top-edge color; the strip paints it so it reads as part of the site.
    tint: Option<String>,
    zoom: f64,
    // Shows a PDF in our viewer (browser://pdf), under the PDF's own URL.
    pdf: bool,
}

struct Tab {
    info: TabInfo,
    // When the tab last went to the background.
    last_active: Instant,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
enum Mode {
    // A strip above the page, painted with the page's top-edge color.
    #[default]
    Strip,
    // The page fills the window; the island floats over it.
    Float,
    // Like float, but the island hides until the cursor reaches the top edge.
    Autohide,
    // Classic tab row and address bar.
    Chrome,
    // Vertical tabs in a sidebar on the left.
    Arc,
    // The island floats at the bottom of the window.
    Bottom,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
enum Search {
    #[default]
    Google,
    Duckduckgo,
    Bing,
    Yandex,
}

impl Search {
    fn home(self) -> &'static str {
        match self {
            Search::Google => "https://www.google.com/",
            Search::Duckduckgo => "https://duckduckgo.com/",
            Search::Bing => "https://www.bing.com/",
            Search::Yandex => "https://ya.ru/",
        }
    }

    fn query(self) -> &'static str {
        match self {
            Search::Google => "https://www.google.com/search?q=",
            Search::Duckduckgo => "https://duckduckgo.com/?q=",
            Search::Bing => "https://www.bing.com/search?q=",
            Search::Yandex => "https://ya.ru/search/?text=",
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Settings {
    mode: Mode,
    search: Search,
    // New tabs open this; empty means the search engine's home page.
    home: String,
    // Background tabs sleep after this many minutes; 0 turns sleeping off.
    sleep_minutes: u64,
    // Reopen the tabs from last time on start.
    restore_session: bool,
    save_history: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Strip,
            search: Search::Google,
            home: String::new(),
            sleep_minutes: 10,
            restore_session: true,
            save_history: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct Visit {
    url: String,
    title: String,
    visits: u32,
    // Unix seconds.
    last: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct Session {
    tabs: Vec<SavedTab>,
    active: usize,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum DownloadState {
    Active,
    Done,
    Failed,
}

#[derive(Serialize, Deserialize, Clone)]
struct Download {
    id: u32,
    url: String,
    path: String,
    state: DownloadState,
    started: u64,
}

#[derive(Serialize, Deserialize)]
struct SavedTab {
    url: String,
    title: String,
}

fn data_file(app: &AppHandle, name: &str) -> Option<std::path::PathBuf> {
    Some(app.path().app_data_dir().ok()?.join(name))
}

fn read_json<T: serde::de::DeserializeOwned>(app: &AppHandle, name: &str) -> Option<T> {
    serde_json::from_slice(&std::fs::read(data_file(app, name)?).ok()?).ok()
}

// Write to a temp file and rename, so a crash mid-write can't leave half a file.
fn write_file(app: &AppHandle, name: &str, bytes: &[u8]) {
    let Some(p) = data_file(app, name) else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(tmp, p);
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    Some(app.path().app_config_dir().ok()?.join("settings.json"))
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, s: &Settings) {
    let Some(p) = settings_path(app) else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec_pretty(s) {
        let _ = std::fs::write(p, json);
    }
}

struct Browser {
    settings: Settings,
    // Size of the resting island as the UI measured it (float/autohide).
    pill: (f64, f64),
    // Auto-hide: the island is currently shown.
    revealed: bool,
    // A Web Store link the settings page should install when it opens.
    pending_install: Option<String>,
    tabs: Vec<Tab>,
    active: Option<u32>,
    next_id: u32,
    island_open: bool,
    // Last known strip color per host, so a revisit is painted instantly.
    tint_cache: HashMap<String, String>,
    history: Vec<Visit>,
    history_dirty: bool,
    // What session.json holds now, to skip identical writes.
    saved_session: String,
    // URLs of closed tabs, newest last.
    closed: Vec<String>,
    // Zoom per host; hosts at 100% are not stored.
    zoom: HashMap<String, f64>,
    zoom_dirty: bool,
    downloads: Vec<Download>,
    downloads_dirty: bool,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            pill: (440.0, 70.0),
            revealed: false,
            pending_install: None,
            tabs: Vec::new(),
            active: None,
            next_id: 0,
            island_open: false,
            tint_cache: HashMap::new(),
            history: Vec::new(),
            history_dirty: false,
            saved_session: String::new(),
            closed: Vec::new(),
            zoom: HashMap::new(),
            zoom_dirty: false,
            downloads: Vec::new(),
            downloads_dirty: false,
        }
    }
}

fn mode(app: &AppHandle) -> Mode {
    app.state::<State>().lock().unwrap().settings.mode
}

type State = Mutex<Browser>;

fn label(id: u32) -> String {
    format!("tab-{id}")
}

fn blank() -> Url {
    Url::parse("about:blank").unwrap()
}

fn home(app: &AppHandle) -> Url {
    let (search, custom) = {
        let st = app.state::<State>();
        let b = st.lock().unwrap();
        (b.settings.search, b.settings.home.clone())
    };
    if !custom.trim().is_empty() {
        return parse_input(app, &custom);
    }
    Url::parse(search.home()).unwrap()
}

// Internal pages live in the app bundle; the address bar shows them as browser://<page>.
const INTERNAL_PAGES: &[&str] = &["settings", "downloads"];

fn internal_page(url: &Url) -> Option<&str> {
    if !matches!(url.scheme(), "browser" | "chrome") {
        return None;
    }
    let page = url.host_str()?;
    INTERNAL_PAGES.iter().copied().find(|p| *p == page)
}

// Where the app's own pages are served: `tauri dev` uses a dev server, a build uses the app protocol.
fn app_base(app: &AppHandle) -> String {
    if tauri::is_dev() {
        if let Some(url) = &app.config().build.dev_url {
            let s = url.to_string();
            return if s.ends_with('/') { s } else { format!("{s}/") };
        }
    }
    (if cfg!(windows) { "http://tauri.localhost/" } else { "tauri://localhost/" }).to_string()
}

// The real URL the webview loads for a (possibly internal) URL.
fn real_url(app: &AppHandle, url: &Url) -> Url {
    match internal_page(url) {
        Some(page) => Url::parse(&format!("{}{page}.html", app_base(app))).unwrap(),
        None => url.clone(),
    }
}

const PDF_VIEWER: &str = "pdf.html?file=";
// Appended to a PDF URL to keep WebKit's own viewer (the fallback when ours can't load it).
const NATIVE_PDF: &str = "bnative";

fn viewer_file(app: &AppHandle, url: &str) -> Option<String> {
    let q = url.strip_prefix(app_base(app).as_str())?.strip_prefix(PDF_VIEWER)?;
    let q = q.split('#').next().unwrap_or(q);
    Some(url::form_urlencoded::parse(format!("f={q}").as_bytes()).next()?.1.into_owned())
}

fn viewer_url(app: &AppHandle, file: &str) -> String {
    let q: String = url::form_urlencoded::byte_serialize(file.as_bytes()).collect();
    format!("{}{PDF_VIEWER}{q}", app_base(app))
}

// What the address bar shows for a URL the webview reports.
fn display_url(app: &AppHandle, url: &str) -> String {
    if let Some(file) = viewer_file(app, url) {
        return file;
    }
    match url.strip_prefix(app_base(app).as_str()) {
        Some(rest) => format!("browser://{}", rest.split(['.', '?', '#']).next().unwrap_or("")),
        None => url.to_string(),
    }
}

fn parse_input(app: &AppHandle, input: &str) -> Url {
    let s = input.trim();
    if s.is_empty() {
        return blank();
    }
    if let Ok(u) = Url::parse(s) {
        if internal_page(&u).is_some() {
            return Url::parse(&format!("browser://{}", u.host_str().unwrap_or(""))).unwrap();
        }
        if matches!(u.scheme(), "http" | "https" | "about" | "file" | "data") {
            return u;
        }
    }
    if !s.contains(' ') {
        let scheme = if s.starts_with("localhost") || s.starts_with("127.0.0.1") { "http" } else { "https" };
        if s.contains('.') || scheme == "http" {
            if let Ok(u) = Url::parse(&format!("{scheme}://{s}")) {
                return u;
            }
        }
    }
    let q: String = url::form_urlencoded::byte_serialize(s.as_bytes()).collect();
    let search = app.state::<State>().lock().unwrap().settings.search;
    Url::parse(&format!("{}{q}", search.query())).unwrap()
}

fn state_json(app: &AppHandle) -> serde_json::Value {
    // In fullscreen macOS hides the traffic lights, so the UI drops their gap.
    let fullscreen = app.get_window("main").and_then(|w| w.is_fullscreen().ok()).unwrap_or(false);
    let st = app.state::<State>();
    let b = st.lock().unwrap();
    let tabs: Vec<&TabInfo> = b.tabs.iter().map(|t| &t.info).collect();
    let active_downloads = b.downloads.iter().filter(|d| d.state == DownloadState::Active).count();
    serde_json::json!({
        "tabs": tabs,
        "downloads": { "active": active_downloads, "total": b.downloads.len() },
        "active": b.active,
        "island": b.island_open,
        "mode": b.settings.mode,
        "revealed": b.revealed,
        "fullscreen": fullscreen,
    })
}

// Pushes the state into the toolbar. Eval is simpler than events and needs no JS API in the UI.
fn render(app: &AppHandle) {
    let json = state_json(app);
    if let Some(ui) = app.get_webview("ui") {
        let _ = ui.eval(format!("window.render && window.render({json})"));
    }
}

fn update(app: &AppHandle, id: u32, f: impl FnOnce(&mut TabInfo)) {
    {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        match b.tabs.iter_mut().find(|t| t.info.id == id) {
            Some(t) => f(&mut t.info),
            None => return,
        }
    }
    render(app);
}

fn window_size(window: &Window) -> LogicalSize<f64> {
    let scale = window.scale_factor().unwrap_or(1.0);
    window.inner_size().unwrap_or_default().to_logical::<f64>(scale)
}

fn content_bounds(app: &AppHandle, window: &Window) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    let size = window_size(window);
    let (left, top) = match mode(app) {
        Mode::Strip => (0.0, TOP_H),
        Mode::Chrome => (0.0, CHROME_H),
        Mode::Arc => (SIDEBAR_W, 0.0),
        Mode::Float | Mode::Autohide | Mode::Bottom => (0.0, 0.0),
    };
    (
        LogicalPosition::new(left, top),
        LogicalSize::new((size.width - left).max(0.0), (size.height - top).max(0.0)),
    )
}

// Where the toolbar webview sits. Open: the whole window, so the panel can overlap the page.
// Closed: the strip, just the island's box, or a thin hover zone, depending on the mode.
fn ui_bounds(app: &AppHandle, window: &Window) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    let full = window_size(window);
    let (open, mode, (pw, ph), revealed) = {
        let st = app.state::<State>();
        let b = st.lock().unwrap();
        (b.island_open, b.settings.mode, b.pill, b.revealed)
    };
    let origin = LogicalPosition::new(0.0, 0.0);
    if open {
        return (origin, full);
    }
    let w = pw.min(full.width);
    let center = ((full.width - w) / 2.0).round();
    match mode {
        Mode::Strip => (origin, LogicalSize::new(full.width, TOP_H)),
        Mode::Chrome => (origin, LogicalSize::new(full.width, CHROME_H)),
        Mode::Arc => (origin, LogicalSize::new(SIDEBAR_W, full.height)),
        Mode::Autohide if !revealed => (origin, LogicalSize::new(full.width, EDGE_H)),
        Mode::Bottom => (LogicalPosition::new(center, (full.height - ph).max(0.0)), LogicalSize::new(w, ph)),
        Mode::Float | Mode::Autohide => (LogicalPosition::new(center, 0.0), LogicalSize::new(w, ph)),
    }
}

fn place_ui(app: &AppHandle) {
    let (Some(window), Some(ui)) = (app.get_window("main"), app.get_webview("ui")) else { return };
    let (pos, size) = ui_bounds(app, &window);
    let _ = ui.set_position(pos);
    let _ = ui.set_size(size);
}

fn relayout(app: &AppHandle) {
    let Some(window) = app.get_window("main") else { return };
    let (pos, size) = content_bounds(app, &window);
    place_ui(app);
    let ids: Vec<u32> = app.state::<State>().lock().unwrap().tabs.iter().map(|t| t.info.id).collect();
    for id in ids {
        if let Some(wv) = app.get_webview(&label(id)) {
            let _ = wv.set_position(pos);
            let _ = wv.set_size(size);
        }
    }
}

fn spawn_webview(app: &AppHandle, id: u32, url: Url) -> tauri::Result<()> {
    // The extension-aware WKWebViewConfiguration is main-thread only, so build tabs there.
    #[cfg(target_os = "macos")]
    if objc2::MainThreadMarker::new().is_none() {
        let (tx, rx) = std::sync::mpsc::channel();
        let a = app.clone();
        app.run_on_main_thread(move || {
            let _ = tx.send(spawn_webview(&a, id, url));
        })?;
        return rx.recv().unwrap_or(Ok(()));
    }
    let window = app.get_window("main").expect("main window");
    let (a1, a2, a3, a4) = (app.clone(), app.clone(), app.clone(), app.clone());
    let builder = WebviewBuilder::new(label(id), WebviewUrl::External(real_url(app, &url)));
    #[cfg(target_os = "macos")]
    let builder = builder.user_agent(USER_AGENT);
    #[cfg(target_os = "macos")]
    let builder = match extensions::mac::tab_config() {
        Some(config) => builder.with_webview_configuration(config),
        None => builder,
    };
    // WebView2 needs the same environment options on every webview.
    #[cfg(windows)]
    let builder = builder.browser_extensions_enabled(true).extensions_path(extensions::dir(app));
    let builder = builder
        .on_page_load(move |wv, p| {
            // about:blank never reports Finished, so it must not count as loading.
            let loading = matches!(p.event(), PageLoadEvent::Started) && p.url().scheme() != "about";
            let viewer = viewer_file(&a1, p.url().as_str()).is_some();
            let url = display_url(&a1, p.url().as_str());
            let cached = if loading { cached_tint(&a1, &url) } else { None };
            let poll_url = url.clone();
            if matches!(p.event(), PageLoadEvent::Finished) {
                if !viewer {
                    record_visit(&a1, id, &url);
                    open_pdf_viewer(&a1, &wv, p.url());
                }
            } else {
                // The webview keeps its zoom across sites; each host gets its own.
                // The PDF viewer zooms the document itself.
                let zoom = if viewer { 1.0 } else { host_zoom(&a1, &url) };
                let _ = wv.set_zoom(zoom);
                update(&a1, id, |t| {
                    t.zoom = zoom;
                    t.pdf = viewer;
                });
            }
            update(&a1, id, |t| {
                t.url = url;
                t.loading = loading;
                if cached.is_some() {
                    t.tint = cached;
                }
            });
            if loading {
                poll_tint(&a1, id, poll_url);
            } else {
                // Final check: scripts may repaint the header after load.
                sample_tint_later(&a1, id, &[0, 800]);
            }
        })
        .on_document_title_changed(move |_, title| {
            retitle_visit(&a2, id, &title);
            update(&a2, id, |t| t.title = title);
            // SPA navigations change the title without a page load.
            sample_tint_later(&a2, id, &[300]);
        })
        // target=_blank and window.open become normal tabs.
        .on_new_window(move |url, _| {
            let app = a3.clone();
            thread::spawn(move || {
                let _ = open_tab(&app, url);
            });
            NewWindowResponse::Deny
        })
        // Files go to ~/Downloads; wry picks the name and adds " (1)" on clashes.
        .on_download(move |_, event| {
            match event {
                DownloadEvent::Requested { url, destination } => download_started(&a4, url.as_str(), destination),
                DownloadEvent::Finished { url, path, success } => download_finished(&a4, url.as_str(), path, success),
                _ => {}
            }
            true
        });
    let (pos, size) = content_bounds(app, &window);
    let wv = window.add_child(builder, pos, size)?;
    // Trackpad pinch and two-finger double tap zoom, like Safari and Chrome.
    #[cfg(target_os = "macos")]
    let _ = wv.with_webview(|pv| unsafe {
        if let Some(view) = (pv.inner() as *mut objc2::runtime::AnyObject).as_ref() {
            let _: () = objc2::msg_send![view, setAllowsMagnification: true];
        }
    });
    // New webviews land on top; the island must stay above them.
    if let Some(ui) = app.get_webview("ui") {
        ui.reparent(&window)?;
    }
    Ok(())
}

fn set_island_open(app: &AppHandle, open: bool) {
    {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        b.island_open = open;
        if !open {
            b.revealed = false;
        }
    }
    place_ui(app);
    let Some(ui) = app.get_webview("ui") else { return };
    if open {
        let _ = ui.set_focus();
        let _ = ui.eval("window.focusAddress && window.focusAddress()");
    } else if let Some(wv) = active_webview(app) {
        let _ = wv.set_focus();
    }
    render(app);
}

// Finds the dominant background color along the page's top edge.
const TINT_JS: &str = r#"(() => { try {
  const parse = (c) => {
    const m = c && c.match(/rgba?\(([^)]+)\)/);
    if (!m) return null;
    const p = m[1].split(/[\s,\/]+/).filter(Boolean).map(Number);
    return p.length === 3 ? [...p, 1] : p;
  };
  const bgOf = (el) => {
    for (; el; el = el.parentElement) {
      const p = parse(getComputedStyle(el).backgroundColor);
      if (p && p[3] > 0) return p;
    }
    return null;
  };
  // Before CSS arrives nothing has a background; don't mistake that for white.
  const own = (document.body && bgOf(document.body)) || bgOf(document.documentElement);
  if (!own && document.readyState !== 'complete') return null;
  const canvas = own || [255, 255, 255, 1];
  const counts = new Map();
  for (const f of [0.02, 0.2, 0.4, 0.6, 0.8, 0.98]) {
    const el = document.elementFromPoint(innerWidth * f, 1);
    let p = (el && bgOf(el)) || canvas;
    if (p[3] < 1) p = [0, 1, 2].map((i) => p[i] * p[3] + canvas[i] * (1 - p[3]));
    const hex = '#' + p.slice(0, 3).map((v) => Math.round(v).toString(16).padStart(2, '0')).join('');
    counts.set(hex, (counts.get(hex) || 0) + 1);
  }
  let best = null, n = 0;
  for (const [k, v] of counts) if (v > n) { best = k; n = v; }
  return [location.host.replace(/^www\./, ''), best];
} catch (e) { return null; } })()"#;

// Find bar, injected into the page. Built with DOM calls and constructed style sheets,
// because many sites forbid innerHTML (Trusted Types) and inline styles (CSP).
const FIND_JS: &str = r#"window.__bfind || (window.__bfind = (() => {
  const MAX = 1000;
  const sheet = (css) => { const s = new CSSStyleSheet(); s.replaceSync(css); return s; };
  document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet(
    '::highlight(bfind-all){background:#fde68a;color:#000}::highlight(bfind-cur){background:#f59e0b;color:#000}')];
  const host = document.createElement('div');
  host.style.cssText = 'all:initial;position:fixed;top:14px;right:16px;z-index:2147483647;display:none';
  const root = host.attachShadow({ mode: 'closed' });
  root.adoptedStyleSheets = [sheet(`
    .bar{display:flex;align-items:center;gap:2px;padding:5px 5px 5px 12px;border-radius:12px;
      font:13px -apple-system,system-ui,sans-serif;background:#fff;color:#1f1f1f;
      box-shadow:0 0 0 .5px rgba(0,0,0,.14),0 8px 28px rgba(0,0,0,.16)}
    input{all:unset;width:190px;padding:4px 0}
    .n{min-width:44px;text-align:right;padding-right:6px;color:#8a8a8a;font-variant-numeric:tabular-nums}
    button{all:unset;width:26px;height:26px;border-radius:7px;display:grid;place-items:center;cursor:default;color:#555}
    button:hover{background:rgba(0,0,0,.07)}
    svg{width:14px;height:14px;fill:none;stroke:currentColor;stroke-width:1.5;stroke-linecap:round;stroke-linejoin:round}
    @media (prefers-color-scheme:dark){.bar{background:#2b2b2d;color:#eee;box-shadow:0 0 0 .5px rgba(255,255,255,.14),0 8px 28px rgba(0,0,0,.5)}
      button{color:#bbb}button:hover{background:rgba(255,255,255,.1)}}`)];
  const make = (tag, cls) => { const e = document.createElement(tag); if (cls) e.className = cls; return e; };
  const icon = (d) => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', '0 0 16 16');
    const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    path.setAttribute('d', d);
    svg.append(path);
    return svg;
  };
  const bar = make('div', 'bar');
  const input = make('input');
  input.placeholder = 'Find in page';
  input.spellcheck = false;
  const count = make('span', 'n');
  const button = (d, title, fn) => {
    const b = make('button');
    b.title = title;
    b.append(icon(d));
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', fn);
    return b;
  };
  bar.append(input, count,
    button('M3.5 9.75 8 5.25l4.5 4.5', 'Previous (Shift+Enter)', () => step(-1)),
    button('M3.5 6.25 8 10.75l4.5-4.5', 'Next (Enter)', () => step(1)),
    button('M4.75 4.75l6.5 6.5M11.25 4.75l-6.5 6.5', 'Close (Esc)', () => close()));
  root.append(bar);
  // Keep the page's own shortcuts from seeing what is typed here.
  for (const t of ['keydown', 'keyup', 'keypress']) host.addEventListener(t, (e) => e.stopPropagation());

  let ranges = [], cur = -1, timer = 0;
  const SKIP = /^(SCRIPT|STYLE|NOSCRIPT|TEMPLATE|TEXTAREA|INPUT|SELECT|OPTION)$/;
  function search() {
    const needle = input.value.toLowerCase();
    ranges = [];
    if (needle) {
      const seen = new Map();
      const visible = (el) => {
        if (!seen.has(el)) seen.set(el, el.checkVisibility ? el.checkVisibility({ visibilityProperty: true }) : true);
        return seen.get(el);
      };
      const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_TEXT, {
        acceptNode: (n) => {
          const p = n.parentElement;
          return p && !SKIP.test(p.tagName) && visible(p) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT;
        },
      });
      for (let n; ranges.length < MAX && (n = walker.nextNode());) {
        const text = n.data.toLowerCase();
        for (let i = text.indexOf(needle); i !== -1 && ranges.length < MAX; i = text.indexOf(needle, i + needle.length)) {
          const r = new Range();
          r.setStart(n, i);
          r.setEnd(n, i + needle.length);
          ranges.push(r);
        }
      }
    }
    // Start at the first match below the top of the screen, like Safari.
    cur = ranges.findIndex((r) => r.getBoundingClientRect().bottom > 0);
    if (cur < 0 && ranges.length) cur = 0;
    paint(true);
  }
  function paint(scroll) {
    if (!window.CSS || !CSS.highlights) return;
    CSS.highlights.set('bfind-all', new Highlight(...ranges));
    if (cur >= 0) CSS.highlights.set('bfind-cur', new Highlight(ranges[cur]));
    else CSS.highlights.delete('bfind-cur');
    count.textContent = input.value ? (ranges.length ? `${cur + 1}/${ranges.length}${ranges.length >= MAX ? '+' : ''}` : '0/0') : '';
    const r = ranges[cur];
    if (scroll && r) {
      const box = r.getBoundingClientRect();
      if (box.top < 60 || box.bottom > innerHeight - 20) r.startContainer.parentElement.scrollIntoView({ block: 'center' });
    }
  }
  function step(d) {
    if (host.style.display === 'none') return open();
    if (!ranges.length) return;
    cur = (cur + d + ranges.length) % ranges.length;
    paint(true);
  }
  function open() {
    if (!host.isConnected) document.documentElement.append(host);
    host.style.display = 'block';
    input.focus();
    input.select();
    if (input.value) search();
  }
  function close() {
    host.style.display = 'none';
    ranges = [];
    cur = -1;
    if (window.CSS && CSS.highlights) { CSS.highlights.delete('bfind-all'); CSS.highlights.delete('bfind-cur'); }
    input.blur();
  }
  input.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(search, 60); });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { clearTimeout(timer); if (!ranges.length) search(); else step(e.shiftKey ? -1 : 1); e.preventDefault(); }
    else if (e.key === 'Escape') { close(); e.preventDefault(); }
  });
  return { open, step, close };
})())"#;

#[derive(Clone, Copy, PartialEq)]
enum Tint {
    // While loading: shown, but not remembered.
    Early,
    // After load: shown and remembered for the host.
    Load,
}

fn sample_tint(app: &AppHandle, id: u32, found: Option<Arc<AtomicBool>>, kind: Tint) {
    if mode(app) != Mode::Strip {
        return;
    }
    let Some(wv) = app.get_webview(&label(id)) else { return };
    // Not wv.url(): wry panics on macOS while the page has no URL yet.
    let (url, pdf) = {
        let st = app.state::<State>();
        let b = st.lock().unwrap();
        b.tabs.iter().find(|t| t.info.id == id).map(|t| (t.info.url.clone(), t.info.pdf)).unwrap_or_default()
    };
    // Our own pages (settings, the PDF viewer) are tinted too, but served from the app.
    let internal = pdf || url.starts_with("browser://");
    if !internal && !url.starts_with("http:") && !url.starts_with("https:") {
        update(app, id, |t| t.tint = None);
        return;
    }
    let a = app.clone();
    let _ = wv.eval_with_callback(TINT_JS, move |res| {
        let Ok(Some((host, tint))) = serde_json::from_str::<Option<(String, String)>>(&res) else { return };
        let url = {
            let st = a.state::<State>();
            let b = st.lock().unwrap();
            let Some(url) = b.tabs.iter().find(|t| t.info.id == id).map(|t| t.info.url.clone()) else { return };
            // Early samples can still see the previous page; only trust the current host.
            let expected = if internal { host_of(&app_base(&a)) } else { host_of(&url) };
            if expected.as_deref() != Some(host.as_str()) {
                return;
            }
            url
        };
        if let Some(f) = &found {
            f.store(true, Ordering::Relaxed);
        }
        // A PDF's viewer color must not stick to its website.
        apply_tint(&a, id, &url, tint, kind == Tint::Load && !internal);
    });
}

fn host_of(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    let host = u.host_str()?;
    Some(host.strip_prefix("www.").unwrap_or(host).to_string())
}

fn cached_tint(app: &AppHandle, url: &str) -> Option<String> {
    let host = host_of(url)?;
    app.state::<State>().lock().unwrap().tint_cache.get(&host).cloned()
}

fn apply_tint(app: &AppHandle, id: u32, url: &str, tint: String, cache: bool) {
    {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        if !b.tabs.iter().any(|t| t.info.id == id && t.info.url == url) {
            return; // navigated away meanwhile
        }
        if let (true, Some(host)) = (cache, host_of(url)) {
            b.tint_cache.insert(host, tint.clone());
        }
    }
    update(app, id, |t| t.tint = Some(tint));
}

// `at_ms` are offsets from now, ascending.
fn sample_tint_later(app: &AppHandle, id: u32, at_ms: &[u64]) {
    let (a, at) = (app.clone(), at_ms.to_vec());
    thread::spawn(move || {
        let mut prev = 0;
        for ms in at {
            thread::sleep(Duration::from_millis(ms - prev));
            prev = ms;
            sample_tint(&a, id, None, Tint::Load);
        }
    });
}

// From navigation start, ask the page every 50 ms until it has a background color.
fn poll_tint(app: &AppHandle, id: u32, url: String) {
    if mode(app) != Mode::Strip {
        return;
    }
    let (a, found) = (app.clone(), Arc::new(AtomicBool::new(false)));
    thread::spawn(move || {
        for _ in 0..100 {
            thread::sleep(Duration::from_millis(50));
            if found.load(Ordering::Relaxed) {
                return;
            }
            // A newer navigation started its own poll.
            let same = a.state::<State>().lock().unwrap().tabs.iter().any(|t| t.info.id == id && t.info.url == url);
            if !same {
                return;
            }
            sample_tint(&a, id, Some(found.clone()), Tint::Early);
        }
    });
}

fn open_tab(app: &AppHandle, url: Url) -> tauri::Result<u32> {
    let id = {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        b.next_id += 1;
        let id = b.next_id;
        b.tabs.push(Tab {
            info: TabInfo {
                id,
                url: url.to_string(),
                title: String::new(),
                loading: url.scheme() != "about",
                suspended: false,
                tint: None,
                zoom: 1.0,
                pdf: false,
            },
            last_active: Instant::now(),
        });
        id
    };
    spawn_webview(app, id, url)?;
    activate(app, id)?;
    Ok(id)
}

fn activate(app: &AppHandle, id: u32) -> tauri::Result<()> {
    let (prev, wake) = {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        if !b.tabs.iter().any(|t| t.info.id == id) {
            return Ok(());
        }
        let prev = b.active.replace(id);
        let now = Instant::now();
        let mut wake = None;
        for t in b.tabs.iter_mut() {
            if Some(t.info.id) == prev {
                t.last_active = now;
            }
            if t.info.id == id && t.info.suspended {
                t.info.suspended = false;
                t.info.loading = true;
                wake = Some(t.info.url.parse().unwrap_or_else(|_| blank()));
            }
        }
        (prev, wake)
    };
    if let Some(url) = wake {
        spawn_webview(app, id, url)?;
    }
    if let Some(p) = prev.filter(|p| *p != id) {
        if let Some(wv) = app.get_webview(&label(p)) {
            wv.hide()?;
        }
    }
    if let Some(wv) = app.get_webview(&label(id)) {
        wv.show()?;
    }
    render(app);
    sample_tint_later(app, id, &[50]);
    Ok(())
}

fn close_tab(app: &AppHandle, id: u32) -> tauri::Result<()> {
    let next = {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        let Some(idx) = b.tabs.iter().position(|t| t.info.id == id) else { return Ok(()) };
        let tab = b.tabs.remove(idx);
        if tab.info.url != "about:blank" {
            if b.closed.len() >= CLOSED_MAX {
                b.closed.remove(0);
            }
            b.closed.push(tab.info.url);
        }
        if b.active == Some(id) {
            b.active = None;
            b.tabs.get(idx).or_else(|| b.tabs.last()).map(|t| t.info.id)
        } else {
            None
        }
    };
    if let Some(wv) = app.get_webview(&label(id)) {
        wv.close()?;
    }
    let empty = app.state::<State>().lock().unwrap().tabs.is_empty();
    if empty {
        open_tab(app, home(app))?;
    } else if let Some(n) = next {
        activate(app, n)?;
    }
    render(app);
    Ok(())
}

// Frees background tabs: the webview is destroyed, only the URL stays.
fn suspend_idle(app: &AppHandle) {
    let victims: Vec<u32> = {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        let active = b.active;
        let minutes = b.settings.sleep_minutes;
        if minutes == 0 {
            return;
        }
        let after = Duration::from_secs(minutes * 60);
        b.tabs
            .iter_mut()
            .filter(|t| Some(t.info.id) != active && !t.info.suspended && t.last_active.elapsed() > after)
            .map(|t| {
                t.info.suspended = true;
                t.info.loading = false;
                t.info.id
            })
            .collect()
    };
    if victims.is_empty() {
        return;
    }
    for id in victims {
        if let Some(wv) = app.get_webview(&label(id)) {
            let _ = wv.close();
        }
    }
    render(app);
}

fn record_visit(app: &AppHandle, id: u32, url: &str) {
    if !url.starts_with("http:") && !url.starts_with("https:") {
        return;
    }
    let st = app.state::<State>();
    let mut b = st.lock().unwrap();
    if !b.settings.save_history {
        return;
    }
    let title = b.tabs.iter().find(|t| t.info.id == id).map(|t| t.info.title.clone()).unwrap_or_default();
    let now = now_secs();
    match b.history.iter_mut().find(|v| v.url == url) {
        Some(v) => {
            v.visits += 1;
            v.last = now;
            if !title.is_empty() {
                v.title = title;
            }
        }
        None => {
            b.history.push(Visit { url: url.to_string(), title, visits: 1, last: now });
            if b.history.len() > HISTORY_MAX {
                b.history.sort_by(|x, y| y.last.cmp(&x.last));
                b.history.truncate(HISTORY_MAX * 9 / 10);
            }
        }
    }
    b.history_dirty = true;
}

// Titles often arrive after the load finishes.
fn retitle_visit(app: &AppHandle, id: u32, title: &str) {
    let st = app.state::<State>();
    let mut b = st.lock().unwrap();
    let Some(url) = b.tabs.iter().find(|t| t.info.id == id).map(|t| t.info.url.clone()) else { return };
    if let Some(v) = b.history.iter_mut().find(|v| v.url == url) {
        if v.title != title && !title.is_empty() {
            v.title = title.to_string();
            b.history_dirty = true;
        }
    }
}

fn session_json(b: &Browser) -> String {
    let tabs: Vec<&TabInfo> = b.tabs.iter().map(|t| &t.info).filter(|t| t.url != "about:blank").collect();
    let session = Session {
        active: tabs.iter().position(|t| Some(t.id) == b.active).unwrap_or(0),
        tabs: tabs.iter().map(|t| SavedTab { url: t.url.clone(), title: t.title.clone() }).collect(),
    };
    serde_json::to_string(&session).unwrap_or_default()
}

fn persist(app: &AppHandle) {
    let (session, history, zoom, downloads) = {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        let session = if b.settings.restore_session {
            let json = session_json(&b);
            (json != b.saved_session).then(|| {
                b.saved_session = json.clone();
                json
            })
        } else {
            None
        };
        let history = std::mem::take(&mut b.history_dirty).then(|| serde_json::to_vec(&b.history).unwrap_or_default());
        let zoom = std::mem::take(&mut b.zoom_dirty).then(|| serde_json::to_vec(&b.zoom).unwrap_or_default());
        let downloads =
            std::mem::take(&mut b.downloads_dirty).then(|| serde_json::to_vec(&b.downloads).unwrap_or_default());
        (session, history, zoom, downloads)
    };
    if let Some(json) = session {
        write_file(app, "session.json", json.as_bytes());
    }
    if let Some(bytes) = history {
        write_file(app, "history.json", &bytes);
    }
    if let Some(bytes) = zoom {
        write_file(app, "zoom.json", &bytes);
    }
    if let Some(bytes) = downloads {
        write_file(app, "downloads.json", &bytes);
    }
}

// Restored tabs start asleep; only the active one loads right away.
fn restore_session(app: &AppHandle) -> tauri::Result<bool> {
    if !app.state::<State>().lock().unwrap().settings.restore_session {
        return Ok(false);
    }
    let Some(session) = read_json::<Session>(app, "session.json") else { return Ok(false) };
    if session.tabs.is_empty() {
        return Ok(false);
    }
    let active = {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        let first = b.next_id + 1;
        for t in session.tabs {
            b.next_id += 1;
            let id = b.next_id;
            b.tabs.push(Tab {
                info: TabInfo { id, url: t.url, title: t.title, loading: false, suspended: true, tint: None, zoom: 1.0, pdf: false },
                last_active: Instant::now(),
            });
        }
        first + session.active.min(b.tabs.len() - 1) as u32
    };
    activate(app, active)?;
    Ok(true)
}

// A short "125%" badge at the top of the page, like Chrome's zoom bubble.
// The page itself is zoomed, so the badge scales by 1/zoom to keep its size.
const ZOOM_HUD_JS: &str = r#"((z) => {
  let hud = window.__bzoomHud;
  if (!hud) {
    const host = document.createElement('div');
    host.style.cssText = 'all:initial;position:fixed;left:50%;z-index:2147483647;pointer-events:none';
    const root = host.attachShadow({ mode: 'closed' });
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(`
      .b{padding:7px 14px;border-radius:10px;font:600 14px -apple-system,system-ui,sans-serif;
        font-variant-numeric:tabular-nums;background:rgba(30,30,32,.86);color:#fff;
        box-shadow:0 6px 24px rgba(0,0,0,.25);opacity:0;transition:opacity .18s}
      .b.on{opacity:1}`);
    root.adoptedStyleSheets = [sheet];
    const badge = document.createElement('div');
    badge.className = 'b';
    root.append(badge);
    hud = window.__bzoomHud = { host, badge, timer: 0 };
  }
  if (!hud.host.isConnected) document.documentElement.append(hud.host);
  hud.host.style.top = (16 / z) + 'px';
  hud.host.style.transform = `translateX(-50%) scale(${1 / z})`;
  hud.host.style.transformOrigin = 'top center';
  hud.badge.textContent = Math.round(z * 100) + '%';
  hud.badge.classList.add('on');
  clearTimeout(hud.timer);
  hud.timer = setTimeout(() => hud.badge.classList.remove('on'), 1200);
})"#;

// WebKit's own PDF view has no thumbnails, search or page controls, so PDFs move to ours.
// location.replace keeps Back working: the PDF URL doesn't stay in the history.
fn open_pdf_viewer(app: &AppHandle, wv: &tauri::Webview, url: &Url) {
    if !matches!(url.scheme(), "http" | "https" | "file") || url.fragment() == Some(NATIVE_PDF) {
        return;
    }
    let (a, w, file) = (app.clone(), wv.clone(), url.to_string());
    let _ = wv.eval_with_callback("document.contentType", move |res| {
        if serde_json::from_str::<String>(&res).ok().as_deref() != Some("application/pdf") {
            return;
        }
        let target = serde_json::to_string(&viewer_url(&a, &file)).unwrap_or_default();
        let _ = w.eval(format!("location.replace({target})"));
    });
}

fn download_dir(app: &AppHandle) -> std::path::PathBuf {
    app.path().download_dir().unwrap_or_else(|_| std::env::temp_dir())
}

// "file.pdf", then "file (1).pdf", like wry does for downloads.
fn free_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    let mut path = dir.join(name);
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    path
}

fn host_zoom(app: &AppHandle, url: &str) -> f64 {
    let Some(host) = host_of(url) else { return 1.0 };
    app.state::<State>().lock().unwrap().zoom.get(&host).copied().unwrap_or(1.0)
}

// `dir`: 1 in, -1 out, 0 back to 100%. Applies to every open tab of the same host.
fn zoom(app: &AppHandle, dir: i32) {
    let Some(url) = ({
        let st = app.state::<State>();
        let b = st.lock().unwrap();
        b.tabs.iter().find(|t| Some(t.info.id) == b.active).map(|t| t.info.url.clone())
    }) else {
        return;
    };
    let pdf = {
        let st = app.state::<State>();
        let b = st.lock().unwrap();
        b.tabs.iter().any(|t| Some(t.info.id) == b.active && t.info.pdf)
    };
    if pdf {
        if let Some(wv) = active_webview(app) {
            let _ = wv.eval(format!("window.__pdfZoom && window.__pdfZoom({dir})"));
        }
        return;
    }
    let Some(host) = host_of(&url) else { return };
    let cur = host_zoom(app, &url);
    let next = match dir {
        0 => 1.0,
        d if d > 0 => ZOOM_STEPS.iter().copied().find(|s| *s > cur + 0.001).unwrap_or(cur),
        _ => ZOOM_STEPS.iter().rev().copied().find(|s| *s < cur - 0.001).unwrap_or(cur),
    };
    if let Some(wv) = active_webview(app) {
        let _ = wv.set_zoom(next);
        let _ = wv.eval(format!("{ZOOM_HUD_JS}({next})"));
    }
    let ids: Vec<u32> = {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        if (next - 1.0).abs() < 0.001 {
            b.zoom.remove(&host);
        } else {
            b.zoom.insert(host.clone(), next);
        }
        b.zoom_dirty = true;
        b.tabs.iter().filter(|t| host_of(&t.info.url).as_deref() == Some(host.as_str())).map(|t| t.info.id).collect()
    };
    for id in ids {
        if let Some(wv) = app.get_webview(&label(id)) {
            let _ = wv.set_zoom(next);
        }
        update(app, id, |t| t.zoom = next);
    }
}

fn download_started(app: &AppHandle, url: &str, destination: &std::path::Path) {
    {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        let id = b.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        b.downloads.push(Download {
            id,
            url: url.to_string(),
            path: destination.to_string_lossy().into_owned(),
            state: DownloadState::Active,
            started: now_secs(),
        });
        if b.downloads.len() > DOWNLOADS_MAX {
            b.downloads.remove(0);
        }
        b.downloads_dirty = true;
    }
    downloads_changed(app);
}

fn download_finished(app: &AppHandle, url: &str, path: Option<std::path::PathBuf>, success: bool) {
    {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        let Some(d) = b.downloads.iter_mut().rev().find(|d| d.url == url && d.state == DownloadState::Active) else { return };
        d.state = if success { DownloadState::Done } else { DownloadState::Failed };
        if let Some(p) = path {
            d.path = p.to_string_lossy().into_owned();
        }
        b.downloads_dirty = true;
    }
    downloads_changed(app);
}

fn downloads_changed(app: &AppHandle) {
    render(app);
    let ids: Vec<u32> = app
        .state::<State>()
        .lock()
        .unwrap()
        .tabs
        .iter()
        .filter(|t| t.info.url.starts_with("browser://downloads"))
        .map(|t| t.info.id)
        .collect();
    for id in ids {
        if let Some(wv) = app.get_webview(&label(id)) {
            let _ = wv.eval("window.refresh && window.refresh()");
        }
    }
}

fn active_webview(app: &AppHandle) -> Option<tauri::Webview> {
    let id = app.state::<State>().lock().unwrap().active?;
    app.get_webview(&label(id))
}

fn focus_address(app: &AppHandle) {
    // Chrome layout has a real address bar; everything else uses the island.
    if mode(app) == Mode::Chrome {
        if let Some(ui) = app.get_webview("ui") {
            let _ = ui.set_focus();
            let _ = ui.eval("window.focusInline && window.focusInline()");
        }
        return;
    }
    set_island_open(app, true);
}

fn err(e: tauri::Error) -> String {
    e.to_string()
}

#[tauri::command]
async fn get_state(app: AppHandle) -> serde_json::Value {
    state_json(&app)
}

#[tauri::command]
async fn new_tab(app: AppHandle, input: Option<String>) -> Result<(), String> {
    let url = match input.as_deref() {
        Some(i) => parse_input(&app, i),
        None => home(&app),
    };
    open_tab(&app, url).map_err(err)?;
    set_island_open(&app, false);
    Ok(())
}

#[tauri::command]
async fn activate_tab(app: AppHandle, id: u32) -> Result<(), String> {
    activate(&app, id).map_err(err)?;
    set_island_open(&app, false);
    Ok(())
}

#[tauri::command]
async fn close(app: AppHandle, id: Option<u32>) -> Result<(), String> {
    let id = id.or_else(|| app.state::<State>().lock().unwrap().active);
    match id {
        Some(id) => close_tab(&app, id).map_err(err),
        None => Ok(()),
    }
}

#[tauri::command]
async fn navigate(app: AppHandle, input: String) -> Result<(), String> {
    let url = parse_input(&app, &input);
    match active_webview(&app) {
        Some(wv) => {
            wv.navigate(real_url(&app, &url)).map_err(err)?;
            set_island_open(&app, false);
            Ok(())
        }
        None => open_tab(&app, url).map(|_| ()).map_err(err),
    }
}

#[tauri::command]
async fn island(app: AppHandle, open: bool) {
    set_island_open(&app, open);
}

#[tauri::command]
async fn pill_size(app: AppHandle, width: f64, height: f64) {
    app.state::<State>().lock().unwrap().pill = (width.ceil(), height.ceil());
    place_ui(&app);
}

#[tauri::command]
async fn reveal(app: AppHandle, on: bool) {
    {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        if b.revealed == on || b.island_open {
            return;
        }
        b.revealed = on;
    }
    place_ui(&app);
    render(&app);
}

fn apply_settings(app: &AppHandle, new: Settings) {
    let old_mode = {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        let old = b.settings.mode;
        b.settings = new.clone();
        b.revealed = false;
        old
    };
    save_settings(app, &new);
    // Off means nothing to come back to later, not just "don't read it".
    if !new.restore_session {
        app.state::<State>().lock().unwrap().saved_session.clear();
        if let Some(p) = data_file(app, "session.json") {
            let _ = std::fs::remove_file(p);
        }
    }
    if new.mode != old_mode {
        relayout(app);
        render(app);
        if new.mode == Mode::Strip {
            if let Some(id) = app.state::<State>().lock().unwrap().active {
                sample_tint_later(app, id, &[0]);
            }
        }
    }
}

#[tauri::command]
async fn set_mode(app: AppHandle, mode: Mode) {
    let mut s = app.state::<State>().lock().unwrap().settings.clone();
    s.mode = mode;
    apply_settings(&app, s);
}

#[tauri::command]
async fn get_settings(app: AppHandle) -> serde_json::Value {
    let settings = app.state::<State>().lock().unwrap().settings.clone();
    serde_json::json!({
        "settings": settings,
        "version": app.package_info().version.to_string(),
        "os": std::env::consts::OS,
    })
}

#[tauri::command]
async fn set_settings(app: AppHandle, settings: Settings) {
    apply_settings(&app, settings);
}

// Switches to the page if a tab already shows it.
fn find_page(app: &AppHandle, page: &str) -> Option<u32> {
    let prefix = format!("browser://{page}");
    app.state::<State>().lock().unwrap().tabs.iter().find(|t| t.info.url.starts_with(&prefix)).map(|t| t.info.id)
}

#[tauri::command]
async fn open_downloads(app: AppHandle) -> Result<(), String> {
    match find_page(&app, "downloads") {
        Some(id) => activate(&app, id).map_err(err)?,
        None => {
            open_tab(&app, Url::parse("browser://downloads").unwrap()).map_err(err)?;
        }
    }
    set_island_open(&app, false);
    Ok(())
}

#[tauri::command]
async fn downloads_list(app: AppHandle) -> Vec<serde_json::Value> {
    let list = app.state::<State>().lock().unwrap().downloads.clone();
    list.into_iter()
        .rev()
        .map(|d| {
            let exists = std::path::Path::new(&d.path).exists();
            let mut v = serde_json::to_value(&d).unwrap_or_default();
            v["exists"] = exists.into();
            v
        })
        .collect()
}

// `reveal`: show the file in Finder instead of opening it.
#[tauri::command]
async fn download_open(app: AppHandle, id: u32, reveal: bool) -> Result<(), String> {
    let path = {
        let st = app.state::<State>();
        let b = st.lock().unwrap();
        b.downloads.iter().find(|d| d.id == id).map(|d| d.path.clone()).ok_or("no such download")?
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        if reveal {
            c.arg("-R");
        }
        c.arg(&path);
        c
    };
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("explorer");
        c.arg(if reveal { format!("/select,{path}") } else { path.clone() });
        c
    };
    #[cfg(not(any(target_os = "macos", windows)))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        let p = std::path::Path::new(&path);
        c.arg(if reveal { p.parent().unwrap_or(p) } else { p });
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn downloads_clear(app: AppHandle) {
    {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        b.downloads.retain(|d| d.state == DownloadState::Active);
        b.downloads_dirty = true;
    }
    downloads_changed(&app);
}

// The viewer is a local page, so fetching the PDF itself would hit CORS. Rust fetches it instead.
// It has no cookies of the site, so PDFs behind a login fail; the viewer falls back then.
#[tauri::command]
async fn pdf_fetch(url: String) -> Result<tauri::ipc::Response, String> {
    let bytes = tauri::async_runtime::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let u = Url::parse(&url).map_err(|e| e.to_string())?;
        if u.scheme() == "file" {
            let path = u.to_file_path().map_err(|_| "bad file path".to_string())?;
            return std::fs::read(path).map_err(|e| e.to_string());
        }
        let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
        let agent = ureq::AgentBuilder::new().tls_connector(Arc::new(tls)).build();
        let req = agent.get(&url);
        #[cfg(target_os = "macos")]
        let req = req.set("User-Agent", USER_AGENT);
        let res = req.call().map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(res.into_reader(), 1 << 30), &mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    })
    .await
    .map_err(|e| e.to_string())??;
    if !bytes.starts_with(b"%PDF") && !bytes.windows(5).take(1024).any(|w| w == b"%PDF-") {
        return Err("not a PDF".into());
    }
    Ok(tauri::ipc::Response::new(bytes))
}

// Saves what the viewer sends (with the user's drawings, if any) to Downloads.
// Headers: x-name (the file name) and x-url (where the PDF came from), both URI-encoded.
#[tauri::command]
async fn pdf_save(app: AppHandle, request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("no data".into()) };
    let header = |k: &str| {
        let v = request.headers().get(k).and_then(|v| v.to_str().ok()).unwrap_or("");
        url::form_urlencoded::parse(format!("v={v}").as_bytes()).next().map(|(_, v)| v.into_owned()).unwrap_or_default()
    };
    let name: String = header("x-name").chars().map(|c| if "/\\:".contains(c) { '_' } else { c }).collect();
    let name = if name.trim().is_empty() { "document.pdf".to_string() } else { name };
    let path = free_path(&download_dir(&app), &name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        let id = b.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        b.downloads.push(Download {
            id,
            url: header("x-url"),
            path: path.to_string_lossy().into_owned(),
            state: DownloadState::Done,
            started: now_secs(),
        });
        b.downloads_dirty = true;
    }
    downloads_changed(&app);
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
async fn print_page(app: AppHandle) -> Result<(), String> {
    active_webview(&app).ok_or("no tab")?.print().map_err(err)
}

#[tauri::command]
async fn open_settings(app: AppHandle, install: Option<String>) -> Result<(), String> {
    app.state::<State>().lock().unwrap().pending_install = install;
    let existing = find_page(&app, "settings");
    match existing {
        Some(id) => {
            activate(&app, id).map_err(err)?;
            // Already open: let it pick up a pending install.
            if let Some(wv) = app.get_webview(&label(id)) {
                let _ = wv.eval("window.checkPending && window.checkPending()");
            }
        }
        None => {
            open_tab(&app, Url::parse("browser://settings").unwrap()).map_err(err)?;
        }
    }
    set_island_open(&app, false);
    Ok(())
}

#[tauri::command]
async fn take_pending_install(app: AppHandle) -> Option<String> {
    app.state::<State>().lock().unwrap().pending_install.take()
}

#[tauri::command]
async fn clear_data(app: AppHandle) -> Result<(), String> {
    // One shared data store: clearing through any tab clears all of them.
    let wv = active_webview(&app).ok_or("no tab")?;
    wv.clear_all_browsing_data().map_err(err)?;
    app.state::<State>().lock().unwrap().tint_cache.clear();
    Ok(())
}

#[tauri::command]
async fn reopen_tab(app: AppHandle) -> Result<(), String> {
    let Some(url) = app.state::<State>().lock().unwrap().closed.pop() else { return Ok(()) };
    open_tab(&app, parse_input(&app, &url)).map_err(err)?;
    set_island_open(&app, false);
    Ok(())
}

#[tauri::command]
async fn suggest(app: AppHandle, query: String) -> Vec<Visit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let now = now_secs();
    let st = app.state::<State>();
    let b = st.lock().unwrap();
    let mut hits: Vec<(f64, &Visit)> = b
        .history
        .iter()
        .filter_map(|v| {
            let url = v.url.to_lowercase();
            let title = v.title.to_lowercase();
            if !words.iter().all(|w| url.contains(w) || title.contains(w)) {
                return None;
            }
            let bare = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
            let bare = bare.strip_prefix("www.").unwrap_or(bare);
            let days = now.saturating_sub(v.last) as f64 / 86400.0;
            let mut score = (v.visits as f64).ln_1p() * 10.0 - days.min(90.0) * 0.3;
            if bare.starts_with(&q) {
                // Short URLs first: "git" should suggest github.com before a deep link.
                score += 50.0 - (bare.len() as f64 / 10.0).min(20.0);
            } else if title.starts_with(&q) {
                score += 15.0;
            }
            Some((score, v))
        })
        .collect();
    hits.sort_by(|a, b| b.0.total_cmp(&a.0));
    hits.into_iter().take(8).map(|(_, v)| v.clone()).collect()
}

#[tauri::command]
async fn clear_history(app: AppHandle) {
    {
        let st = app.state::<State>();
        let mut b = st.lock().unwrap();
        b.history.clear();
        b.history_dirty = true;
    }
    persist(&app);
}

#[tauri::command]
async fn ext_list(app: AppHandle) -> Vec<extensions::Info> {
    extensions::list(&app)
}

#[tauri::command]
async fn ext_install(app: AppHandle, input: String) -> Result<extensions::Info, String> {
    tauri::async_runtime::spawn_blocking(move || extensions::install(&app, &input))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn ext_remove(app: AppHandle, id: String) -> Result<(), String> {
    extensions::remove(&app, &id)
}

#[tauri::command]
async fn nav(app: AppHandle, action: String) -> Result<(), String> {
    let Some(wv) = active_webview(&app) else { return Ok(()) };
    match action.as_str() {
        "back" => wv.eval("history.back()"),
        "forward" => wv.eval("history.forward()"),
        "reload" => wv.reload(),
        "stop" => wv.eval("window.stop()"),
        "find" => {
            let _ = wv.set_focus();
            wv.eval(format!("{FIND_JS}; window.__bfind.open()"))
        }
        "zoom_in" | "zoom_out" | "zoom_reset" => {
            zoom(&app, match action.as_str() { "zoom_in" => 1, "zoom_out" => -1, _ => 0 });
            Ok(())
        }
        "find_next" | "find_prev" => {
            let _ = wv.set_focus();
            let step = if action == "find_next" { 1 } else { -1 };
            wv.eval(format!("{FIND_JS}; window.__bfind.step({step})"))
        }
        _ => Ok(()),
    }
    .map_err(err)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let tab = Submenu::with_items(
        app,
        "Tab",
        true,
        &[
            &MenuItem::with_id(app, "new_tab", "New Tab", true, Some("CmdOrCtrl+T"))?,
            &MenuItem::with_id(app, "close_tab", "Close Tab", true, Some("CmdOrCtrl+W"))?,
            &MenuItem::with_id(app, "reopen_tab", "Reopen Closed Tab", true, Some("CmdOrCtrl+Shift+T"))?,
            &MenuItem::with_id(app, "focus_address", "Open Location", true, Some("CmdOrCtrl+L"))?,
            &MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?,
            &MenuItem::with_id(app, "reload", "Reload", true, Some("CmdOrCtrl+R"))?,
            &MenuItem::with_id(app, "back", "Back", true, Some("CmdOrCtrl+["))?,
            &MenuItem::with_id(app, "forward", "Forward", true, Some("CmdOrCtrl+]"))?,
        ],
    )?;
    // Without the Edit menu, Cmd+C/V/A do nothing in WKWebView.
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "find", "Find…", true, Some("CmdOrCtrl+F"))?,
            &MenuItem::with_id(app, "find_next", "Find Next", true, Some("CmdOrCtrl+G"))?,
            &MenuItem::with_id(app, "find_prev", "Find Previous", true, Some("CmdOrCtrl+Shift+G"))?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "View",
        true,
        &[
            &MenuItem::with_id(app, "zoom_in", "Zoom In", true, Some("CmdOrCtrl+="))?,
            &MenuItem::with_id(app, "zoom_out", "Zoom Out", true, Some("CmdOrCtrl+-"))?,
            &MenuItem::with_id(app, "zoom_reset", "Actual Size", true, Some("CmdOrCtrl+0"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "downloads", "Downloads", true, Some("CmdOrCtrl+Shift+J"))?,
        ],
    )?;
    let app_menu = Submenu::with_items(
        app,
        "Browser",
        true,
        &[
            &PredefinedMenuItem::about(app, None, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &edit, &view, &tab])
}

fn ui_builder(app: &AppHandle) -> WebviewBuilder<tauri::Wry> {
    let builder = WebviewBuilder::new("ui", WebviewUrl::App("index.html".into()));
    #[cfg(windows)]
    let builder = builder.browser_extensions_enabled(true).extensions_path(extensions::dir(app));
    #[cfg(not(windows))]
    let _ = app;
    builder
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(logs::plugin())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(State::default())
        .manage(updater::Ready::default())
        .invoke_handler(tauri::generate_handler![
            get_state, new_tab, activate_tab, close, navigate, nav, island, pill_size, reveal, set_mode,
            ext_list, ext_install, ext_remove, get_settings, set_settings, open_settings,
            take_pending_install, clear_data, reopen_tab, suggest, clear_history, open_downloads,
            downloads_list, download_open, downloads_clear, pdf_fetch, pdf_save, print_page,
            updater::update_check, updater::update_install, logs::open_logs
        ])
        .on_menu_event(|app, event| {
            let app = app.clone();
            let id = event.id().as_ref().to_string();
            tauri::async_runtime::spawn(async move {
                let _ = match id.as_str() {
                    "new_tab" => new_tab(app, None).await,
                    "close_tab" => close(app, None).await,
                    "reopen_tab" => reopen_tab(app).await,
                    "downloads" => open_downloads(app).await,
                    "focus_address" => {
                        focus_address(&app);
                        Ok(())
                    }
                    "settings" => open_settings(app, None).await,
                    other => nav(app, other.to_string()).await,
                };
            });
        })
        .setup(|app| {
            let handle = app.handle().clone();
            logs::catch_panics();
            log::info!(
                "Browser {} starting on {} ({})",
                handle.package_info().version,
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            // No engine, no browser. Windows borrows WebView2 from Edge, and Wine
            // or a stripped-down system may not have it; say so instead of dying blank.
            match tauri::webview_version() {
                Ok(v) => log::info!("webview engine {v}"),
                Err(e) => {
                    log::error!("no webview engine: {e}");
                    let note = format!(
                        "Browser needs the WebView2 runtime and could not find it: {e}\n\
                         Install it from https://developer.microsoft.com/microsoft-edge/webview2\n"
                    );
                    eprintln!("{note}");
                    let _ = std::fs::write(std::env::temp_dir().join("browser-startup-error.log"), &note);
                    std::process::exit(1);
                }
            }
            {
                let st = app.state::<State>();
                let mut b = st.lock().unwrap();
                b.settings = load_settings(&handle);
                b.history = read_json(&handle, "history.json").unwrap_or_default();
                b.zoom = read_json(&handle, "zoom.json").unwrap_or_default();
                // An app quit mid-download leaves it unfinished for good.
                b.downloads = read_json::<Vec<Download>>(&handle, "downloads.json").unwrap_or_default();
                for d in b.downloads.iter_mut().filter(|d| d.state == DownloadState::Active) {
                    d.state = DownloadState::Failed;
                }
            }
            #[cfg(target_os = "macos")]
            extensions::mac::init(&handle);

            #[allow(unused_mut)]
            let mut wb = tauri::window::WindowBuilder::new(app, "main")
                .title("Browser")
                .inner_size(1200.0, 800.0)
                .min_inner_size(480.0, 300.0);
            #[cfg(target_os = "macos")]
            {
                wb = wb.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
                app.set_menu(build_menu(&handle)?)?;
            }
            let window = wb.build()?;
            log::info!("main window created");

            let os = std::env::consts::OS;
            let (pos, size) = ui_bounds(&handle, &window);
            let toolbar = |transparent: bool| {
                ui_builder(&handle)
                    .transparent(transparent)
                    .initialization_script(format!("document.documentElement.dataset.os = '{os}';"))
            };
            // A transparent child webview is a WebView2 feature that some Windows
            // builds refuse; an opaque toolbar is better than no browser at all.
            if let Err(e) = window.add_child(toolbar(true), pos, size) {
                log::warn!("transparent toolbar failed ({e}), retrying opaque");
                window.add_child(toolbar(false), pos, size)?;
            }
            log::info!("toolbar webview created");

            let h = handle.clone();
            window.on_window_event(move |e| {
                if let WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } = e {
                    relayout(&h);
                    render(&h);
                }
            });

            let h = handle.clone();
            thread::spawn(move || loop {
                thread::sleep(SUSPEND_CHECK);
                suspend_idle(&h);
            });

            let h = handle.clone();
            thread::spawn(move || loop {
                thread::sleep(SAVE_EVERY);
                persist(&h);
            });

            updater::watch(&handle);



            // `browser url1 url2 …` opens each as a tab.
            // A tab that refuses to open is logged, not fatal: the window stays up
            // and the address bar still works.
            let args: Vec<String> = std::env::args().skip(1).collect();
            if args.is_empty() {
                match restore_session(&handle) {
                    Ok(true) => log::info!("session restored"),
                    Ok(false) => {
                        if let Err(e) = open_tab(&handle, home(&handle)) {
                            log::error!("could not open the first tab: {e}");
                        }
                    }
                    Err(e) => log::error!("could not restore the session: {e}"),
                }
            }
            for arg in args {
                if let Err(e) = open_tab(&handle, parse_input(&handle, &arg)) {
                    log::error!("could not open {arg}: {e}");
                }
            }
            log::info!("startup finished");
            Ok(())
        })
        .build(tauri::generate_context!());

    // A missing WebView2 on Windows dies here, before any log file exists,
    // so the reason goes somewhere findable no matter what.
    match app {
        Ok(app) => app.run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                persist(app);
                updater::install_on_quit(app);
            }
        }),
        Err(e) => {
            let note = format!("Browser could not start: {e}\n");
            eprintln!("{note}");
            let _ = std::fs::write(std::env::temp_dir().join("browser-startup-error.log"), &note);
            std::process::exit(1);
        }
    }
}
