// browser://settings. A local page, so it may talk to Rust; websites in tabs can't.
const invoke = (cmd, args = {}) => window.__TAURI_INTERNALS__.invoke(cmd, args);
const $ = (id) => document.getElementById(id);

const ICONS = {
  gear: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1.08-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1.08 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>',
  search: '<svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.35-4.35"/></svg>',
  appearance: '<svg viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" rx="3"/><path d="M3 9h18"/></svg>',
  'search-engine': '<svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.35-4.35"/></svg>',
  startup: '<svg viewBox="0 0 24 24"><path d="M12 3v9M18.36 6.64a9 9 0 1 1-12.73 0"/></svg>',
  performance: '<svg viewBox="0 0 24 24"><path d="M12 13 16 9"/><path d="M3.5 17a9 9 0 1 1 17 0"/></svg>',
  extensions: '<svg viewBox="0 0 24 24"><path d="M10 4.5a2 2 0 0 1 4 0V6h3a1 1 0 0 1 1 1v3h1.5a2 2 0 0 1 0 4H18v3a1 1 0 0 1-1 1h-3v-1.5a2 2 0 0 0-4 0V18H7a1 1 0 0 1-1-1v-3h1.5a2 2 0 0 0 0-4H6V7a1 1 0 0 1 1-1h3z"/></svg>',
  privacy: '<svg viewBox="0 0 24 24"><path d="M12 3 5 6v6c0 4.5 3 7.5 7 9 4-1.5 7-4.5 7-9V6z"/></svg>',
  about: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/></svg>',
  updates: '<svg viewBox="0 0 24 24"><path d="M12 4v10"/><path d="m8 10 4 4 4-4"/><path d="M4 18h16"/></svg>',
  remove: '<svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18"/></svg>',
};

const MODES = [
  { id: 'strip', name: 'Island', text: 'A bar above the page, tinted with the site color.' },
  { id: 'float', name: 'Floating', text: 'The page fills the window. The island floats over it.' },
  { id: 'autohide', name: 'Auto-hide', text: 'Hidden until you touch the top edge.' },
  { id: 'chrome', name: 'Classic', text: 'A tab row and an address bar, like Chrome.' },
  { id: 'arc', name: 'Sidebar', text: 'Vertical tabs on the left, like Arc.' },
  { id: 'bottom', name: 'Bottom', text: 'The island floats at the bottom.' },
];

let settings = null;

function el(tag, cls, html) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (html != null) e.innerHTML = html;
  return e;
}

function save() {
  invoke('set_settings', { settings });
}

// ---------- nav ----------

const sections = [...document.querySelectorAll('main section')];

function drawNav() {
  $('nav').replaceChildren(...sections.map((s) => {
    const b = el('button', 'nav-item', ICONS[s.id] || '');
    b.dataset.target = s.id;
    b.append(s.dataset.title);
    b.addEventListener('click', () => {
      $('search').value = '';
      filter('');
      s.scrollIntoView({ block: 'start' });
      markNav(s.id);
    });
    return b;
  }));
}

function markNav(id) {
  document.querySelectorAll('.nav-item').forEach((b) => b.classList.toggle('on', b.dataset.target === id));
}

$('main').addEventListener('scroll', () => {
  const top = $('main').getBoundingClientRect().top;
  const current = sections.filter((s) => !s.hidden).find((s) => s.getBoundingClientRect().bottom > top + 40);
  if (current) markNav(current.id);
});

// ---------- search ----------

function filter(q) {
  const needle = q.trim().toLowerCase();
  let any = false;
  sections.forEach((s) => {
    let visible = 0;
    s.querySelectorAll('.row').forEach((row) => {
      const text = (row.textContent + ' ' + (row.dataset.keywords || '') + ' ' + s.dataset.title).toLowerCase();
      const hit = !needle || text.includes(needle);
      row.hidden = !hit;
      if (hit) visible++;
    });
    const extList = s.querySelector('#extList');
    if (extList) extList.hidden = !!needle && visible === 0;
    s.hidden = visible === 0;
    any ||= visible > 0;
  });
  $('nothing').hidden = any;
}

