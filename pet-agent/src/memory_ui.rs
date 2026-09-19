//! Memory Manager 原生窗口（Win32，pet-agent 内）。
//!
//! 视图：已保存 / 待确认 / 已删除 / 变更记录；搜索 + kind 过滤；
//! 操作：编辑 / 删除(soft) / Pin / Unpin / 恢复 / 接受 / 拒绝 / 导出 / 备份。
//! 关闭=隐藏；复用同一窗口。

use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::log_line;
use crate::memory::MemoryManager;

const ID_LIST: i32 = 2001;
const ID_EDIT: i32 = 2002;
const ID_STATUS: i32 = 2003;
const ID_SEARCH: i32 = 2004;
const ID_KIND: i32 = 2005;
const ID_REFRESH: i32 = 2100;
const ID_DELETE: i32 = 2101;
const ID_PIN: i32 = 2102;
const ID_UNPIN: i32 = 2103;
const ID_RESTORE: i32 = 2104;
const ID_ACCEPT: i32 = 2105;
const ID_REJECT: i32 = 2106;
const ID_EDIT_SAVE: i32 = 2107;
const ID_EXPORT: i32 = 2108;
const ID_BACKUP: i32 = 2109;
const ID_VIEW_ACTIVE: i32 = 2200;
const ID_VIEW_PENDING: i32 = 2201;
const ID_VIEW_DELETED: i32 = 2202;
const ID_VIEW_AUDIT: i32 = 2203;

const BTN: u32 = 0x0000_0000; // BS_PUSHBUTTON
const EDGE: u32 = 0x0000_0200; // WS_EX_CLIENTEDGE
const BORDER: u32 = 0x0080_0000; // WS_BORDER
const AUTOHSCROLL: u32 = 0x0080; // ES_AUTOHSCROLL
const VSCROLL: u32 = 0x0020_0000; // WS_VSCROLL
const LBS_NOTIFY: u32 = 0x0000_0001;
const CBS_DROPDOWNLIST: u32 = 0x0003;

#[derive(PartialEq, Clone, Copy)]
enum View {
    Active,
    Pending,
    Deleted,
    Audit,
}

struct MgrState {
    mgr: MemoryManager,
    view: View,
    ids: Vec<i64>,
}

thread_local! {
    static ST: RefCell<Option<MgrState>> = RefCell::new(None);
    static WIN: RefCell<isize> = RefCell::new(0);
}

