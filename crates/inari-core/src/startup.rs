//! "Run Inari at Windows startup" via the per-user Run registry key.
//!
//! Writes/removes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Inari`.
//! Per-user (HKCU) needs no admin rights, which fits a portable, copy-and-run
//! tool. The value is the quoted absolute path to the current exe, so moving
//! the folder and re-enabling the toggle keeps it correct.
//!
//! This only launches Inari. Which services then start is governed by the
//! per-service auto-start setting — the two are deliberately independent.

#[cfg(windows)]
mod imp {
    use anyhow::{Context, Result};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
    };

    const RUN_KEY: PCWSTR =
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    const VALUE_NAME: PCWSTR = w!("Inari");

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<HKEY> {
        let mut hkey = HKEY::default();
        let rc = unsafe {
            RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, 0, access, &mut hkey)
        };
        if rc != ERROR_SUCCESS {
            anyhow::bail!("RegOpenKeyExW failed: {:?}", rc);
        }
        Ok(hkey)
    }

    /// Enable: write the current exe path under the Run key.
    pub fn enable() -> Result<()> {
        let exe = std::env::current_exe().context("cannot resolve current exe")?;
        // Quote the path so spaces in the install dir are handled by the shell.
        let value = format!("\"{}\"", exe.to_string_lossy());
        let wide = to_wide(&value);
        let bytes = unsafe {
            std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2)
        };

        let hkey = open(KEY_WRITE)?;
        let rc = unsafe { RegSetValueExW(hkey, VALUE_NAME, 0, REG_SZ, Some(bytes)) };
        unsafe { let _ = RegCloseKey(hkey); }
        if rc != ERROR_SUCCESS {
            anyhow::bail!("RegSetValueExW failed: {:?}", rc);
        }
        Ok(())
    }

    /// Disable: remove the Run value (no-op if absent).
    pub fn disable() -> Result<()> {
        let hkey = open(KEY_WRITE)?;
        let rc = unsafe { RegDeleteValueW(hkey, VALUE_NAME) };
        unsafe { let _ = RegCloseKey(hkey); }
        // Treat "not found" as success — disabling something already off is fine.
        if rc != ERROR_SUCCESS && rc.0 != 2 {
            anyhow::bail!("RegDeleteValueW failed: {:?}", rc);
        }
        Ok(())
    }

    /// Whether the Run value currently exists.
    pub fn is_enabled() -> bool {
        let hkey = match open(KEY_READ) {
            Ok(h) => h,
            Err(_) => return false,
        };
        let rc = unsafe {
            RegQueryValueExW(hkey, VALUE_NAME, Some(std::ptr::null_mut()), None, None, None)
        };
        unsafe { let _ = RegCloseKey(hkey); }
        rc == ERROR_SUCCESS
    }

    /// Reconcile the registry with the desired state.
    pub fn apply(enabled: bool) -> Result<()> {
        if enabled { enable() } else { disable() }
    }
}

#[cfg(windows)]
pub use imp::{apply, disable, enable, is_enabled};

#[cfg(not(windows))]
mod imp {
    use anyhow::Result;
    pub fn apply(_enabled: bool) -> Result<()> { Ok(()) }
    pub fn enable() -> Result<()> { Ok(()) }
    pub fn disable() -> Result<()> { Ok(()) }
    pub fn is_enabled() -> bool { false }
}

#[cfg(not(windows))]
pub use imp::{apply, disable, enable, is_enabled};
