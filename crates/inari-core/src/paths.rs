use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct InariPaths {
    pub base:    PathBuf,
    pub runtime: PathBuf,
    pub flavors: PathBuf,
    pub config:  PathBuf,
    pub sites:   PathBuf,
    pub data:    PathBuf,
    pub logs:    PathBuf,
    pub nginx:   PathBuf,
    pub php:     PathBuf,
    pub mysql:   PathBuf,
    pub redis:   PathBuf,
}

impl InariPaths {
    /// Resolve paths relative to the running executable's directory.
    pub fn from_exe() -> anyhow::Result<Self> {
        let exe = std::env::current_exe()?;
        let base = exe
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine exe directory"))?
            .to_path_buf();
        // Expand any 8.3 short path (e.g. C:\Users\QUANGQ~1\...) to its long
        // form. nginx CreateFile() fails on short paths when 8.3 generation is
        // disabled on the volume, which broke launches from %TEMP% and from
        // installs under long/spaced usernames.
        Ok(Self::from_base(normalize_long_path(base)))
    }

    pub fn from_base(base: PathBuf) -> Self {
        let runtime = base.join("runtime");
        Self {
            nginx:   runtime.join("nginx"),
            php:     runtime.join("php"),
            mysql:   runtime.join("mysql"),
            redis:   runtime.join("redis"),
            flavors: base.join("flavors"),
            config:  base.join("config"),
            sites:   base.join("sites"),
            data:    base.join("data"),
            logs:    base.join("logs"),
            runtime,
            base,
        }
    }

    // --- binary paths ---

    pub fn nginx_exe(&self) -> PathBuf {
        self.nginx.join("nginx.exe")
    }

    pub fn php_exe(&self) -> PathBuf {
        self.php.join("php-cgi.exe")
    }

    /// MariaDB 10.3 portable layout: mysql/bin/mysqld.exe
    pub fn mysql_exe(&self) -> PathBuf {
        self.mysql.join("bin").join("mysqld.exe")
    }

    pub fn redis_exe(&self) -> PathBuf {
        self.redis.join("redis-server.exe")
    }

    /// MariaDB admin tool, used for graceful shutdown (avoids datadir crash recovery).
    pub fn mysqladmin_exe(&self) -> PathBuf {
        self.mysql.join("bin").join("mysqladmin.exe")
    }

    /// Bundled Adminer single-file PHP app, exposed by generated nginx config
    /// at /_inari/adminer.php when present.
    pub fn adminer_php(&self) -> PathBuf {
        self.runtime.join("adminer").join("adminer.php")
    }

    // --- runtime paths ---

    pub fn nginx_conf(&self) -> PathBuf {
        self.config.join("nginx.conf")
    }

    /// Generated php.ini (dev-tuned), passed to php-cgi via -c.
    pub fn php_ini(&self) -> PathBuf {
        self.config.join("php.ini")
    }

    pub fn mysql_datadir(&self) -> PathBuf {
        self.data.join("mysql")
    }

    pub fn pid_file(&self, service: &str) -> PathBuf {
        self.data.join(format!("{}.pid", service))
    }

    pub fn default_flavor(&self) -> PathBuf {
        self.flavors.join("default.lua")
    }
}

/// Expand a Windows 8.3 short path to its long form via GetLongPathNameW.
/// Returns the input unchanged if it has no short components or on any error.
/// No-op on non-Windows.
#[cfg(windows)]
fn normalize_long_path(p: PathBuf) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetLongPathNameW;

    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        // First call with empty buffer returns the required length (incl. NUL).
        let needed = GetLongPathNameW(PCWSTR(wide.as_ptr()), None);
        if needed == 0 {
            return p;
        }
        let mut buf = vec![0u16; needed as usize];
        let written = GetLongPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buf));
        if written == 0 || written as usize > buf.len() {
            return p;
        }
        let s = std::ffi::OsString::from_wide(&buf[..written as usize]);
        PathBuf::from(s)
    }
}

#[cfg(not(windows))]
fn normalize_long_path(p: PathBuf) -> PathBuf {
    p
}
