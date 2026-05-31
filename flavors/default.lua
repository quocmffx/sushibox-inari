-- SushiBox Inari — default flavor
-- Edit this file to customise ports, sites, and hooks.
-- All paths are relative to the Inari.exe directory.

flavor = "default"

ports = {
    panel = 1788,   -- web panel
    web   = 8080,   -- nginx / PHP
    mysql = 3307,   -- MariaDB 10.3
    redis = 6380,   -- Redis / Valkey
}

sites = {
    {
        name  = "default",
        root  = "sites/default",
        index = "index.php",
    },
}

-- nginx_template = nil  -- set to a string to override the built-in template

hooks = {
    on_start = {},  -- shell commands run after services start
    on_stop  = {},  -- shell commands run before services stop
}