$('search').addEventListener('input', (e) => filter(e.target.value));

// ---------- appearance ----------

function drawModes() {
  $('modes').replaceChildren(...MODES.map((m) => {
    const b = el('button', 'mode' + (settings.mode === m.id ? ' on' : ''));
    const name = el('b');
    name.textContent = m.name;
    const text = el('small');
    text.textContent = m.text;
    b.append(el('span', 'preview ' + m.id, '<i></i>'), name, text);
    b.addEventListener('click', () => { settings.mode = m.id; save(); drawModes(); });
    return b;
  }));
}

// ---------- search engine, new tab ----------

$('engine').addEventListener('change', (e) => { settings.search = e.target.value; save(); });

function drawHome() {
  const custom = settings.home.trim() !== '';
  document.querySelector(`input[name=home][value=${custom ? 'custom' : 'engine'}]`).checked = true;
  $('homeUrl').value = settings.home;
  $('homeUrl').disabled = !custom;
}
document.querySelectorAll('input[name=home]').forEach((r) => r.addEventListener('change', () => {
  const custom = document.querySelector('input[name=home]:checked').value === 'custom';
  $('homeUrl').disabled = !custom;
  if (custom) $('homeUrl').focus();
  else { settings.home = ''; save(); }
}));
$('homeUrl').addEventListener('change', (e) => { settings.home = e.target.value.trim(); save(); });

// ---------- performance ----------

function drawSaver() {
  const on = settings.sleep_minutes > 0;
  $('saver').setAttribute('aria-checked', String(on));
  $('sleepRow').style.opacity = on ? '1' : '.45';
  $('sleep').disabled = !on;
  if (on) $('sleep').value = String(settings.sleep_minutes);
}
$('saver').addEventListener('click', () => {
  settings.sleep_minutes = settings.sleep_minutes > 0 ? 0 : Number($('sleep').value || 10);
  save();
  drawSaver();
});
$('sleep').addEventListener('change', (e) => { settings.sleep_minutes = Number(e.target.value); save(); });

// ---------- on startup, history ----------

function drawSwitches() {
  $('restore').setAttribute('aria-checked', String(settings.restore_session));
  $('history').setAttribute('aria-checked', String(settings.save_history));
}
$('restore').addEventListener('click', () => { settings.restore_session = !settings.restore_session; save(); drawSwitches(); });
$('history').addEventListener('click', () => { settings.save_history = !settings.save_history; save(); drawSwitches(); });

// ---------- extensions ----------

async function loadExtensions() {
  const exts = await invoke('ext_list');
  const box = $('extList');
  if (!exts.length) {
    box.replaceChildren(el('div', 'ext-empty', 'No extensions installed'));
    return;
  }
  box.replaceChildren(...exts.map((x) => {
    const row = el('div', 'ext-row');
    const name = el('span', 'name');
    name.textContent = x.name + ' ';
    const ver = el('small');
    ver.textContent = x.version;
    name.append(ver);
    const rm = el('button', 'btn', 'Remove');
    rm.addEventListener('click', async () => { await invoke('ext_remove', { id: x.id }); loadExtensions(); });
    row.append(name, rm);
    return row;
  }));
}

async function install(input) {
  const status = $('extStatus');
  status.className = 'status';
  status.textContent = 'Installing…';
  $('extAdd').disabled = true;
  try {
    const x = await invoke('ext_install', { input });
    status.textContent = `Added ${x.name}. Reload open pages to use it.`;
    $('extInput').value = '';
    loadExtensions();
  } catch (e) {
    status.className = 'status error';
    status.textContent = String(e);
  } finally {
    $('extAdd').disabled = false;
  }
}

