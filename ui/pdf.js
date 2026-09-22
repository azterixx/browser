// browser://pdf: pdf.js with Chrome's PDF viewer controls. Rust fetches the file (the page can't: CORS).
import * as pdfjsLib from './pdfjs/pdf.min.mjs';

// pdf_viewer.mjs reads pdf.js from this global when it loads, so it must be set first.
globalThis.pdfjsLib = pdfjsLib;
const { EventBus, PDFViewer, PDFLinkService, PDFFindController, FindState, ScrollMode, SpreadMode } =
  await import('./pdfjs/pdf_viewer.mjs');

const invoke = (cmd, args = {}, options) => window.__TAURI_INTERNALS__.invoke(cmd, args, options);
const $ = (id) => document.getElementById(id);
const abs = (path) => new URL(path, location.href).href;
const { AnnotationEditorType } = pdfjsLib;

const svg = (body, box = 24) => `<svg viewBox="0 0 ${box} ${box}">${body}</svg>`;
const ICONS = {
  menu: svg('<path d="M4 6.5h16M4 12h16M4 17.5h16"/>'),
  minus: svg('<path d="M6 12h12"/>'),
  plus: svg('<path d="M12 6v12M6 12h12"/>'),
  fitPage: svg('<rect x="6" y="3.5" width="12" height="17" rx="1.5"/><path d="M12 7.5v9M10 9.5l2-2 2 2M10 14.5l2 2 2-2"/>'),
  fitWidth: svg('<rect x="3.5" y="6" width="17" height="12" rx="1.5"/><path d="M7.5 12h9M9.5 10l-2 2 2 2M14.5 10l2 2-2 2"/>'),
  rotate: svg('<path d="M5.2 13.5a7 7 0 1 0 1.9-6.4L4.5 9.5"/><path d="M4.5 5v4.5H9"/><circle cx="12" cy="12" r="1.2"/>'),
  annotate: svg('<path d="M4 19c1.8-2.6 3.4-3 4.5-1.2S11.4 19 12.5 17"/><path d="M14.3 5.2l3.5 3.5-6.3 6.3H8v-3.5z"/>'),
  undo: svg('<path d="M9 14 4.5 9.5 9 5"/><path d="M4.5 9.5H14a5.5 5.5 0 0 1 0 11h-2"/>'),
  redo: svg('<path d="m15 14 4.5-4.5L15 5"/><path d="M19.5 9.5H10a5.5 5.5 0 0 0 0 11h2"/>'),
  download: svg('<path d="M12 4.5v10M7.5 10.5 12 15l4.5-4.5M5 19.5h14"/>'),
  print: svg('<path d="M7 8.5V4.5h10v4M7 16.5H5.5a1 1 0 0 1-1-1V11a2.5 2.5 0 0 1 2.5-2.5h10a2.5 2.5 0 0 1 2.5 2.5v4.5a1 1 0 0 1-1 1H17"/><rect x="7" y="13.5" width="10" height="6.5" rx=".5"/>'),
  more: svg('<circle cx="12" cy="5.5" r="1.6" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none"/><circle cx="12" cy="18.5" r="1.6" fill="currentColor" stroke="none"/>'),
  check: svg('<path d="M5 12.5l4.5 4.5L19 7.5"/>'),
  thumbs: svg('<rect x="5" y="3.5" width="14" height="7" rx="1"/><rect x="5" y="13.5" width="14" height="7" rx="1"/>'),
  outline: svg('<path d="M9 6.5h11M9 12h11M9 17.5h11M4.5 6.5h.01M4.5 12h.01M4.5 17.5h.01"/>'),
  up: svg('<path d="M3.5 9.75 8 5.25l4.5 4.5"/>', 16),
  down: svg('<path d="M3.5 6.25 8 10.75l4.5-4.5"/>', 16),
  close: svg('<path d="M4.75 4.75l6.5 6.5M11.25 4.75l-6.5 6.5"/>', 16),
  chevron: svg('<path d="M6 4l4 4-4 4"/>', 16),
};
const set = (id, icon) => { $(id).innerHTML = ICONS[icon]; };
[['menu', 'menu'], ['zoomOut', 'minus'], ['zoomIn', 'plus'], ['fit', 'fitPage'], ['rotate', 'rotate'],
 ['annotate', 'annotate'], ['undo', 'undo'], ['redo', 'redo'], ['download', 'download'], ['print', 'print'],
 ['more', 'more'], ['showThumbs', 'thumbs'], ['showOutline', 'outline'],
 ['findPrev', 'up'], ['findNext', 'down'], ['findClose', 'close']].forEach(([id, icon]) => set(id, icon));