unsafe extern "system" fn mgr_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            handle_command(hwnd, id);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn show(mgr: MemoryManager) {
    unsafe {
        let existing = WIN.with(|h| *h.borrow());
        if existing != 0 {
            let hwnd = HWND(existing as *mut _);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            refresh(hwnd);
            return;
        }
        let instance =
            windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let class = w!("PetAgentMemoryWnd");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(mgr_wndproc),
            hInstance: HINSTANCE(instance.0),
            lpszClassName: class,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class,
            w!("Memory Manager"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            240,
            160,
            660,
            480,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
        .unwrap_or_default();
        if hwnd.0.is_null() {
            log_line("memory manager window create failed");
            return;
        }
        WIN.with(|h| *h.borrow_mut() = hwnd.0 as isize);
        ST.with(|s| {
            *s.borrow_mut() = Some(MgrState {
                mgr,
                view: View::Active,
                ids: Vec::new(),
            })
        });
        create_controls(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOW);
        refresh(hwnd);
    }
}

unsafe fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn mk(
    hwnd: HWND,
    class: PCWSTR,
    text: PCWSTR,
    style: u32,
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    ex: u32,
) {
    let inst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
    let _ = CreateWindowExW(
        WINDOW_EX_STYLE(ex),
        class,
        text,
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
        x,
        y,
        w,
        h,
        Some(hwnd),
        Some(HMENU(id as isize as *mut _)),
        Some(HINSTANCE(inst.0)),
        None,
    );
}

unsafe fn create_controls(hwnd: HWND) {
    let b = |s: &str| wide(s);
    let vb = b("已保存");
    let vp = b("待确认");
    let vd = b("已删除");
    let va = b("变更记录");
    mk(
        hwnd,
        w!("BUTTON"),
        PCWSTR(vb.as_ptr()),
        BTN,
        ID_VIEW_ACTIVE,
        10,
        10,
        80,
        26,
        0,
    );
    mk(
        hwnd,
        w!("BUTTON"),
        PCWSTR(vp.as_ptr()),
        BTN,
        ID_VIEW_PENDING,
        95,
        10,
        80,
        26,
        0,
    );
    mk(
        hwnd,
        w!("BUTTON"),
        PCWSTR(vd.as_ptr()),
        BTN,
        ID_VIEW_DELETED,
        180,
        10,
        80,
        26,
        0,
    );
    mk(
        hwnd,
        w!("BUTTON"),
        PCWSTR(va.as_ptr()),
        BTN,
        ID_VIEW_AUDIT,
        265,
        10,
        80,
        26,
        0,
    );
    mk(
        hwnd,
        w!("EDIT"),
        w!(""),
        BORDER | AUTOHSCROLL,
        ID_SEARCH,
        360,
        10,
        160,
        26,
        EDGE,
    );
    mk(
        hwnd,
        w!("COMBOBOX"),
        w!(""),
        CBS_DROPDOWNLIST | VSCROLL,
        ID_KIND,
        530,
        10,
        110,
        200,
        0,
    );
    mk(
        hwnd,
        w!("LISTBOX"),
        w!(""),
        BORDER | VSCROLL | LBS_NOTIFY,
        ID_LIST,
        10,
        45,
        630,
        300,
        EDGE,
    );
    mk(
        hwnd,
        w!("EDIT"),
        w!(""),
        BORDER | AUTOHSCROLL,
        ID_EDIT,
        10,
        352,
        630,
        26,
        EDGE,
    );
    mk(
        hwnd,
        w!("STATIC"),
        w!(""),
        0,
        ID_STATUS,
        10,
        384,
        630,
        20,
        0,
    );
    let btns = [
        (ID_REFRESH, "刷新"),
        (ID_EDIT_SAVE, "保存编辑"),
        (ID_DELETE, "删除"),
        (ID_PIN, "Pin"),
        (ID_UNPIN, "Unpin"),
        (ID_RESTORE, "恢复"),
        (ID_ACCEPT, "接受"),
        (ID_REJECT, "拒绝"),
        (ID_EXPORT, "导出"),
        (ID_BACKUP, "备份"),
    ];
    let mut x = 10;
    for (id, label) in btns {
        let t = b(label);
        mk(
            hwnd,
            w!("BUTTON"),
            PCWSTR(t.as_ptr()),
            BTN,
            id,
            x,
            410,
            60,
            26,
            0,
        );
        x += 64;
    }
    let combo = GetDlgItem(Some(hwnd), ID_KIND).unwrap_or_default();
    for k in [
        "all",
        "fact",
        "preference",
        "project",
        "relationship",
        "custom",
    ] {
        let t = b(k);
        let _ = SendMessageW(
            combo,
            CB_ADDSTRING as u32,
            Some(WPARAM(0)),
            Some(LPARAM(t.as_ptr() as isize)),
        );
    }
    let _ = SendMessageW(combo, CB_SETCURSEL as u32, Some(WPARAM(0)), Some(LPARAM(0)));
}

unsafe fn set_status(hwnd: HWND, text: &str) {
    let c = GetDlgItem(Some(hwnd), ID_STATUS).unwrap_or_default();
    let t = wide(text);
    let _ = SetWindowTextW(c, PCWSTR(t.as_ptr()));
}

unsafe fn get_text(hwnd: HWND, id: i32) -> String {
    let c = GetDlgItem(Some(hwnd), id).unwrap_or_default();
    let len = GetWindowTextLengthW(c);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; (len + 1) as usize];
    let n = GetWindowTextW(c, &mut buf);
    String::from_utf16_lossy(&buf[..n as usize])
}

