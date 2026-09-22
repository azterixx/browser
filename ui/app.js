// Island behavior ported from Blanc (MIT, github.com/bnfy/blanc).
const invoke = (cmd, args = {}) => window.__TAURI_INTERNALS__.invoke(cmd, args);
const $ = (id) => document.getElementById(id);
const root = document.documentElement;
const isMac = root.dataset.os === 'macos';

const ICONS = {
  back: '<svg viewBox="0 0 16 16"><path d="M9.75 3.5 5.25 8l4.5 4.5"/></svg>',
  forward: '<svg viewBox="0 0 16 16"><path d="M6.25 3.5 10.75 8l-4.5 4.5"/></svg>',
  plus: '<svg viewBox="0 0 16 16"><path d="M8 3v10M3 8h10"/></svg>',
  reload: '<svg viewBox="0 0 16 16"><path d="M12.42 10.35a5 5 0 1 1-4.42-7.35c1.4 0 2.74.56 3.74 1.53L13 5.78"/><path d="M13 3v2.78h-2.78"/></svg>',
  stop: '<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="5"/><rect class="stop-mark" x="6.05" y="6.05" width="3.9" height="3.9" rx="0.8"/></svg>',
  close: '<svg viewBox="0 0 16 16"><path d="M4.75 4.75l6.5 6.5M11.25 4.75l-6.5 6.5"/></svg>',
  collapse: '<svg viewBox="0 0 16 16"><path d="M3.5 9.75 8 5.25l4.5 4.5"/></svg>',
  search: '<svg viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.25"/><path d="m10.25 10.25 3 3"/></svg>',
  go: '<svg viewBox="0 0 16 16"><path d="M3 8h10M8.5 3.5 13 8l-4.5 4.5"/></svg>',
  chevron: '<svg viewBox="0 0 16 16"><path d="M4 6.25 8 10.25l4-4"/></svg>',
  puzzle: '<svg viewBox="0 0 16 16"><path d="M6.5 2.75a1.25 1.25 0 0 1 2.5 0v1h2.25c.41 0 .75.34.75.75V6.75h1a1.25 1.25 0 0 1 0 2.5h-1v2.25c0 .41-.34.75-.75.75H9.25v-1a1.25 1.25 0 0 0-2.5 0v1H4.5a.75.75 0 0 1-.75-.75V9.25h1a1.25 1.25 0 0 0 0-2.5h-1V4.5c0-.41.34-.75.75-.75h2V2.75z"/></svg>',
  kebab: '<svg viewBox="0 0 16 16"><circle cx="8" cy="3.5" r=".9"/><circle cx="8" cy="8" r=".9"/><circle cx="8" cy="12.5" r=".9"/></svg>',
  tune: '<svg viewBox="0 0 16 16"><path d="M2.75 5h6.5M12.25 5h1M2.75 11h1M6.75 11h6.5"/><circle cx="10.75" cy="5" r="1.5"/><circle cx="5.25" cy="11" r="1.5"/></svg>',
  gear: '<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="2.1"/><path d="M8 1.75v1.6M8 12.65v1.6M14.25 8h-1.6M3.35 8h-1.6M12.42 3.58l-1.13 1.13M4.71 11.29l-1.13 1.13M12.42 12.42l-1.13-1.13M4.71 4.71 3.58 3.58"/></svg>',
  download: '<svg viewBox="0 0 16 16"><path d="M8 2.75v7.5M4.75 7 8 10.25 11.25 7M3 13.25h10"/></svg>',
  globe: '<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5"/><path d="M2.5 8h11M8 2.5c1.6 1.6 2.3 3.5 2.3 5.5S9.6 11.9 8 13.5M8 2.5C6.4 4.1 5.7 6 5.7 8s.7 3.9 2.3 5.5"/></svg>',
};
const MAX_DOTS = 7;
const FLOATING = ['float', 'autohide', 'bottom'];
const openSettings = (install) => invoke('open_settings', install ? { install } : {});
const zoomLabel = (tab) => (tab && Math.abs((tab.zoom ?? 1) - 1) > 0.001 ? `${Math.round(tab.zoom * 100)}%` : '');
function showZoom(node) {
  const text = zoomLabel(activeTab());
  node.hidden = !text;
  node.textContent = text;
}
function showDownloads(node) {
  const d = state.downloads || { active: 0, total: 0 };
  node.hidden = !d.total;
  node.classList.toggle('busy', d.active > 0);
  node.title = d.active ? `Downloading ${d.active} file${d.active > 1 ? 's' : ''}` : 'Downloads';
}