document.querySelectorAll('.check').forEach((c) => { c.innerHTML = ICONS.check; });
// Buttons must not steal focus from the page or the find field.
document.querySelectorAll('button').forEach((b) => b.addEventListener('mousedown', (e) => e.preventDefault()));

const file = new URLSearchParams(location.search).get('file') || '';
const fileName = (() => {
  try {
    const name = decodeURIComponent(new URL(file).pathname.split('/').pop() || '');
    return name || 'document.pdf';
  } catch { return 'document.pdf'; }
})();
const saveName = /\.pdf$/i.test(fileName) ? fileName : fileName + '.pdf';
document.title = fileName;
$('title').textContent = fileName.replace(/\.pdf$/i, '');

pdfjsLib.GlobalWorkerOptions.workerSrc = abs('pdfjs/pdf.worker.min.mjs');

const container = $('viewerContainer');
const eventBus = new EventBus();
const linkService = new PDFLinkService({ eventBus });
const findController = new PDFFindController({ eventBus, linkService });
const viewer = new PDFViewer({
  container,
  viewer: $('viewer'),
  eventBus,
  linkService,
  findController,
  removePageBorders: true,
  imageResourcesPath: abs('pdfjs/images/'),
  annotationEditorMode: AnnotationEditorType.NONE,
  supportsPinchToZoom: false,
});
linkService.setViewer(viewer);

let doc = null;
let original = null; // the bytes as fetched; pdf.js takes its own copy away to the worker

// ---------- loading ----------

function fail(message) {
  $('error').hidden = false;
  $('errorText').textContent = message;
  $('toolbar').querySelectorAll('button, input').forEach((b) => { if (b.id !== 'menu') b.disabled = true; });
}
$('native').addEventListener('click', () => location.replace(file.split('#')[0] + '#bnative'));

async function load() {
  let data;
  try {
    data = new Uint8Array(await invoke('pdf_fetch', { url: file }));
  } catch (e) {
    fail(String(e));
    return;
  }
  original = data.slice();
  const task = pdfjsLib.getDocument({
    data,
    cMapUrl: abs('pdfjs/cmaps/'),
    cMapPacked: true,
    standardFontDataUrl: abs('pdfjs/standard_fonts/'),
    wasmUrl: abs('pdfjs/wasm/'),
    iccUrl: abs('pdfjs/iccs/'),
    isEvalSupported: false,
    enableXfa: true,
  });
  task.onPassword = askPassword;
  try {
    doc = await task.promise;
  } catch (e) {
    fail(e?.message || String(e));
    return;
  }
  viewer.setDocument(doc);
  linkService.setDocument(doc);
  $('pageCount').textContent = doc.numPages;
  const { info } = await doc.getMetadata().catch(() => ({ info: {} }));
  if (info?.Title && info.Title.trim()) {
    document.title = info.Title.trim();
    $('title').textContent = info.Title.trim();
  }
  buildThumbs();
  buildOutline();
}

