<?php
// Inari smoke-test landing page.
// Verifies: PHP runs via FastCGI, nginx routing, basic extensions.

$php   = PHP_VERSION;
$sapi  = PHP_SAPI;
$now   = date('Y-m-d H:i:s');
$exts  = ['pdo_mysql', 'mysqli', 'curl', 'mbstring', 'openssl', 'gd', 'redis'];
$loaded = array_filter($exts, 'extension_loaded');
$missing = array_diff($exts, $loaded);
?>
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Inari · PHP smoke test</title>
  <style>
    :root { color-scheme: dark; }
    body {
      margin: 0; min-height: 100vh; display: grid; place-items: center;
      background: #1e1b2e; color: #e8e6f0;
      font: 15px/1.6 system-ui, -apple-system, "Segoe UI", sans-serif;
    }
    .card {
      width: min(560px, 90vw); background: #262236; border: 1px solid #3a3550;
      border-radius: 14px; padding: 28px 32px;
      box-shadow: 0 12px 40px rgba(0,0,0,.35);
    }
    h1 { margin: 0 0 4px; font-size: 20px; }
    .sub { color: #a8a2c0; font-size: 13px; margin-bottom: 20px; }
    .accent { color: #f59e0b; }
    table { width: 100%; border-collapse: collapse; font-size: 13px; }
    td { padding: 6px 0; border-bottom: 1px solid #332e48; }
    td:first-child { color: #a8a2c0; width: 42%; }
    .ok { color: #34d399; } .bad { color: #f87171; }
    code { background: #1e1b2e; padding: 1px 6px; border-radius: 5px; }
  </style>
</head>
<body>
  <div class="card">
    <h1>🦊 <span class="accent">Inari</span> is serving PHP</h1>
    <p class="sub">If you can read this, nginx + php-cgi are wired up correctly.</p>
    <table>
      <tr><td>PHP version</td><td><code><?= htmlspecialchars($php) ?></code></td></tr>
      <tr><td>SAPI</td><td><code><?= htmlspecialchars($sapi) ?></code></td></tr>
      <tr><td>Server time</td><td><?= htmlspecialchars($now) ?></td></tr>
      <tr>
        <td>Extensions loaded</td>
        <td><?= $loaded ? '<span class="ok">' . htmlspecialchars(implode(', ', $loaded)) . '</span>' : '<span class="bad">none</span>' ?></td>
      </tr>
      <?php if ($missing): ?>
      <tr><td>Not loaded</td><td class="bad"><?= htmlspecialchars(implode(', ', $missing)) ?></td></tr>
      <?php endif; ?>
    </table>
  </div>
</body>
</html>
