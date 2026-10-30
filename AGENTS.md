# AGENTS.md

WeFlow is an Electron 43 + React 19 desktop app that archives and analyzes WeChat data locally. Main process in `electron/`, renderer in `src/`, native WCDB in `native/wcdb-native/`.

## Commands

- `npm run typecheck` — renderer **and** electron type checks. Run this after touching `electron/**` or `src/**`. Plain `npx tsc` only checks the renderer.
- `npm run test:wcdb` — WCDB service smoke test (46 assertions, no native library needed).
- `npm run test:release-notes` — renders malicious release notes and the update dialog (unsigned notice + SHA-512), asserting no HTML or event handlers survive.
- `node scripts/check-release-config.cjs` — verifies the update feed, publish config, `repository.url` and README all target the same GitHub repo.
- `node scripts/check-release-artifacts.cjs [platform] [arch]` — inspects `release/**` (or `RELEASE_DIST_DIR`) for `app.asar`, koffi, and the bundled WCDB DLL.
- `npm run build:portable` — Windows single-file exe into `release/`. Long-running (several minutes); run only when packaging changes.

## Package manager

Releases build with **npm + package-lock.json**. `pnpm-lock.yaml` and `pnpm-workspace.yaml` are gitignored local conveniences, and `pnpm.overrides` is inert for npm. When adding or upgrading a dependency, update `package-lock.json` too; when changing pnpm build approvals, keep `allowBuilds` in `pnpm-workspace.yaml` (including `electron`) so a fresh `pnpm install` produces a runnable `node_modules/electron/dist`.

## Release critical paths

Every change touching these needs `check-release-config.cjs` / `check-release-artifacts.cjs` to pass:

- `scripts/after-pack.cjs` — copies koffi and `native/wcdb-native/prebuilt/wcdb_api.dll` into `<app>/resources/resources/native/wcdb-native/dist`. Under CI it throws when either is missing; locally it warns. `WCDB_DLL_SOURCE` can point at a different prebuilt DLL.
- `package.json` → `build.electronFuses`, `build.asar`, `build.extraResources` — keep fuses enabled (no inspect, no `NODE_OPTIONS`, asar integrity, asar-only load).
- `package.json` → `build.publish` and `electron/main.ts` `repoBase` — the canonical release repo is `hicccc77/WeFlow` (the local clone may be a fork; the update feed intentionally follows upstream).
- `electron/services/wcdbCore.ts` → `getDllPath()` — resolution order is explicit libPath → `WCDB_DLL_PATH` (must exist) → bundled DLL. Keep the bundled path in sync with `after-pack.cjs`.

## Security invariants

- Renderer IPC that touches the filesystem stays constrained: `shell:openPath` only opens paths inside userData, downloads, `Documents/WeFlow`, the configured export directory, or directories the user picked through a dialog. New path-taking IPC needs the same containment, not a raw `fs`/`shell` call.
- Release notes and any remote text render as React nodes. Raw HTML injection (`dangerouslySetInnerHTML`) stays out of the renderer; the only exception is the Shiki-generated highlight HTML in `src/components/ai-elements/code-block.tsx`, which is built from escaped code text.
- `webSecurity` is `true` in the main, video and image windows. Local media (images, video, audio, avatars) must be served through the `weflow-media://` protocol (`electron/mediaProtocol.ts`), which only allows files under userData, temp, `Documents/WeFlow`, and the configured `cachePath`/`dbPath`. Renderer code must build media URLs with `src/utils/mediaUrl.ts` (`toMediaUrl`) instead of `file://`; main-process producers use `electron/mediaUrl.ts`. Do not re-add raw `file://` media to the renderer.
- The production build injects a CSP meta tag (`vite.config.ts`, build-only so dev HMR keeps working). It forbids inline scripts and `file:` connections; `script-src` includes `'wasm-unsafe-eval'` because Shiki instantiates its highlighting engine from WebAssembly. If you add a new resource origin, extend the CSP in the same change.
- `session.defaultSession` permission handlers deny everything except `clipboard-sanitized-write`. New permission needs (camera, notifications, etc.) require an explicit allow-list entry.
- The local HTTP API (`httpService`) requires `httpApiToken` for every route except `/health`, binds to `127.0.0.1` by default, and accepts query tokens only because SSE clients cannot set headers. Keep each of these properties.
- Secrets (`decryptKey`, `imageAesKey`, `imageXorKey`, `aiModelApiKey`) go through `safeStorage`; diagnostics log booleans and counts, never key material.

## Deeper context

Architecture review, risk register and remediation plan: `docs/architecture-review/`. HTTP API usage: `docs/HTTP-API.md`. Recent packaging/dependency notes: `docs/修复总结-2026-10-30.md`.