// Chrome asks in the page itself; so do we (prompt() doesn't work in this webview).
function askPassword(update, reason) {
  const back = document.createElement('div');
  back.className = 'dialog-back';
  const box = document.createElement('div');
  box.className = 'dialog';
  const h = document.createElement('h3');
  h.textContent = reason === pdfjsLib.PasswordResponses.INCORRECT_PASSWORD ? 'Incorrect password' : 'This document is password protected';
  const input = document.createElement('input');
  input.type = 'password';
  input.className = 'field';
  input.style.cssText = 'width:100%;height:32px;text-align:left;padding:0 8px';
  input.placeholder = 'Enter password';
  const actions = document.createElement('div');
  actions.className = 'dialog-actions';
  const ok = document.createElement('button');
  ok.className = 'text-btn';
  ok.textContent = 'Submit';
  const submit = () => { back.remove(); update(input.value); };
  ok.addEventListener('click', submit);
  input.addEventListener('keydown', (e) => { if (e.key === 'Enter') submit(); });
  actions.append(ok);
  box.append(h, input, actions);
  back.append(box);
  document.body.append(back);
  input.focus();
}

// ---------- page number ----------

eventBus.on('pagesinit', () => {
  viewer.currentScaleValue = 'auto';
  $('pageInput').value = viewer.currentPageNumber;
});
eventBus.on('pagechanging', ({ pageNumber }) => {
  if (document.activeElement !== $('pageInput')) $('pageInput').value = pageNumber;
  markThumb(pageNumber);
});
$('pageInput').addEventListener('keydown', (e) => {
  if (e.key !== 'Enter') return;
  const n = parseInt(e.target.value, 10);
  if (n >= 1 && n <= viewer.pagesCount) viewer.currentPageNumber = n;
  e.target.value = viewer.currentPageNumber;
  container.focus();
});
$('pageInput').addEventListener('focus', (e) => e.target.select());
$('pageInput').addEventListener('blur', (e) => { e.target.value = viewer.currentPageNumber; });

// ---------- zoom ----------

// Chrome's zoom levels.
const STEPS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];
const clampScale = (s) => Math.min(5, Math.max(0.25, s));
let fitMode = 'width';

function showScale() {
  if (document.activeElement !== $('zoomInput')) $('zoomInput').value = Math.round(viewer.currentScale * 100) + '%';
  $('zoomOut').disabled = viewer.currentScale <= STEPS[0] + 0.001;
  $('zoomIn').disabled = viewer.currentScale >= STEPS.at(-1) - 0.001;
}
eventBus.on('scalechanging', showScale);

function zoomStep(dir) {
  if (!doc) return;
  const cur = viewer.currentScale;
  const next = dir > 0
    ? STEPS.find((s) => s > cur + 0.001) ?? cur
    : [...STEPS].reverse().find((s) => s < cur - 0.001) ?? cur;
  viewer.currentScale = next;
}
$('zoomIn').addEventListener('click', () => zoomStep(1));
$('zoomOut').addEventListener('click', () => zoomStep(-1));
$('zoomInput').addEventListener('focus', (e) => e.target.select());
$('zoomInput').addEventListener('blur', showScale);
$('zoomInput').addEventListener('keydown', (e) => {
  if (e.key !== 'Enter') return;
  const n = parseFloat(e.target.value);
  if (n > 0) viewer.currentScale = clampScale(n / 100);
  e.target.blur();
  container.focus();
});

// ⌘+ / ⌘− / ⌘0 come from the app menu through Rust.
window.__pdfZoom = (dir) => {
  if (dir === 0) {
    viewer.currentScaleValue = 'auto';
    setFit('width');
  } else zoomStep(dir);
};

function setFit(mode) {
  fitMode = mode;
  // The button offers the other mode, like Chrome's.
  $('fit').innerHTML = mode === 'width' ? ICONS.fitPage : ICONS.fitWidth;
  $('fit').title = mode === 'width' ? 'Fit to page' : 'Fit to width';
}
setFit('width');
$('fit').addEventListener('click', () => {
  const next = fitMode === 'width' ? 'page' : 'width';
  viewer.currentScaleValue = next === 'page' ? 'page-fit' : 'page-width';
  setFit(next);
});

