// Picks the right installer from the newest GitHub release. If GitHub says no,
// the button keeps its href to the releases page and nobody hits a dead end.
const REPO = 'azterixx/browser';

const btn = document.getElementById('dl');
const label = document.getElementById('dl-label');
const meta = document.getElementById('dl-meta');
const alt = document.getElementById('alt');

const ua = navigator.userAgent;
const os = /Windows/i.test(ua) ? 'windows' : /Mac|iPhone|iPad/i.test(ua) ? 'macos' : /Linux|Android/i.test(ua) ? 'linux' : 'other';
const NAMES = { macos: 'macOS', windows: 'Windows', linux: 'Linux' };

// Browsers hide the CPU, so a Mac gets Apple Silicon by default and an Intel link next to it.
const match = {
  macos: [
    { re: /aarch64.*\.dmg$/i, name: 'macOS (Apple Silicon)' },
    { re: /x64.*\.dmg$/i, name: 'macOS (Intel)' },
  ],
  windows: [
    { re: /-setup\.exe$/i, name: 'Windows' },
    { re: /\.msi$/i, name: 'Windows (MSI)' },
  ],
  linux: [
    { re: /\.AppImage$/i, name: 'Linux (AppImage)' },
    { re: /\.deb$/i, name: 'Linux (deb)' },
    { re: /\.rpm$/i, name: 'Linux (rpm)' },
  ],
  other: [],
};

const mb = (n) => `${(n / 1048576).toFixed(1)} MB`;

function pick(assets, rules) {
  return rules
    .map((r) => ({ ...r, asset: assets.find((a) => r.re.test(a.name)) }))
    .filter((r) => r.asset);
}

async function load() {
  const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`);
  if (!res.ok) throw new Error(res.status);
  const rel = await res.json();
  // .sig and latest.json belong to the updater, not to people.
  const assets = rel.assets.filter((a) => !/\.sig$|^latest\.json$/i.test(a.name));
  const hits = pick(assets, match[os]);
  if (!hits.length) throw new Error('no build for this system');

  const [first, ...rest] = hits;
  btn.href = first.asset.browser_download_url;
  label.textContent = `Download for ${first.name}`;
  meta.textContent = `${rel.tag_name} · ${mb(first.asset.size)}`;

  alt.replaceChildren(
    ...rest.flatMap((h, i) => {
      const a = document.createElement('a');
      a.href = h.asset.browser_download_url;
      a.textContent = h.name;
      return i ? [document.createTextNode(' · '), a] : [a];
    }),
  );
  if (rest.length) alt.prepend(document.createTextNode('Also for: '));
}

if (os === 'other') {
  label.textContent = 'See all downloads';
} else {
  label.textContent = `Download for ${NAMES[os]}`;
}

load().catch((e) => {
  meta.textContent = 'see all releases';
  alt.textContent = `Could not read GitHub (${e.message}).`;
});
