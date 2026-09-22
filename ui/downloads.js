// browser://downloads. Rust calls window.refresh() whenever a download starts or ends.
const invoke = (cmd, args = {}) => window.__TAURI_INTERNALS__.invoke(cmd, args);
const $ = (id) => document.getElementById(id);

const ICONS = {
  download: '<svg viewBox="0 0 24 24"><path d="M12 4v11M7 10.5l5 5 5-5M5 20h14"/></svg>',
  file: '<svg viewBox="0 0 24 24"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/></svg>',
  failed: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.5"/><path d="M12 8v5M12 16h.01"/></svg>',
  search: '<svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.35-4.35"/></svg>',
};

let items = [];

function el(tag, cls, html) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (html != null) e.innerHTML = html;
  return e;
}

const fileName = (path) => path.split(/[\\/]/).pop() || path;
const hostOf = (url) => { try { return new URL(url).host.replace(/^www\./, ''); } catch { return ''; } };
const when = (secs) => new Date(secs * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });

function draw() {
  const needle = $('search').value.trim().toLowerCase();
  const shown = items.filter((d) => !needle || (d.path + ' ' + d.url).toLowerCase().includes(needle));
  const list = $('list');
  if (!shown.length) {
    list.replaceChildren(el('div', 'empty', needle ? 'No downloads found' : 'Files you download appear here'));
    return;
  }
  list.replaceChildren(...shown.map((d) => {
    const gone = d.state === 'done' && !d.exists;
    const row = el('div', 'dl ' + d.state + (gone ? ' gone' : ''));
    row.append(el('span', 'ico', d.state === 'failed' ? ICONS.failed : d.state === 'active' ? ICONS.download : ICONS.file));
    const info = el('div', 'info');
    const name = el(d.state === 'done' && !gone ? 'button' : 'span', 'name');
    name.textContent = fileName(d.path);
    if (name.tagName === 'BUTTON') name.addEventListener('click', () => invoke('download_open', { id: d.id, reveal: false }));
    const sub = el('small');
    const status = { active: 'Downloading…', failed: 'Failed', done: gone ? 'Deleted' : '' }[d.state];
    sub.textContent = [status, hostOf(d.url), when(d.started)].filter(Boolean).join(' · ');
    sub.title = d.url;
    info.append(name, sub);
    row.append(info);
    if (d.state === 'done' && !gone) {
      const show = el('button', 'btn', 'Show in Finder');
      show.addEventListener('click', () => invoke('download_open', { id: d.id, reveal: true }));
      row.append(show);
    } else if (d.state === 'failed') {
      const retry = el('button', 'btn', 'Retry');
      // A file response never replaces this page; the download just starts again.
      retry.addEventListener('click', () => location.assign(d.url));
      row.append(retry);
    }
    return row;
  }));
}

window.refresh = async () => {
  items = await invoke('downloads_list');
  draw();
};

document.querySelector('.brand-icon').innerHTML = ICONS.download;
document.querySelector('.search-icon').innerHTML = ICONS.search;
$('search').addEventListener('input', draw);
$('clear').addEventListener('click', () => invoke('downloads_clear'));
// The file may be deleted in Finder while this page is open.
window.addEventListener('focus', window.refresh);
window.refresh();