// Trackpad pinch (WebKit gesture events) and Ctrl+wheel zoom the document around the cursor,
// instead of magnifying the whole page with the toolbar.
let gestureScale = 1;
const pinch = (factor, e) => {
  if (!doc) return;
  viewer.updateScale({ drawingDelay: 400, scaleFactor: factor, origin: [e.clientX, e.clientY] });
};
document.addEventListener('gesturestart', (e) => { e.preventDefault(); gestureScale = 1; }, { passive: false });
document.addEventListener('gesturechange', (e) => {
  e.preventDefault();
  pinch(e.scale / gestureScale, e);
  gestureScale = e.scale;
}, { passive: false });
document.addEventListener('gestureend', (e) => e.preventDefault(), { passive: false });
container.addEventListener('wheel', (e) => {
  if (!e.ctrlKey && !e.metaKey) return;
  e.preventDefault();
  pinch(Math.exp(-e.deltaY / 100), e);
}, { passive: false });

// ---------- rotate ----------

$('rotate').addEventListener('click', () => {
  if (!doc) return;
  viewer.pagesRotation = (viewer.pagesRotation + 270) % 360;
  redrawThumbs();
});

// ---------- annotate ----------

let drawing = false;
$('annotate').addEventListener('click', () => {
  if (!doc) return;
  drawing = !drawing;
  viewer.annotationEditorMode = { mode: drawing ? AnnotationEditorType.INK : AnnotationEditorType.NONE };
  $('annotate').classList.toggle('on', drawing);
});
eventBus.on('annotationeditorstateschanged', ({ details }) => {
  if ('hasSomethingToUndo' in details) $('undo').disabled = !details.hasSomethingToUndo;
  if ('hasSomethingToRedo' in details) $('redo').disabled = !details.hasSomethingToRedo;
});
$('undo').addEventListener('click', () => eventBus.dispatch('editingaction', { source: null, name: 'undo' }));
$('redo').addEventListener('click', () => eventBus.dispatch('editingaction', { source: null, name: 'redo' }));

// ---------- download, print ----------

async function download() {
  if (!doc) return;
  // With drawings, save a new PDF that contains them; otherwise the untouched original.
  const bytes = doc.annotationStorage.size > 0 ? await doc.saveDocument() : original;
  await invoke('pdf_save', bytes, {
    headers: { 'x-name': encodeURIComponent(saveName), 'x-url': encodeURIComponent(file) },
  });
}
$('download').addEventListener('click', download);

// The viewer only draws the pages on screen, so printing first renders every page to an image.
let printing = false;
async function print() {
  if (!doc || printing) return;
  printing = true;
  $('print').disabled = true;
  const box = $('printContainer');
  box.replaceChildren();
  try {
    for (let i = 1; i <= doc.numPages; i++) {
      const page = await doc.getPage(i);
      const viewport = page.getViewport({ scale: 150 / 72, rotation: (page.rotate + viewer.pagesRotation) % 360 });
      const canvas = document.createElement('canvas');
      canvas.width = Math.floor(viewport.width);
      canvas.height = Math.floor(viewport.height);
      const ctx = canvas.getContext('2d');
      ctx.fillStyle = '#fff';
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      await page.render({
        canvasContext: ctx, viewport, intent: 'print',
        annotationMode: pdfjsLib.AnnotationMode.ENABLE_STORAGE,
        printAnnotationStorage: doc.annotationStorage.print,
      }).promise;
      const blob = await new Promise((r) => canvas.toBlob(r));
      const img = new Image();
      img.src = URL.createObjectURL(blob);
      await img.decode();
      box.append(img);
    }
    await invoke('print_page');
  } finally {
    printing = false;
    $('print').disabled = false;
    // The print dialog copies the page; the images can go once it's up.
    setTimeout(() => {
      box.querySelectorAll('img').forEach((img) => URL.revokeObjectURL(img.src));
      box.replaceChildren();
    }, 60000);
  }
}
$('print').addEventListener('click', print);

