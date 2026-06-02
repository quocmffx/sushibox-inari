# Changelog

All notable changes to Inari are documented here.

## v0.1.0-beta.4 — 2026-06-02

State sync and smoke-test hardening.

- Normalized Settings state: `/api/settings` now returns the persisted settings
  and effective config after save, while `/api/status` includes authoritative
  service ports.
- Updated the panel to apply backend save responses immediately, refresh config
  and status, and show save success/error notices inside the Settings overlay.
- Fixed the Adminer nginx alias by replacing `{adminer_dir}` correctly and made
  Adminer packaging reproducible through `package-portable.ps1`.
- Added an Inari Adminer wrapper for local dev defaults (`127.0.0.1:3307`,
  `root`, empty password) and patched Adminer 4.8.1's PHP 8 null-offset warning.
- Added `scripts/smoke-desktop.ps1` to verify desktop bundles end-to-end:
  settings/config/status sync, service start, web response, and Adminer response.
- Updated portable packaging to include release-check and smoke scripts.

## v0.1.0-beta.3 — 2026-06-02

Reliability and Laravel dev polish.

- Added Laravel/front-controller friendly nginx fallback via `try_files`, so
  deep links route through `index.php` while static files are still served
  directly.
- Added an Adminer shortcut served from the bundled runtime at
  `/_inari/adminer.php`, keeping user web roots clean.
- Added port conflict diagnostics to start failures: reports which PID and
  process is already using the port.
- Hardened service status with PID identity checks to avoid false "running"
  states after Windows recycles a stale PID.
- Improved dependency-aware service lifecycle: nginx starts after PHP/backends
  and stops before the services it depends on.
- Kept PHP-CGI as an internal nginx dependency in the public UI/status surface.
- Added `scripts/release-check.ps1` for repeatable beta validation.
- Fixed deprecated Tauri tray API usage.

## v0.1.0-beta.2 — 2026-05-31

UX and design pass on top of beta.1.

- Theme is now **System / Light / Dark** (segmented control in Settings),
  persisted. Removed the forced-dark override that caused a flash on first
  toggle.
- Window **docks to the bottom-right** of the primary monitor (above the
  taskbar) on first run, like PowerToys / PC Manager, and **remembers** wherever
  you drag it afterwards (saved portably next to the exe).
- Settings footer links to the **GitHub repo**; both Settings tabs now fit
  without scrolling.
- Removed the misleading "default" flavor chip from the header.
- README: logo header plus panel / settings / dark / English / desktop
  screenshots (EN and VI).

## v0.1.0-beta.1 — 2026-05-31

First public beta. Builds on v0.1.0 with:

- Settings: "Run at Windows startup" (per-user HKCU Run entry; only launches
  Inari, does not start services).
- Settings: "Start minimized to tray" (launch straight to tray without showing
  the main window). Independent of startup and per-service auto-start.
- README: beta notice, SmartScreen first-launch note, panel screenshot.

## v0.1.0 — 2026-05-31

First public release.

### Features
- Portable, copy-and-run Windows dev runtime manager for nginx, PHP, MariaDB,
  and Redis, controlled from a compact native panel (Tauri 2).
- Per-service and bulk start/stop/restart, with live status and the bundled
  version of each service.
- Auto-start selected services on launch.
- GUI-first configuration: ports, web root, auto-start, dark mode, and
  language, persisted to `data/settings.json` (overrides `flavors/default.lua`).
- Generated, dev-tuned `nginx.conf` and `php.ini` on every start (errors
  visible, UTF-8, sensible limits, opcache with fast revalidation, and the
  common dev extensions: pdo_mysql, mysqli, curl, mbstring, openssl, gd, ...).
- Dev shortcuts: open the site in the system browser, open the site folder,
  open logs.
- System tray with hide-to-tray on close; single-instance.
- English and Vietnamese interface with a flag toggle.
- `inari-cli.exe` for automation (start/stop/restart/status).

### Packaging
- Portable bundle is ~204 MB (zip ~71 MB). The bundled MariaDB is trimmed at
  packaging time from 334 MB to 47 MB (drops debug symbols, dev link libs,
  the template datadir, backup tooling, and unused maintenance clients) while
  remaining able to init, run, and shut down cleanly.
- Bundle ships `LICENSE` and `THIRD_PARTY.md`.

### Fixes / hardening
- Services spawn with `CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB` so no
  console flashes and children are not killed by an inherited job object.
- Graceful MariaDB shutdown via `mysqladmin` to avoid datadir crash recovery.
- Resolve the install directory to its long-path form, so launches from 8.3
  short paths (e.g. `%TEMP%`, or long/spaced usernames) no longer break nginx.
- Strip a UTF-8 BOM from `settings.json` before parsing, so an editor-saved
  BOM no longer silently resets settings (including auto-start) to defaults.

### License
- Inari's own code is MIT. Bundled runtimes are licensed separately; see
  `THIRD_PARTY.md`. The bundled MariaDB server is GPL-2.0.