const address = $('address');
const list = $('list');
const panel = $('panel');
const pill = $('pill');

let state = { tabs: [], active: null, island: false, mode: 'strip', revealed: false };
let rows = [];
let sel = 0;
let typed = false;       // until the user types, the list shows the open tabs
let hist = [];           // history suggestions for the typed text
let histQuery = '';
let openedByClick = false;

document.querySelectorAll('[data-key]').forEach((k) => { k.textContent = (isMac ? '⌘' : 'Ctrl+') + k.dataset.key; });

// ---------- helpers ----------

const parse = (url) => { try { return new URL(url); } catch { return null; } };
const hostOf = (url) => parse(url)?.host.replace(/^www\./, '') ?? '';
const isBlank = (tab) => !tab || tab.url === 'about:blank';
const activeTab = () => state.tabs.find((t) => t.id === state.active);
function faviconOf(url) {
  const u = parse(url);
  return u && u.protocol.startsWith('http') ? `${u.origin}/favicon.ico` : '';
}
function labelOf(tab) {
  if (tab.title) return tab.title;
  if (isBlank(tab)) return 'New Tab';
  return hostOf(tab.url) || tab.url;
}
function looksLikeUrl(s) {
  return /^[a-z]+:\/\//i.test(s) || (!/\s/.test(s) && (/\.[a-z]{2,}/i.test(s) || s.startsWith('localhost')));
}
function el(tag, cls, html) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (html != null) e.innerHTML = html;
  return e;
}
function iconButton(cls, icon, title, onClick) {
  const b = el('button', cls, ICONS[icon]);
  b.title = title;
  b.addEventListener('mousedown', (e) => e.preventDefault()); // keep focus where it was
  b.addEventListener('click', (e) => { e.stopPropagation(); onClick(); });
  return b;
}
function favicon(url) {
  const box = el('span', 'ico', ICONS.globe);
  const src = faviconOf(url);
  if (src) {
    const img = new Image();
    img.onload = () => box.replaceChildren(img);
    img.src = src;
  }
  return box;
}

// ---------- resting island ----------

function drawPill() {
  const tab = activeTab();

  $('pillNav').replaceChildren(
    iconButton('pill-btn', 'back', 'Back', () => invoke('nav', { action: 'back' })),
    iconButton('pill-btn', 'forward', 'Forward', () => invoke('nav', { action: 'forward' })),
  );

  const dots = $('dots');
  dots.replaceChildren();
  if (state.tabs.length > 1) {
    // Keep the active tab inside the visible window of dots.
    const idx = state.tabs.findIndex((t) => t.id === state.active);
    const start = Math.max(0, Math.min(idx - Math.floor(MAX_DOTS / 2), state.tabs.length - MAX_DOTS));
    state.tabs.slice(start, start + MAX_DOTS).forEach((t) => {
      const d = el('button', 'dot' + (t.id === state.active ? ' active' : '') + (t.loading ? ' loading' : ''));
      d.title = labelOf(t);
      d.addEventListener('click', (e) => { e.stopPropagation(); invoke('activate_tab', { id: t.id }); });
      dots.append(d);
    });
    const hidden = state.tabs.length - Math.min(MAX_DOTS, state.tabs.length);
    if (hidden > 0) dots.append(el('span', 'dots-more', `+${hidden}`));
  }

  const blank = isBlank(tab);
  const domain = $('domain');
  domain.textContent = blank ? 'Search or type a URL' : hostOf(tab.url) || tab.url;
  domain.className = blank ? 'placeholder' : '';
  const fav = $('fav');
  const src = blank ? '' : faviconOf(tab.url);
  if (fav.getAttribute('src') !== src) {
    fav.onerror = () => fav.setAttribute('src', '');
    fav.setAttribute('src', src);
  }

  showZoom($('pzoom'));
  const dl = iconButton('pill-btn', 'download', 'Downloads', () => invoke('open_downloads'));
  showDownloads(dl);
  $('pillActions').replaceChildren(
    dl,
    iconButton('pill-btn', 'plus', 'New tab', () => invoke('new_tab')),
    tab?.loading
      ? iconButton('pill-btn', 'stop', 'Stop', () => invoke('nav', { action: 'stop' }))
      : iconButton('pill-btn', 'reload', 'Reload', () => invoke('nav', { action: 'reload' })),
    iconButton('pill-btn close', 'close', 'Close tab', () => invoke('close')),
  );

  // Faux header: the strip continues the page's own top edge.
  // Unknown color keeps the previous one, so the strip never flashes to the theme default.
  if (tab?.tint) paintStrip(tab.tint);
}