// ---------- more menu ----------

const menu = $('menuPopup');
$('more').addEventListener('click', (e) => {
  e.stopPropagation();
  menu.hidden = !menu.hidden;
  $('more').classList.toggle('on', !menu.hidden);
});
document.addEventListener('mousedown', (e) => {
  if (!menu.hidden && !menu.contains(e.target) && e.target !== $('more')) {
    menu.hidden = true;
    $('more').classList.remove('on');
  }
});
const present = menu.querySelector('[data-act=present]');
if (!document.fullscreenEnabled && !document.webkitFullscreenEnabled) present.hidden = true;
menu.addEventListener('click', (e) => {
  const item = e.target.closest('.item');
  if (!item) return;
  menu.hidden = true;
  $('more').classList.remove('on');
  const act = item.dataset.act;
  if (act === 'twoPage') {
    const on = !item.classList.contains('on');
    item.classList.toggle('on', on);
    viewer.spreadMode = on ? SpreadMode.ODD : SpreadMode.NONE;
  } else if (act === 'annotations') {
    const on = !item.classList.contains('on');
    item.classList.toggle('on', on);
    $('viewer').classList.toggle('hide-annotations', !on);
  } else if (act === 'present') {
    startPresenting();
  } else if (act === 'properties') {
    showProperties();
  }
});

// ---------- present ----------

let beforePresent = null;
function startPresenting() {
  if (!doc) return;
  const el = document.documentElement;
  (el.requestFullscreen || el.webkitRequestFullscreen)?.call(el);
}
function onFullscreen() {
  const on = !!(document.fullscreenElement || document.webkitFullscreenElement);
  document.body.classList.toggle('presenting', on);
  if (on) {
    beforePresent = { scale: viewer.currentScaleValue, page: viewer.currentPageNumber, scroll: viewer.scrollMode };
    viewer.scrollMode = ScrollMode.PAGE;
    viewer.currentScaleValue = 'page-fit';
  } else if (beforePresent) {
    viewer.scrollMode = beforePresent.scroll;
    viewer.currentScaleValue = beforePresent.scale;
    beforePresent = null;
  }
}
document.addEventListener('fullscreenchange', onFullscreen);
document.addEventListener('webkitfullscreenchange', onFullscreen);

// ---------- document properties ----------

function formatDate(s) {
  const d = s && pdfjsLib.PDFDateString.toDateObject(s);
  return d ? d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : '–';
}
function formatSize(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}
async function showProperties() {
  if (!doc) return;
  const { info } = await doc.getMetadata().catch(() => ({ info: {} }));
  const page = await doc.getPage(viewer.currentPageNumber);
  const [x0, y0, x1, y1] = page.view;
  const inches = (v) => (Math.abs(v) / 72).toFixed(2);
  const rows = [
    ['File name', saveName],
    ['File size', formatSize(original.length)],
    ['Title', info.Title || '–'],
    ['Author', info.Author || '–'],
    ['Subject', info.Subject || '–'],
    ['Keywords', info.Keywords || '–'],
    ['Created', formatDate(info.CreationDate)],
    ['Modified', formatDate(info.ModDate)],
    ['Application', info.Creator || '–'],
    ['PDF producer', info.Producer || '–'],
    ['PDF version', info.PDFFormatVersion || '–'],
    ['Page count', String(doc.numPages)],
    ['Page size', `${inches(x1 - x0)} × ${inches(y1 - y0)} in`],
    ['Fast web view', info.IsLinearized ? 'Yes' : 'No'],
  ];
  $('propsList').replaceChildren(...rows.flatMap(([k, v]) => {
    const dt = document.createElement('dt');
    dt.textContent = k;
    const dd = document.createElement('dd');
    dd.textContent = v;
    return [dt, dd];
  }));
  $('props').hidden = false;
}
$('propsClose').addEventListener('click', () => { $('props').hidden = true; });
$('props').addEventListener('mousedown', (e) => { if (e.target === $('props')) $('props').hidden = true; });

