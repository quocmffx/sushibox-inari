# Third-Party Software

Inari is a runtime manager. It does not modify the programs it bundles; it
downloads released binaries (see `runtime/manifest.toml`) and launches them.
The Inari application code is MIT licensed (see `LICENSE`). The bundled and
fetched components below are licensed separately by their respective authors.

When you redistribute an Inari portable bundle, you redistribute these
components too, so their license terms (notably attribution and, for MariaDB,
the GPL source-availability requirement) apply to your distribution.

| Component | Version | License | Source |
|---|---|---|---|
| nginx | 1.18.0 | BSD-2-Clause | https://nginx.org/ |
| PHP | 8.4.x | PHP License 3.01 | https://www.php.net/license/ |
| MariaDB (server) | 10.3.x | GPL-2.0-only | https://mariadb.org/ |
| MariaDB (client libraries) | 10.3.x | LGPL-2.1 | https://mariadb.org/ |
| Redis (Windows, tporadowski fork) | 5.0.14.x | BSD-3-Clause | https://github.com/tporadowski/redis |
| Adminer | 4.8.1 | Apache-2.0 OR GPL-2.0 | https://www.adminer.org/ |
| Microsoft Edge WebView2 Runtime | latest | Microsoft Software License Terms (Distributable Code) | https://developer.microsoft.com/microsoft-edge/webview2/ |
| Be Vietnam Pro (font) | — | SIL Open Font License 1.1 | https://github.com/bettergui/BeVietnamPro |

## MariaDB (GPL-2.0) — source availability

The Inari bundle ships the MariaDB server binary (`mysqld.exe`), which is
licensed under GPL-2.0. The binaries are unmodified releases obtained from
MariaDB's official download site. The corresponding source code for each
version is available from the MariaDB Foundation at:

  https://mariadb.org/download/

A copy of the GNU General Public License v2 is available at:

  https://www.gnu.org/licenses/old-licenses/gpl-2.0.html

## Notes

- Inari itself contains no third-party source code; it only orchestrates the
  binaries listed above as separate processes.
- Runtime binaries are **not** committed to this repository. They are fetched
  on demand by `scripts/fetch-runtime.ps1` per `runtime/manifest.toml`.
- The Redis component intentionally uses the tporadowski Windows fork pinned to
  the 5.0.x line, which predates the Redis Source Available License (RSAL) and
  remains BSD-3-Clause.