let stripTint = null;
function paintStrip(tint) {
  if (tint === stripTint) return;
  stripTint = tint;
  const [a, b] = document.querySelectorAll('#strip .tint');
  const [next, prev] = a.classList.contains('on') ? [b, a] : [a, b];
  next.style.background = tint;
  next.classList.add('on');
  prev.classList.remove('on');
}

// ---------- expanded island ----------

function drawPanelActions() {
  $('panelActions').replaceChildren(
    iconButton('act-btn', 'back', 'Back', () => invoke('nav', { action: 'back' })),
    iconButton('act-btn', 'forward', 'Forward', () => invoke('nav', { action: 'forward' })),
    iconButton('act-btn', 'reload', 'Reload', () => invoke('nav', { action: 'reload' })),
    iconButton('act-btn', 'collapse', 'Collapse (Esc)', collapse),
  );
}

function drawList() {
  const q = typed ? address.value.trim() : '';
  const needle = q.toLowerCase();
  const tabs = state.tabs.filter((t) => !needle
    || labelOf(t).toLowerCase().includes(needle) || t.url.toLowerCase().includes(needle));

  rows = [];
  const cur = activeTab();
  if (!q && cur && /^https:\/\/chromewebstore\.google\.com\/detail\//.test(cur.url)) {
    rows.push({ kind: 'ext', input: cur.url });
  }
  if (q) rows.push({ kind: 'go', input: q });
  tabs.forEach((tab) => rows.push({ kind: 'tab', tab }));
  if (q) {
    const open = new Set(state.tabs.map((t) => t.url));
    hist.filter((h) => !open.has(h.url)).forEach((h) => rows.push({ kind: 'hist', h }));
  }
  sel = Math.max(0, Math.min(sel, rows.length - 1));

  list.replaceChildren(...rows.map((r, i) => {
    const row = el('div', 'row' + (i === sel ? ' sel' : ''));
    if (r.kind === 'ext') {
      row.append(el('span', 'ico', ICONS.plus));
      const title = el('span', 'title');
      title.textContent = 'Add this extension to Browser';
      const sub = el('span', 'sub');
      sub.textContent = 'chrome web store';
      row.append(title, sub);
    } else if (r.kind === 'go') {
      const url = looksLikeUrl(r.input);
      row.append(el('span', 'ico', url ? ICONS.go : ICONS.search));
      const title = el('span', 'title');
      title.textContent = r.input;
      const sub = el('span', 'sub');
      sub.textContent = url ? 'open' : 'search';
      row.append(title, sub);
    } else if (r.kind === 'hist') {
      const title = el('span', 'title');
      title.textContent = r.h.title || displayUrl(r.h.url);
      const sub = el('span', 'sub');
      sub.textContent = hostOf(r.h.url);
      row.append(favicon(r.h.url), title, sub);
    } else {
      const t = r.tab;
      if (t.id === state.active) row.classList.add('active');
      if (t.suspended) row.classList.add('asleep');
      const title = el('span', 'title');
      title.textContent = labelOf(t);
      row.append(favicon(t.url), title);
      if (q) {
        const sub = el('span', 'sub');
        sub.textContent = t.suspended ? 'asleep' : hostOf(t.url);
        row.append(sub);
      }
      row.append(iconButton('row-close', 'close', 'Close tab', () => invoke('close', { id: t.id })));
      row.addEventListener('auxclick', (e) => { if (e.button === 1) invoke('close', { id: t.id }); });
    }
    row.addEventListener('mousemove', () => { if (sel !== i) { sel = i; drawList(); } });
    row.addEventListener('click', () => choose(i));
    return row;
  }));
  list.querySelector('.sel')?.scrollIntoView({ block: 'nearest' });
}