// ---------- sidebar: thumbnails and outline ----------

const THUMB_W = 88;
let thumbObserver = null;

$('menu').addEventListener('click', () => {
  const open = $('sidebar').hidden;
  $('sidebar').hidden = !open;
  $('menu').classList.toggle('on', open);
  // The page area got narrower or wider; keep the fit.
  requestAnimationFrame(() => {
    if (['auto', 'page-width', 'page-fit'].includes(viewer.currentScaleValue)) viewer.currentScaleValue = viewer.currentScaleValue;
    markThumb(viewer.currentPageNumber, true);
  });
});

async function buildThumbs() {
  const first = await doc.getPage(1);
  const vp = first.getViewport({ scale: 1 });
  thumbObserver?.disconnect();
  thumbObserver = new IntersectionObserver((entries) => {
    for (const en of entries) {
      if (!en.isIntersecting) continue;
      thumbObserver.unobserve(en.target);
      drawThumb(en.target);
    }
  }, { root: $('thumbs'), rootMargin: '300px' });
  $('thumbs').replaceChildren(...Array.from({ length: doc.numPages }, (_, i) => {
    const t = document.createElement('button');
    t.className = 'thumb';
    t.dataset.page = i + 1;
    const pic = document.createElement('span');
    pic.className = 'pic';
    pic.style.aspectRatio = `${vp.width} / ${vp.height}`;
    const n = document.createElement('span');
    n.textContent = i + 1;
    t.append(pic, n);
    t.addEventListener('click', () => { viewer.currentPageNumber = i + 1; });
    thumbObserver.observe(t);
    return t;
  }));
  markThumb(viewer.currentPageNumber || 1);
}

async function drawThumb(t) {
  const page = await doc.getPage(Number(t.dataset.page));
  const rotation = (page.rotate + viewer.pagesRotation) % 360;
  const base = page.getViewport({ scale: 1, rotation });
  const viewport = page.getViewport({ scale: (THUMB_W * devicePixelRatio) / base.width, rotation });
  const canvas = document.createElement('canvas');
  canvas.width = Math.floor(viewport.width);
  canvas.height = Math.floor(viewport.height);
  await page.render({ canvasContext: canvas.getContext('2d'), viewport }).promise.catch(() => {});
  const pic = t.querySelector('.pic');
  pic.style.aspectRatio = `${base.width} / ${base.height}`;
  pic.replaceChildren(canvas);
}

function redrawThumbs() {
  document.querySelectorAll('.thumb').forEach((t) => {
    t.querySelector('.pic').replaceChildren();
    thumbObserver?.observe(t);
  });
}

function markThumb(n, scroll = false) {
  const box = $('thumbs');
  box.querySelectorAll('.thumb.on').forEach((t) => t.classList.remove('on'));
  const t = box.querySelector(`.thumb[data-page="${n}"]`);
  if (!t) return;
  t.classList.add('on');
  if ($('sidebar').hidden) return;
  const r = t.getBoundingClientRect();
  const b = box.getBoundingClientRect();
  if (scroll || r.top < b.top || r.bottom > b.bottom) t.scrollIntoView({ block: 'nearest' });
}

