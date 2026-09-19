//! pet-agent 独立开机启动（HKCU Run，键名 DesktopPetAgent）。

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("DesktopPetAgent");

fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub fn is_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            Some(0),
            KEY_QUERY_VALUE,
            &mut hkey,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let mut data = [0u8; 1024];
        let mut size = data.len() as u32;
        let rc = RegQueryValueExW(
            hkey,
            VALUE_NAME,
            None,
            None,
            Some(data.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        rc == ERROR_SUCCESS
    }
}

pub fn enable() -> bool {
    let path = exe_path();
    if path.is_empty() {
        return false;
    }
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2) };
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            Some(0),
            KEY_SET_VALUE,
            &mut hkey,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let rc = RegSetValueExW(hkey, VALUE_NAME, None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(hkey);
        rc == ERROR_SUCCESS
    }
}

pub fn disable() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            Some(0),
            KEY_SET_VALUE,
            &mut hkey,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let rc = RegDeleteValueW(hkey, VALUE_NAME);
        let _ = RegCloseKey(hkey);
        rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND
    }
}

pub fn toggle() -> bool {
    if is_enabled() {
        let _ = disable();
    } else {
        let _ = enable();
    }
    is_enabled()
}