#[allow(dead_code)]
unsafe fn set_text(hwnd: HWND, id: i32, text: &str) {
    let c = GetDlgItem(Some(hwnd), id).unwrap_or_default();
    let t = wide(text);
    let _ = SetWindowTextW(c, PCWSTR(t.as_ptr()));
}

unsafe fn list_sel(hwnd: HWND) -> Option<i64> {
    let lb = GetDlgItem(Some(hwnd), ID_LIST).unwrap_or_default();
    let idx = SendMessageW(lb, LB_GETCURSEL as u32, Some(WPARAM(0)), Some(LPARAM(0))).0;
    if idx < 0 {
        return None;
    }
    ST.with(|s| {
        s.borrow()
            .as_ref()
            .and_then(|st| st.ids.get(idx as usize).copied())
    })
}

unsafe fn refresh(hwnd: HWND) {
    let lb = GetDlgItem(Some(hwnd), ID_LIST).unwrap_or_default();
    let _ = SendMessageW(lb, LB_RESETCONTENT as u32, Some(WPARAM(0)), Some(LPARAM(0)));
    let search = get_text(hwnd, ID_SEARCH).to_lowercase();
    let combo = GetDlgItem(Some(hwnd), ID_KIND).unwrap_or_default();
    let ki = SendMessageW(combo, CB_GETCURSEL as u32, Some(WPARAM(0)), Some(LPARAM(0))).0;
    let kind = [
        "all",
        "fact",
        "preference",
        "project",
        "relationship",
        "custom",
    ]
    .get(ki as usize)
    .copied()
    .unwrap_or("all");

    ST.with(|s| {
        let mut b = s.borrow_mut();
        let st = match b.as_mut() {
            Some(x) => x,
            None => return,
        };
        st.ids.clear();
        let mut lines: Vec<String> = Vec::new();
        match st.view {
            View::Active => {
                for m in st.mgr.list_active() {
                    if kind != "all" && m.kind != kind {
                        continue;
                    }
                    if !search.is_empty() && !m.content.to_lowercase().contains(&search) {
                        continue;
                    }
                    st.ids.push(m.id);
                    lines.push(format!(
                        "#{} [{}]{} {}",
                        m.id,
                        m.kind,
                        if m.pinned { " [pin]" } else { "" },
                        m.content
                    ));
                }
            }
            View::Pending => {
                for p in st.mgr.list_pending() {
                    if !search.is_empty() && !p.content.to_lowercase().contains(&search) {
                        continue;
                    }
                    st.ids.push(p.id);
                    let tgt = p.target_id.map(|t| format!(" ->#{t}")).unwrap_or_default();
                    lines.push(format!("p#{} [{}]{} {}", p.id, p.action, tgt, p.content));
                }
            }
            View::Deleted => {
                for m in st.mgr.list_deleted() {
                    if !search.is_empty() && !m.content.to_lowercase().contains(&search) {
                        continue;
                    }
                    st.ids.push(m.id);
                    lines.push(format!("#{} [{}] {}", m.id, m.kind, m.content));
                }
            }
            View::Audit => {
                for (ts, action, mid, before, after) in st.mgr.audit_log(200) {
                    st.ids.push(0);
                    lines.push(format!("{ts} {action} #{mid:?} {before:?}->{after:?}"));
                }
            }
        }
        for l in &lines {
            let t = wide(l);
            let _ = SendMessageW(
                lb,
                LB_ADDSTRING as u32,
                Some(WPARAM(0)),
                Some(LPARAM(t.as_ptr() as isize)),
            );
        }
    });
}

