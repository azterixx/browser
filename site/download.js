// Works out the visitor's system and CPU, then hands them that exact installer.
// The file is pulled in a hidden frame, so the page never jumps to GitHub.
const REPO = 'azterixx/browser';

const btn = document.getElementById('dl');
const label = document.getElementById('dl-label');
const meta = document.getElementById('dl-meta');
const alt = document.getElementById('alt');

const ua = navigator.userAgent;
const os = /Windows/i.test(ua) ? 'windows' : /Mac|iPhone|iPad/i.test(ua) ? 'macos' : /Linux|Android/i.test(ua) ? 'linux' : 'other';
const NAMES = { macos: 'macOS', windows: 'Windows', linux: 'Linux' };

// No browser reports the CPU, so the GPU name gives it away: Apple Silicon Macs
// render through an "Apple M…" GPU, Intel ones through Intel or Radeon.
function appleSilicon() {
  try {
    const gl = document.createElement('canvas').getContext('webgl');
    const info = gl && gl.getExtension('WEBGL_debug_renderer_info');
    const gpu = info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : '';
    if (/apple m\d|apple gpu/i.test(gpu)) return true;
    if (/intel|radeon|amd/i.test(gpu)) return false;
  } catch {}
  // Rosetta and locked-down browsers land here; Apple stopped selling Intel Macs in 2023.
  return navigator.maxTouchPoints > 0 || !/Intel/i.test(navigator.platform || '');
}

const MAC_ARM = { re: /aarch64.*\.dmg$/i, name: 'macOS (Apple Silicon)' };
const MAC_X64 = { re: /x64\.dmg$/i, name: 'macOS (Intel)' };

const rules = {
  macos: () => (appleSilicon() ? [MAC_ARM, MAC_X64] : [MAC_X64, MAC_ARM]),
  windows: () => [
    { re: /-setup\.exe$/i, name: 'Windows' },
    { re: /\.msi$/i, name: 'Windows (MSI)' },
  ],
  linux: () => [
    { re: /\.AppImage$/i, name: 'Linux (AppImage)' },
    { re: /\.deb$/i, name: 'Linux (deb)' },
    { re: /\.rpm$/i, name: 'Linux (rpm)' },
  ],
  other: () => [],
};

const mb = (n) => `${(n / 1048576).toFixed(1)} MB`;

// A hidden frame downloads the file without navigating: the page, and the
// "Also for" links, stay where they are.
function pull(url) {
  const frame = document.createElement('iframe');
  frame.hidden = true;
  frame.src = url;
  document.body.append(frame);
  setTimeout(() => frame.remove(), 90000);
}

function arm(a, name, started) {
  a.addEventListener('click', (e) => {
    if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return; // let "open in new tab" be
    e.preventDefault();
    pull(a.href);
    started(name);
  });
}

async function load() {
  const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`);
  if (!res.ok) throw new Error(res.status);
  const rel = await res.json();
  // .sig files and latest.json belong to the updater, not to people.
  const assets = rel.assets.filter((a) => !/\.sig$|^latest\.json$/i.test(a.name));
  const hits = rules[os]()
    .map((r) => ({ ...r, asset: assets.find((a) => r.re.test(a.name)) }))
    .filter((r) => r.asset);
  if (!hits.length) throw new Error('no build for this system');

  const [first, ...rest] = hits;
  const idle = `${rel.tag_name} · ${mb(first.asset.size)}`;
  btn.href = first.asset.browser_download_url;
  label.textContent = `Download for ${first.name}`;
  meta.textContent = idle;

  const started = (name) => {
    meta.textContent = `Downloading ${name}… check your downloads folder`;
    setTimeout(() => { meta.textContent = idle; }, 8000);
  };
  arm(btn, first.name, started);

  alt.replaceChildren(
    ...rest.flatMap((h, i) => {
      const a = document.createElement('a');
      a.href = h.asset.browser_download_url;
      a.textContent = h.name;
      arm(a, h.name, started);
      return i ? [document.createTextNode(' · '), a] : [a];
    }),
  );
  if (rest.length) alt.prepend(document.createTextNode('Also for: '));

  // browser-indol-mu.vercel.app/?auto — a link that downloads on arrival.
  if (new URLSearchParams(location.search).has('auto')) {
    pull(btn.href);
    started(first.name);
  }
}

label.textContent = os === 'other' ? 'See all downloads' : `Download for ${NAMES[os]}`;

load().catch((e) => {
  meta.textContent = 'see all releases';
  alt.textContent = `Could not read GitHub (${e.message}).`;
});
