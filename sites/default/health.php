<?php
// Simple JSON health endpoint — verifies non-index PHP routing works.
header('Content-Type: application/json');
echo json_encode([
    'ok'      => true,
    'php'     => PHP_VERSION,
    'sapi'    => PHP_SAPI,
    'time'    => date('c'),
    'uri'     => $_SERVER['REQUEST_URI'] ?? null,
], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES);
