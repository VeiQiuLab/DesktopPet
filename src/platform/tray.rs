//! Windows 系统托盘图标与托盘菜单。

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::log_error;

/// 托盘回调消息（WM_APP + 1）。
pub const TRAY_CALLBACK_MSG: u32 = WM_APP + 1;
/// 托盘图标 ID。
pub const TRAY_ICON_ID: u32 = 1;

/// 托盘菜单命令 ID。
pub const CMD_TRAY_TOGGLE_VISIBLE: usize = 100;
pub const CMD_TRAY_RESET: usize = 101;
pub const CMD_TRAY_AUTOSTART: usize = 102;
pub const CMD_TRAY_EXIT: usize = 103;
pub const CMD_TRAY_TEST_BUBBLE: usize = 104;
pub const CMD_CHAR_BASE: usize = 200;

/// 系统托盘图标。Drop 时自动移除。
pub struct TrayIcon {
    hwnd: HWND,
    added: bool,
}

impl TrayIcon {
    pub unsafe fn new(hwnd: HWND) -> Option<Self> {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_ICON_ID;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = TRAY_CALLBACK_MSG;
        // 托盘专用图标（头部特写，小尺寸更清晰）
        nid.hIcon = crate::platform::assets::load_tray_icon();
        let tip: Vec<u16> = "Desktop Pet".encode_utf16().collect();
        for (i, c) in tip.iter().enumerate().take(127) {
            nid.szTip[i] = *c;
        }
        if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
            log_error("tray icon add failed");
            return None;
        }
        Some(TrayIcon { hwnd, added: true })
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        if self.added {
            unsafe {
                let mut nid = NOTIFYICONDATAW::default();
                nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = self.hwnd;
                nid.uID = TRAY_ICON_ID;
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            }
            self.added = false;
        }
    }
}

/// 向菜单追加一个字符串项。
unsafe fn append(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let _ = AppendMenuW(menu, flags, id, PCWSTR(wide.as_ptr()));
}

/// 弹出托盘菜单，返回选中的命令 ID（0 = 取消）。
pub unsafe fn show_tray_menu(
    hwnd: HWND,
    characters: &[(String, String)],
    active: &str,
    auto_start: bool,
) -> usize {
    let menu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return 0,
    };

    append(menu, MF_STRING, CMD_TRAY_TOGGLE_VISIBLE, "显示 / 隐藏桌宠");
    append(menu, MF_SEPARATOR, 0, "");

    // 角色子菜单
    let char_menu = CreatePopupMenu().unwrap_or_default();
    if characters.is_empty() {
        append(char_menu, MF_STRING | MF_GRAYED, 0, "(无角色)");
    } else {
        for (i, (id, name)) in characters.iter().enumerate() {
            let mut flags = MF_STRING;
            if id == active {
                flags |= MF_CHECKED;
            }
            append(char_menu, flags, CMD_CHAR_BASE + i, name);
        }
    }
    append(menu, MF_POPUP, char_menu.0 as usize, "角色");

    append(menu, MF_SEPARATOR, 0, "");
    append(menu, MF_STRING, CMD_TRAY_TEST_BUBBLE, "测试气泡");
    append(menu, MF_STRING, CMD_TRAY_RESET, "重置位置");
    let mut as_flags = MF_STRING;
    if auto_start {
        as_flags |= MF_CHECKED;
    }
    append(menu, as_flags, CMD_TRAY_AUTOSTART, "开机启动");
    append(menu, MF_SEPARATOR, 0, "");
    append(menu, MF_STRING, CMD_TRAY_EXIT, "退出");

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);

    let _ = SetForegroundWindow(hwnd);

    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        pt.x,
        pt.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    cmd.0 as usize
}
