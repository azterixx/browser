# Browser

A small desktop browser built with Tauri 2. It uses the web engine your system already has:
WebKit on macOS, WebView2 on Windows, WebKitGTK on Linux.

## Develop

```sh
npm install
npm run dev
```

## Releasing

Releases are built by GitHub Actions and installed copies update themselves from them.

One-time setup on GitHub:

1. Settings → Secrets → Actions, add:
   - `TAURI_SIGNING_PRIVATE_KEY` — the contents of `~/.tauri/browser.key`
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — empty (the key has no password)
2. In Vercel, connect this repo to the `browser` project so `site/` deploys on every push
   (Project → Settings → Git). Until then: `vercel deploy --cwd site --prod`.

Keep `~/.tauri/browser.key` safe. Lose it and no existing install can ever be updated again.

To ship a version:

```sh
npm version patch          # bumps package.json
# set the same number in src-tauri/tauri.conf.json and src-tauri/Cargo.toml
git commit -am "v0.1.1" && git tag v0.1.1
git push --follow-tags
```

The workflow builds macOS (Apple Silicon + Intel) and Windows, signs the bundles,
and publishes the release together with `latest.json`. The app checks that file on start
and every six hours; the landing page reads the same release to pick the right installer.

Landing page: https://browser-indol-mu.vercel.app (Vercel project `azterixxs-projects/browser`).