function choose(i) {
  const r = rows[i];
  if (!r) return;
  if (r.kind === 'ext') { openSettings(r.input); return; }
  if (r.kind === 'tab') invoke('activate_tab', { id: r.tab.id });
  else if (r.kind === 'hist') invoke('navigate', { input: r.h.url });
  else invoke('navigate', { input: r.input });
}

// Answers can come back out of order; only the latest query counts.
async function fetchHistory(q) {
  histQuery = q;
  const found = q ? await invoke('suggest', { query: q }) : [];
  if (histQuery !== q) return;
  hist = found;
  if (state.island) drawList();
}

// The panel grows out of the resting pill: same anchor, clip from pill size to full.
function morph(reverse = false) {
  const p = pill.getBoundingClientRect();
  const f = panel.getBoundingClientRect();
  const dx = Math.max(0, (f.width - p.width) / 2);
  const dy = Math.max(0, f.height - p.height);
  // The bottom island grows upwards.
  const clip = state.mode === 'bottom' ? `${dy}px ${dx}px 0 ${dx}px` : `0 ${dx}px ${dy}px ${dx}px`;
  const from = { clipPath: `inset(${clip} round 17px)`, opacity: 0.6 };
  const to = { clipPath: 'inset(0 0 0 0 round 18px)', opacity: 1 };
  return panel.animate(reverse ? [to, from] : [from, to], {
    duration: reverse ? 140 : 240,
    easing: 'cubic-bezier(.2,.8,.2,1)',
  }).finished;
}

let closing = false;
async function collapse() {
  if (closing || !state.island) return;
  closing = true;
  try { await morph(true); } catch {}
  await invoke('island', { open: false });
  closing = false;
}

// ---------- state from Rust ----------

window.render = (s) => {
  const opening = s.island && !state.island;
  const modeChanged = s.mode !== state.mode;
  state = s;
  root.dataset.mode = s.mode;
  root.classList.toggle('fullscreen', !!s.fullscreen);
  if (modeChanged) { lastSize = ''; requestAnimationFrame(reportSize); }
  root.classList.toggle('revealed', !!s.revealed);
  root.classList.toggle('open', s.island);
  if (opening) {
    // Clicking the pill expands in place; a shortcut summons it over a dimmed page.
    root.classList.toggle('dim', !openedByClick);
    openedByClick = false;
    drawPanelActions();
    sel = 0;
    drawList();
    morph();
  } else if (s.island) {
    drawList();
  }
  drawPill();
  if (s.mode === 'chrome') drawChrome();
  if (s.mode === 'arc') drawSidebar();
};

// ---------- classic layout ----------

function tabIcon(t) {
  return t.loading ? el('span', 'ico', ICONS.reload) : favicon(t.url);
}

// Chrome shows "github.com/tauri-apps", not "https://www.github.com/tauri-apps/".
function displayUrl(url) {
  return url.replace(/^https?:\/\//, '').replace(/^www\./, '').replace(/\/$/, '');
}

function drawChrome() {
  const box = $('ctabs');
  box.replaceChildren(...state.tabs.map((t) => {
    const tab = el('div', 'ctab' + (t.id === state.active ? ' active' : '') + (t.suspended ? ' asleep' : ''));
    tab.title = t.url;
    const title = el('span', 'title');
    title.textContent = labelOf(t);
    tab.append(
      el('span', 'c-hover'),
      tabIcon(t),
      title,
      iconButton('row-close', 'close', 'Close tab', () => invoke('close', { id: t.id })),
      el('span', 'foot l'),
      el('span', 'foot r'),
    );
    tab.addEventListener('mousedown', (e) => {
      if (e.button === 0 && !e.target.closest('button')) invoke('activate_tab', { id: t.id });
    });
    tab.addEventListener('auxclick', (e) => { if (e.button === 1) invoke('close', { id: t.id }); });
    return tab;
  }));
  requestAnimationFrame(() => {
    for (const tab of box.children) tab.classList.toggle('narrow', tab.offsetWidth < 72);
  });
  const caddr = $('caddr');
  if (document.activeElement !== caddr) {
    const tab = activeTab();
    caddr.value = isBlank(tab) ? '' : displayUrl(tab.url);
  }
  $('creload').innerHTML = activeTab()?.loading ? ICONS.close : ICONS.reload;
  showZoom($('czoom'));
  showDownloads($('cdl'));
}

window.focusInline = () => { const c = $('caddr'); c.focus(); c.select(); };

$('caddr').addEventListener('focus', (e) => {
  const tab = activeTab();
  if (!isBlank(tab)) e.target.value = tab.url;
  e.target.select();
});
$('caddr').addEventListener('blur', () => drawChrome());
// Inline autocomplete: "git" becomes "git|hub.com" with the rest selected, like Chrome.
$('caddr').addEventListener('input', async (e) => {
  const c = e.target;
  const v = c.value;
  if (!v || /\s/.test(v) || !e.inputType?.startsWith('insert')) return;
  const found = await invoke('suggest', { query: v });
  if (c.value !== v || document.activeElement !== c) return;
  const low = v.toLowerCase();
  const best = found.map((h) => displayUrl(h.url))
    .filter((u) => u.toLowerCase().startsWith(low))
    .sort((a, b) => a.length - b.length)[0];
  if (!best) return;
  c.value = v + best.slice(v.length);
  c.setSelectionRange(v.length, c.value.length);
});
$('caddr').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { invoke('navigate', { input: e.target.value }); e.target.blur(); }
  else if (e.key === 'Escape') { e.target.blur(); drawChrome(); }
});