async function buildOutline() {
  const items = await doc.getOutline().catch(() => null);
  if (!items?.length) return;
  $('sideTabs').hidden = false;
  const render = (list) => list.map((it) => {
    const wrap = document.createElement('div');
    const row = document.createElement('button');
    row.className = 'ol-item';
    const toggle = document.createElement('span');
    toggle.className = 'ol-toggle';
    const text = document.createElement('span');
    text.textContent = it.title;
    row.append(toggle, text);
    wrap.append(row);
    if (it.items?.length) {
      toggle.innerHTML = ICONS.chevron;
      const kids = document.createElement('div');
      kids.className = 'ol-kids';
      kids.hidden = true;
      kids.append(...render(it.items));
      wrap.append(kids);
      toggle.addEventListener('click', (e) => {
        e.stopPropagation();
        kids.hidden = !kids.hidden;
        toggle.style.transform = kids.hidden ? '' : 'rotate(90deg)';
      });
    }
    row.addEventListener('click', () => {
      if (it.dest) linkService.goToDestination(it.dest);
      else if (it.url) window.open(it.url);
    });
    return wrap;
  });
  $('outline').replaceChildren(...render(items));
}
const showPanel = (outline) => {
  $('outline').hidden = !outline;
  $('thumbs').hidden = outline;
  $('showOutline').classList.toggle('on', outline);
  $('showThumbs').classList.toggle('on', !outline);
};
$('showThumbs').addEventListener('click', () => showPanel(false));
$('showOutline').addEventListener('click', () => showPanel(true));

// ---------- find (⌘F from the app menu calls window.__bfind) ----------

const findbar = $('findbar');
const findInput = $('findInput');
let findTimer = 0;
function runFind(type, previous = false) {
  eventBus.dispatch('find', {
    source: null, type, query: findInput.value, caseSensitive: false, entireWord: false,
    highlightAll: true, findPrevious: previous, matchDiacritics: false,
  });
}
function showCount({ current, total }) {
  $('findCount').textContent = findInput.value ? `${total ? current : 0}/${total}` : '';
}
eventBus.on('updatefindmatchescount', ({ matchesCount }) => showCount(matchesCount));
eventBus.on('updatefindcontrolstate', ({ state, matchesCount }) => {
  if (state === FindState.NOT_FOUND) showCount({ current: 0, total: 0 });
  else if (matchesCount) showCount(matchesCount);
});
findInput.addEventListener('input', () => { clearTimeout(findTimer); findTimer = setTimeout(() => runFind(''), 80); });
findInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') { e.preventDefault(); runFind('again', e.shiftKey); }
  else if (e.key === 'Escape') { e.preventDefault(); window.__bfind.close(); }
});
$('findNext').addEventListener('click', () => runFind('again', false));
$('findPrev').addEventListener('click', () => runFind('again', true));
$('findClose').addEventListener('click', () => window.__bfind.close());
window.__bfind = {
  open() {
    findbar.hidden = false;
    findInput.focus();
    findInput.select();
    if (findInput.value) runFind('');
  },
  step(d) {
    if (findbar.hidden) return this.open();
    runFind('again', d < 0);
  },
  close() {
    findbar.hidden = true;
    eventBus.dispatch('findbarclose', { source: null });
    $('findCount').textContent = '';
    container.focus();
  },
};

// ---------- keys ----------

document.addEventListener('keydown', (e) => {
  const mod = e.metaKey || e.ctrlKey;
  const typing = e.target.closest?.('input, textarea, [contenteditable]');
  if (mod && e.key.toLowerCase() === 'p') { e.preventDefault(); print(); return; }
  if (mod && e.key.toLowerCase() === 's') { e.preventDefault(); download(); return; }
  if (typing || mod) return;
  const presenting = document.body.classList.contains('presenting');
  if (presenting && ['ArrowRight', 'ArrowDown', 'PageDown', ' '].includes(e.key)) { e.preventDefault(); viewer.nextPage(); }
  else if (presenting && ['ArrowLeft', 'ArrowUp', 'PageUp'].includes(e.key)) { e.preventDefault(); viewer.previousPage(); }
  else if (e.key === 'Escape') {
    if (!$('props').hidden) $('props').hidden = true;
    else if (!menu.hidden) { menu.hidden = true; $('more').classList.remove('on'); }
  }
});

load();