unsafe fn handle_command(hwnd: HWND, id: i32) {
    match id {
        ID_VIEW_ACTIVE => set_view(hwnd, View::Active),
        ID_VIEW_PENDING => set_view(hwnd, View::Pending),
        ID_VIEW_DELETED => set_view(hwnd, View::Deleted),
        ID_VIEW_AUDIT => set_view(hwnd, View::Audit),
        ID_REFRESH | ID_SEARCH | ID_KIND => refresh(hwnd),
        ID_DELETE => with_mgr(hwnd, |mgr, sel| {
            if let Some(i) = sel {
                let r = MessageBoxW(
                    Some(hwnd),
                    w!("确定删除这条记忆吗？（soft delete，可恢复）"),
                    w!("确认"),
                    MB_YESNO | MB_ICONQUESTION,
                );
                if r == IDYES {
                    let _ = mgr.soft_delete(i);
                    return Some("已删除（可在「已删除」恢复）".into());
                }
            }
            None
        }),
        ID_PIN => with_mgr(hwnd, |m, s| {
            if let Some(i) = s {
                let _ = m.set_pinned(i, true);
                Some("已 Pin".into())
            } else {
                None
            }
        }),
        ID_UNPIN => with_mgr(hwnd, |m, s| {
            if let Some(i) = s {
                let _ = m.set_pinned(i, false);
                Some("已 Unpin".into())
            } else {
                None
            }
        }),
        ID_RESTORE => with_mgr(hwnd, |m, s| {
            if let Some(i) = s {
                let _ = m.restore(i);
                Some("已恢复".into())
            } else {
                None
            }
        }),
        ID_ACCEPT => with_mgr(hwnd, |m, s| {
            if let Some(i) = s {
                let _ = m.accept(i);
                Some("已接受".into())
            } else {
                None
            }
        }),
        ID_REJECT => with_mgr(hwnd, |m, s| {
            if let Some(i) = s {
                let _ = m.reject(i);
                Some("已拒绝".into())
            } else {
                None
            }
        }),
        ID_EDIT_SAVE => {
            let sel = list_sel(hwnd);
            let text = get_text(hwnd, ID_EDIT);
            if let (Some(i), false) = (sel, text.trim().is_empty()) {
                ST.with(|s| {
                    if let Some(st) = s.borrow().as_ref() {
                        let _ = st.mgr.update_content(i, &text);
                    }
                });
                set_status(hwnd, "已保存编辑 (source=user_ui)");
                refresh(hwnd);
            } else {
                set_status(hwnd, "请选中一条并填写新内容");
            }
        }
        ID_EXPORT => ST.with(|s| {
            if let Some(st) = s.borrow().as_ref() {
                let mut p = std::env::current_exe().unwrap_or_default();
                p.pop();
                p.push("memory_export.json");
                let _ = std::fs::write(&p, st.mgr.export_json());
                set_status(hwnd, &format!("已导出: {}", p.display()));
            }
        }),
        ID_BACKUP => ST.with(|s| {
            if let Some(st) = s.borrow().as_ref() {
                let mut p = std::env::current_exe().unwrap_or_default();
                p.pop();
                p.push("data");
                p.push("memory.db");
                match st.mgr.backup(&p) {
                    Ok(b) => set_status(hwnd, &format!("已备份: {}", b.display())),
                    Err(e) => set_status(hwnd, &format!("备份失败: {e}")),
                }
            }
        }),
        _ => {}
    }
}

unsafe fn set_view(hwnd: HWND, v: View) {
    ST.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.view = v;
        }
    });
    refresh(hwnd);
}

unsafe fn with_mgr<F: FnOnce(&MemoryManager, Option<i64>) -> Option<String>>(hwnd: HWND, f: F) {
    let sel = list_sel(hwnd);
    let msg = ST.with(|s| s.borrow().as_ref().and_then(|st| f(&st.mgr, sel)));
    if let Some(m) = msg {
        set_status(hwnd, &m);
        refresh(hwnd);
    }
}