$('extAdd').addEventListener('click', () => install($('extInput').value));
$('extInput').addEventListener('keydown', (e) => { if (e.key === 'Enter') install($('extInput').value); });
$('store').addEventListener('click', () => invoke('new_tab', { input: 'https://chromewebstore.google.com/' }));

// The island's "Add this extension" hands the link over through Rust.
window.checkPending = async () => {
  const link = await invoke('take_pending_install');
  if (!link) return;
  $('extensions').scrollIntoView({ block: 'start' });
  markNav('extensions');
  install(link);
};

// ---------- privacy ----------

// Two clicks instead of confirm(): dialogs don't show in this webview.
function dangerButton(id, label, cmd) {
  const b = $(id);
  let armTimer = 0;
  b.addEventListener('click', async () => {
    if (!b.classList.contains('armed')) {
      b.classList.add('armed');
      b.textContent = 'Click again to delete';
      armTimer = setTimeout(() => { b.classList.remove('armed'); b.textContent = label; }, 4000);
      return;
    }
    clearTimeout(armTimer);
    b.classList.remove('armed');
    b.disabled = true;
    b.textContent = 'Deleting…';
    try {
      await invoke(cmd);
      b.textContent = 'Deleted';
    } catch (e) {
      b.textContent = 'Failed';
    }
    setTimeout(() => { b.disabled = false; b.textContent = label; }, 2500);
  });
}
dangerButton('clear', 'Delete data', 'clear_data');
dangerButton('clearHistory', 'Clear history', 'clear_history');

// ---------- updates ----------

let found = null;

async function checkUpdates(manual) {
  const btn = $('up-btn');
  const status = $('up-status');
  btn.disabled = true;
  status.textContent = 'Checking for updates…';
  try {
    found = await invoke('update_check');
  } catch (e) {
    status.textContent = `Could not check: ${e}`;
    btn.textContent = 'Try again';
    btn.disabled = false;
    return;
  }
  btn.disabled = false;
  if (!found) {
    status.textContent = manual ? 'You are on the latest version.' : 'Checks for a new version on its own, a few times a day.';
    btn.textContent = 'Check for updates';
    return;
  }
  status.textContent = `Version ${found.version} is ready${found.notes ? `: ${found.notes.split('\n')[0]}` : ''}`;
  btn.textContent = `Update to ${found.version}`;
}

async function installUpdate() {
  const btn = $('up-btn');
  const status = $('up-status');
  btn.disabled = true;
  btn.textContent = 'Updating…';
  status.textContent = 'Downloading. Browser restarts when it is done.';
  try {
    await invoke('update_install');
  } catch (e) {
    status.textContent = `Update failed: ${e}`;
    btn.textContent = 'Try again';
    btn.disabled = false;
  }
}

$('up-btn').addEventListener('click', () => (found ? installUpdate() : checkUpdates(true)));

$('logs').addEventListener('click', async () => {
  const b = $('logs');
  try {
    await invoke('open_logs');
  } catch (e) {
    $('log-path').textContent = `Could not open the folder: ${e}`;
    b.textContent = 'Failed';
    setTimeout(() => { b.textContent = 'Show logs'; }, 2500);
  }
});

// ---------- init ----------

document.querySelector('.brand-icon').innerHTML = ICONS.gear;
document.querySelector('.search-icon').innerHTML = ICONS.search;
drawNav();
markNav('appearance');

invoke('get_settings').then((r) => {
  settings = r.settings;
  $('version').textContent = `Version ${r.version}`;
  $('engine-name').textContent = r.os === 'macos' ? 'WebKit (the engine of Safari)' : r.os === 'windows' ? 'WebView2 (Chromium)' : 'WebKitGTK';
  $('engine').value = settings.search;
  drawModes();
  drawHome();
  drawSaver();
  drawSwitches();
  loadExtensions();
  window.checkPending();
});

checkUpdates(false);