// ---------- sidebar layout ----------

function drawSidebar() {
  const tab = activeTab();
  const saddr = $('saddr');
  const blank = isBlank(tab);
  saddr.textContent = blank ? 'Search or enter address' : hostOf(tab.url) || tab.url;
  saddr.classList.toggle('placeholder', blank);
  $('sreload').innerHTML = tab?.loading ? ICONS.stop : ICONS.reload;
  showDownloads($('sdl'));

  $('stabs').replaceChildren(...state.tabs.map((t) => {
    const row = el('div', 's-row' + (t.id === state.active ? ' active' : '') + (t.suspended ? ' asleep' : ''));
    row.title = t.url;
    const title = el('span', 'title');
    title.textContent = labelOf(t);
    row.append(tabIcon(t), title, iconButton('row-close', 'close', 'Close tab', () => invoke('close', { id: t.id })));
    row.addEventListener('click', () => invoke('activate_tab', { id: t.id }));
    row.addEventListener('auxclick', (e) => { if (e.button === 1) invoke('close', { id: t.id }); });
    return row;
  }));
}

// ---------- shared buttons of the classic and sidebar layouts ----------

const navAction = (action) => () => {
  if (action === 'reload' && activeTab()?.loading) action = 'stop';
  invoke('nav', { action });
};
[['cback', 'back'], ['sback', 'back'], ['cforward', 'forward'], ['sforward', 'forward']].forEach(([id, icon]) => {
  $(id).innerHTML = ICONS[icon];
  $(id).addEventListener('click', navAction(icon));
});
['creload', 'sreload'].forEach((id) => {
  $(id).innerHTML = ICONS.reload;
  $(id).addEventListener('click', navAction('reload'));
});
$('cadd').innerHTML = ICONS.plus;
$('csearch').innerHTML = ICONS.chevron;
$('csearch').addEventListener('click', () => { openedByClick = false; invoke('island', { open: true }); });
$('cinfo').innerHTML = ICONS.tune;
$('cadd').addEventListener('click', () => invoke('new_tab'));
$('snew').querySelector('.ico').innerHTML = ICONS.plus;
$('snew').addEventListener('click', () => invoke('new_tab'));
$('saddr').addEventListener('click', () => { openedByClick = false; invoke('island', { open: true }); });
$('cgear').innerHTML = ICONS.kebab;
$('cgear').addEventListener('click', () => openSettings());
$('cext').innerHTML = ICONS.puzzle;
$('cext').addEventListener('click', () => openSettings());
$('sgear').innerHTML = ICONS.gear;
$('sgear').addEventListener('click', () => openSettings());
['cdl', 'sdl'].forEach((id) => {
  $(id).innerHTML = ICONS.download;
  $(id).addEventListener('click', () => invoke('open_downloads'));
});
['pzoom', 'czoom'].forEach((id) => {
  $(id).addEventListener('mousedown', (e) => e.preventDefault());
  $(id).addEventListener('click', (e) => { e.stopPropagation(); invoke('nav', { action: 'zoom_reset' }); });
});

window.focusAddress = () => {
  const tab = activeTab();
  address.value = isBlank(tab) ? '' : tab.url;
  typed = false;
  sel = 0;
  hist = [];
  histQuery = '';
  address.focus();
  address.select();
  if (state.island) drawList();
};

