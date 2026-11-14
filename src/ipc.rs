//! Связь между процессами: окно настроек / второй запуск → основной процесс (скрытое окно с известным классом).

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const IPC_CLASS: PCWSTR = w!("Flurry.Ipc.v1");
pub const WM_HZ_CMD: u32 = WM_APP + 10;
pub const WM_HZ_STATUS: u32 = WM_APP + 11;

pub mod cmd {
    pub const RELOAD: usize = 1;
    pub const TOGGLE: usize = 2;
    pub const CLEAR: usize = 3;
    pub const NEXT_MODE: usize = 4;
    pub const SETTINGS: usize = 5;
    pub const STORM: usize = 6;
    pub const HK_SUSPEND: usize = 7;
    pub const HK_RESUME: usize = 8;
    pub const EXIT: usize = 9;
    pub const WEATHER_REFRESH: usize = 10;
    pub const WEATHER_READY: usize = 11;
}

/// Биты статуса (ответ на WM_HZ_STATUS).
pub mod st {
    pub const ENABLED: u32 = 1;
    pub const FULLSCREEN: u32 = 2;
    pub const LOCKED: u32 = 4;
    pub const ECO: u32 = 8;
    pub const RUNNING: u32 = 16;
    pub const GPU_FAIL: u32 = 32;
    pub fn fps(v: u32) -> u32 {
        (v >> 8) & 0xFF
    }
    pub fn monitors(v: u32) -> u32 {
        (v >> 16) & 0xFF
    }
}

pub fn main_window() -> Option<HWND> {
    unsafe { FindWindowW(IPC_CLASS, PCWSTR::null()).ok().filter(|h| !h.is_invalid()) }
}

pub fn post(c: usize) -> bool {
    match main_window() {
        Some(h) => unsafe { PostMessageW(h, WM_HZ_CMD, WPARAM(c), LPARAM(0)).is_ok() },
        None => false,
    }
}

pub fn status() -> Option<u32> {
    let h = main_window()?;
    let mut res = 0usize;
    unsafe {
        let r = SendMessageTimeoutW(h, WM_HZ_STATUS, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, 300, Some(&mut res));
        if r.0 == 0 {
            return None;
        }
    }
    Some(res as u32)
}

pub fn main_pid() -> Option<u32> {
    let h = main_window()?;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