// ---------- input ----------

// A click opens the island; press and drag moves the window instead.
let press = null;
let dragged = false;
pill.addEventListener('mousedown', (e) => {
  if (e.button !== 0 || e.target.closest('button')) return;
  press = { x: e.screenX, y: e.screenY };
  dragged = false;
});
window.addEventListener('mousemove', (e) => {
  if (!press || !(e.buttons & 1)) { press = null; return; }
  if (Math.hypot(e.screenX - press.x, e.screenY - press.y) > 4) {
    press = null;
    dragged = true;
    invoke('plugin:window|start_dragging', { label: 'main' });
  }
});
window.addEventListener('mouseup', () => { press = null; });
pill.addEventListener('click', () => {
  if (dragged) { dragged = false; return; }
  openedByClick = true;
  invoke('island', { open: true });
});

// Float / auto-hide: the webview is cut to the island's box, so tell Rust how big it is.
const SHADOW = 18;
let lastSize = '';
function reportSize() {
  if (!FLOATING.includes(state.mode)) return;
  const w = pill.offsetWidth + SHADOW * 2;
  const h = 6 + pill.offsetHeight + SHADOW;
  const key = `${w}x${h}`;
  if (key !== lastSize) { lastSize = key; invoke('pill_size', { width: w, height: h }); }
}
new ResizeObserver(reportSize).observe(pill);

// Auto-hide: reveal at the top edge, hide again shortly after the cursor leaves the island.
let hideTimer = 0;
$('edge').addEventListener('mouseenter', () => invoke('reveal', { on: true }));
root.addEventListener('mouseenter', () => clearTimeout(hideTimer));
root.addEventListener('mouseleave', () => {
  if (state.mode !== 'autohide' || state.island) return;
  clearTimeout(hideTimer);
  hideTimer = setTimeout(() => invoke('reveal', { on: false }), 700);
});

// ---------- settings ----------

$('gear').innerHTML = ICONS.gear;
$('gear').addEventListener('click', () => openSettings());

// Rust calls this when the background check finds a new version.
window.updateReady = (u) => {
  document.querySelectorAll('#gear, #cgear, #sgear').forEach((b) => {
    b.classList.add('has-update');
    b.title = `Update to ${u.version}`;
  });
};

$('backdrop').addEventListener('mousedown', collapse);
$('newtab').addEventListener('click', () => invoke('new_tab'));

address.addEventListener('input', () => { typed = true; sel = 0; drawList(); fetchHistory(address.value.trim()); });
address.addEventListener('keydown', (e) => {
  if (e.key === 'ArrowDown') { sel = Math.min(sel + 1, rows.length - 1); drawList(); e.preventDefault(); }
  else if (e.key === 'ArrowUp') { sel = Math.max(sel - 1, 0); drawList(); e.preventDefault(); }
  else if (e.key === 'Enter') {
    if (typed) choose(sel);
    else if (address.value) invoke('navigate', { input: address.value });
    else collapse();
  }
});

// Shortcuts while the island has focus. On macOS the native menu covers the page too.
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && state.island) {
    collapse();
    return;
  }
  if (!(e.metaKey || e.ctrlKey)) return;
  const k = e.key.toLowerCase();
  if (k === 't' && e.shiftKey) invoke('reopen_tab');
  else if (k === 't') invoke('new_tab');
  else if (k === 'f') { collapse(); invoke('nav', { action: 'find' }); }
  else if (k === '=' || k === '+') invoke('nav', { action: 'zoom_in' });
  else if (k === '-') invoke('nav', { action: 'zoom_out' });
  else if (k === '0') invoke('nav', { action: 'zoom_reset' });
  else if (k === 'j' && e.shiftKey) invoke('open_downloads');
  else if (k === 'g') invoke('nav', { action: e.shiftKey ? 'find_prev' : 'find_next' });
  else if (k === 'w') invoke('close');
  else if (k === 'l') {
    if (state.mode === 'chrome') window.focusInline();
    else if (state.island) window.focusAddress();
    else invoke('island', { open: true });
  }
  else if (k === 'r') invoke('nav', { action: 'reload' });
  else if (k === ',') openSettings();
  else return;
  e.preventDefault();
});

invoke('get_state').then(window.render);
