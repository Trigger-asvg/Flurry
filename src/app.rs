//! Основной процесс: трей, горячие клавиши, автозапуск, связь с окном настроек, поток рендера.

use crate::ipc::{self, cmd, wide, IPC_CLASS, WM_HZ_CMD, WM_HZ_STATUS};
use crate::osd::Osd;
use crate::render::{self, Cmd};
use crate::settings::{self, log, Mode, Settings};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_TRAY: u32 = WM_APP + 1;
const TIMER_SEASON: usize = 5;
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Flurry");

struct App {
    hwnd: HWND,
    s: Settings,
    render: render::Handle,
    osd: Osd,
    icon: HICON,
    taskbar_created: u32,
    hk_suspended: bool,
    registered: [bool; 3],
    wx: Option<crate::auto::WeatherState>,
    wx_tx: std::sync::mpsc::Sender<String>,
    last_mode: Mode,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

// --- низкоуровневый хук клавиатуры: подстраховка, если сочетание занято другой программой
static HOOK_KEYS: Mutex<[(u32, u32); 3]> = Mutex::new([(0, 0); 3]);
static HOOK_HWND: AtomicIsize = AtomicIsize::new(0);
static HOOK_OFF: AtomicBool = AtomicBool::new(false);
/// Установленный хук (HHOOK) или 0. Хук ставится только если какое-то сочетание не удалось зарегистрировать.
static HOOK: AtomicIsize = AtomicIsize::new(0);
static LAST_FIRE: Mutex<[u64; 3]> = Mutex::new([0; 3]);

/// Ставит или снимает low-level хук клавиатуры (вызывать из потока с циклом сообщений).
fn set_hook(on: bool) {
    let cur = HOOK.load(Ordering::Relaxed);
    unsafe {
        if on && cur == 0 {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            if let Ok(h) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_hook), HINSTANCE(hinst.0), 0) {
                HOOK.store(h.0 as isize, Ordering::Relaxed);
            }
        } else if !on && cur != 0 {
            let _ = UnhookWindowsHookEx(HHOOK(cur as _));
            HOOK.store(0, Ordering::Relaxed);
        }
    }
}

fn tick_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

/// Защита от двойного срабатывания (RegisterHotKey + хук).
fn debounce(id: usize) -> bool {
    let mut l = LAST_FIRE.lock().unwrap_or_else(|e| e.into_inner());
    let now = tick_ms();
    if id == 0 || id > 3 || now.saturating_sub(l[id - 1]) < 300 {
        return false;
    }
    l[id - 1] = now;
    true
}

unsafe extern "system" fn kb_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && (wp.0 as u32 == WM_KEYDOWN || wp.0 as u32 == WM_SYSKEYDOWN) && !HOOK_OFF.load(Ordering::Relaxed) {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let down = |vk: VIRTUAL_KEY| (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0;
        let mut mods = 0;
        if down(VK_MENU) { mods |= 1; }
        if down(VK_CONTROL) { mods |= 2; }
        if down(VK_SHIFT) { mods |= 4; }
        if down(VK_LWIN) || down(VK_RWIN) { mods |= 8; }
        let keys = *HOOK_KEYS.lock().unwrap_or_else(|e| e.into_inner());
        for (i, (m, vk)) in keys.iter().enumerate() {
            if *vk != 0 && *vk == k.vkCode && *m == mods {
                let h = HWND(HOOK_HWND.load(Ordering::Relaxed) as _);
                let _ = PostMessageW(h, WM_HOTKEY, WPARAM(i + 1), LPARAM(0));
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wp, lp)
}

// ---------------------------------------------------------------- запуск

pub fn run(tray_only: bool) {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CreateMutexW(None, true, w!("Flurry.Main.v1"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if !tray_only {
                ipc::post(cmd::SETTINGS);
            }
            return;
        }
        dark_menus();
    }
    let first_run = !settings::exists();
    let s = settings::load();
    if first_run {
        settings::save(&s);
    }
    sync_autostart(s.autostart);

    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: IPC_CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        // обычное (невидимое) окно верхнего уровня, а не message-only: так приходит TaskbarCreated после перезапуска проводника
        let hwnd = match CreateWindowExW(WS_EX_TOOLWINDOW, IPC_CLASS, w!("Flurry"), WS_POPUP, 0, 0, 0, 0, None, None, HINSTANCE(hinst.0), None) {
            Ok(h) => h,
            Err(e) => {
                log(&format!("ipc window: {e}"));
                return;
            }
        };
        let icon = load_icon();
        let wx = crate::auto::load_wx();
        let (eff, _) = crate::auto::effective(&s, wx.as_ref());
        let render = render::spawn(eff.clone());
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        let wx_tx = spawn_weather(hwnd);
        if s.weather_sync {
            let _ = wx_tx.send(s.city.clone());
        }
        let app = App { hwnd, s, render, osd: Osd::new(), icon, taskbar_created, hk_suspended: false, registered: [false; 3], wx, wx_tx, last_mode: eff.mode };
        APP.with(|a| *a.borrow_mut() = Some(app));
        HOOK_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
        with(|a| {
            a.add_tray();
            a.register_hotkeys();
        });
        SetTimer(hwnd, TIMER_SEASON, 5 * 60 * 1000, None);
        if !tray_only {
            open_settings();
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        set_hook(false);
        let app = APP.with(|a| a.borrow_mut().take());
        if let Some(mut a) = app {
            a.remove_tray();
            a.render.stop();
        }
    }
}

fn load_icon() -> HICON {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let sz = GetSystemMetrics(SM_CXSMICON);
        LoadImageW(HINSTANCE(hinst.0), PCWSTR(1 as _), IMAGE_ICON, sz, sz, LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default()
    }
}

/// Тёмные контекстные меню (недокументированный uxtheme SetPreferredAppMode).
unsafe fn dark_menus() {
    if let Ok(ux) = LoadLibraryW(w!("uxtheme.dll")) {
        if let Some(f) = GetProcAddress(ux, PCSTR_ORD(135)) {
            let f: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            f(1); // AllowDark
        }
        if let Some(f) = GetProcAddress(ux, PCSTR_ORD(136)) {
            let f: unsafe extern "system" fn() = std::mem::transmute(f);
            f();
        }
    }
}

#[allow(non_snake_case)]
fn PCSTR_ORD(n: usize) -> windows::core::PCSTR {
    windows::core::PCSTR(n as *const u8)
}

pub fn exe_path() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn sync_autostart(on: bool) {
    unsafe {
        if on {
            let v = wide(&format!("\"{}\" --tray", exe_path()));
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, REG_SZ.0, Some(v.as_ptr() as _), (v.len() * 2) as u32);
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE);
        }
    }
}

/// Фоновый поток погоды: получает город, обновляет каждые 15 минут.
fn spawn_weather(hwnd: HWND) -> std::sync::mpsc::Sender<String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let h = hwnd.0 as isize;
    let _ = std::thread::Builder::new().name("weather".into()).spawn(move || {
        let mut city = String::new();
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(15 * 60)) {
                Ok(c) => city = c,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return,
            }
            // склеиваем пачку быстрых изменений
            while let Ok(c) = rx.recv_timeout(std::time::Duration::from_millis(600)) {
                city = c;
            }
            if city.trim().is_empty() {
                continue;
            }
            let st = crate::weather::fetch(&city);
            if !st.ok() {
                log(&format!("weather: {}", st.error));
                // временный сбой сети не должен затирать свежие данные по тому же городу
                let keep = crate::auto::load_wx().is_some_and(|old| old.fresh() && crate::auto::same_city(&old.query, &city));
                if keep {
                    continue;
                }
            }
            crate::auto::save_wx(&st);
            unsafe {
                let _ = PostMessageW(HWND(h as _), WM_HZ_CMD, WPARAM(cmd::WEATHER_READY), LPARAM(0));
            }
        }
    });
    tx
}

pub fn open_settings() {
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--settings").spawn();
    }
}

// ---------------------------------------------------------------- логика

impl App {
    fn add_tray(&mut self) {
        unsafe {
            let mut nid = self.nid();
            nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
            nid.uCallbackMessage = WM_TRAY;
            nid.hIcon = self.icon;
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }
        self.update_tip();
    }

    fn nid(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: self.hwnd, uID: 1, ..Default::default() }
    }

    fn update_tip(&self) {
        let tip = format!("Flurry — {}{}", self.last_mode.name(), if self.s.enabled { "" } else { " (выкл.)" });
        let mut nid = self.nid();
        nid.uFlags = NIF_TIP;
        for (i, c) in tip.encode_utf16().take(127).enumerate() {
            nid.szTip[i] = c;
        }
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    fn remove_tray(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid());
        }
    }

    fn register_hotkeys(&mut self) {
        unsafe {
            for id in 1..=3 {
                let _ = UnregisterHotKey(self.hwnd, id);
            }
            // в хук попадают только сочетания, которые не удалось зарегистрировать (заняты другой программой)
            let mut keys = [(0u32, 0u32); 3];
            self.registered = [false; 3];
            if !self.hk_suspended {
                for (id, hk) in self.s.hotkeys() {
                    let i = (id - 1) as usize;
                    if hk.is_set() {
                        self.registered[i] = RegisterHotKey(self.hwnd, id, HOT_KEY_MODIFIERS(hk.mods | 0x4000), hk.vk).is_ok();
                        if !self.registered[i] {
                            keys[i] = (hk.mods, hk.vk);
                            log(&format!("hotkey {} busy, using hook", hk.describe()));
                        }
                    }
                }
            }
            let need_hook = keys.iter().any(|k| k.1 != 0);
            *HOOK_KEYS.lock().unwrap_or_else(|e| e.into_inner()) = keys;
            HOOK_OFF.store(self.hk_suspended, Ordering::Relaxed);
            set_hook(need_hook);
        }
    }

    /// Сообщить потоку погоды город (пустая строка — остановить опрос).
    fn sync_weather_thread(&self) {
        let city = if self.s.weather_sync { self.s.city.clone() } else { String::new() };
        let _ = self.wx_tx.send(city);
    }

    fn push(&mut self, save: bool) {
        if save {
            settings::save(&self.s);
        }
        let (eff, _) = crate::auto::effective(&self.s, self.wx.as_ref());
        self.last_mode = eff.mode;
        self.render.send(Cmd::Settings(Box::new(eff)));
        self.update_tip();
    }

    fn osd(&mut self, text: &str, dot: [u8; 3]) {
        if self.s.show_osd {
            let h = self.hwnd;
            self.osd.show(h, text, dot);
        }
    }

    fn toggle(&mut self) {
        self.s.enabled = !self.s.enabled;
        self.push(true);
        let (t, c) = if self.s.enabled { (format!("{} — включено", self.last_mode.name()), self.last_mode.accent()) } else { ("Эффекты выключены".to_string(), [142, 142, 147]) };
        self.osd(&t, c);
    }

    fn set_mode_quiet(&mut self, m: Mode) {
        self.s.mode = m;
        self.s.auto_season = false;
        self.s.weather_sync = false;
        self.s.enabled = true;
        self.sync_weather_thread();
        self.push(true);
    }

    fn set_mode(&mut self, m: Mode) {
        self.s.mode = m;
        self.s.auto_season = false;
        self.s.weather_sync = false;
        self.s.enabled = true;
        self.sync_weather_thread();
        self.push(true);
        self.osd(m.name(), m.accent());
    }

    fn reload(&mut self) {
        let old = self.s.clone();
        self.s = settings::load();
        if old.hotkeys() != self.s.hotkeys() {
            self.register_hotkeys();
        }
        if old.autostart != self.s.autostart {
            sync_autostart(self.s.autostart);
        }
        if old.weather_sync != self.s.weather_sync || old.city.trim() != self.s.city.trim() {
            self.sync_weather_thread();
        }
        self.push(false);
    }

    fn command(&mut self, c: usize) {
        match c {
            cmd::RELOAD => self.reload(),
            cmd::TOGGLE => self.toggle(),
            cmd::CLEAR => self.render.send(Cmd::Clear),
            cmd::NEXT_MODE => {
                let m = self.last_mode.next();
                self.set_mode(m);
            }
            cmd::SETTINGS => open_settings(),
            cmd::STORM => {
                if !self.s.enabled {
                    self.s.enabled = true;
                    self.push(true);
                }
                self.render.send(Cmd::Storm);
                let t = match self.last_mode {
                    Mode::Snow => "Метель!",
                    Mode::Rain => "Гроза!",
                    Mode::Sakura | Mode::Leaves => "Листопад!",
                    _ => "Буря!",
                };
                let c = self.last_mode.accent();
                self.osd(t, c);
            }
            cmd::WEATHER_REFRESH => self.sync_weather_thread(),
            cmd::WEATHER_READY => {
                self.wx = crate::auto::load_wx();
                self.push(false);
            }
            cmd::HK_SUSPEND => {
                self.hk_suspended = true;
                self.register_hotkeys();
            }
            cmd::HK_RESUME => {
                self.hk_suspended = false;
                self.register_hotkeys();
            }
            cmd::EXIT => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            // из командной строки — без всплывашки (могут вызывать скрипты, пока пользователь в игре)
            c if (100..100 + Mode::ALL.len()).contains(&c) => self.set_mode_quiet(Mode::from_index(c - 100)),
            _ => {}
        }
    }
}

fn tray_menu(hwnd: HWND) {
    let Some((enabled, mode, auto, wsync, has_city)) = with(|a| (a.s.enabled, a.last_mode, a.s.auto_season, a.s.weather_sync, !a.s.city.trim().is_empty())) else { return };
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        let sub = CreatePopupMenu().unwrap_or_default();
        for (i, m) in Mode::ALL.iter().enumerate() {
            let flags = if *m == mode { MF_STRING | MF_CHECKED } else { MF_STRING };
            let t = wide(m.name());
            let _ = AppendMenuW(sub, flags, 100 + i, PCWSTR(t.as_ptr()));
        }
        let _ = AppendMenuW(sub, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(sub, if auto { MF_STRING | MF_CHECKED } else { MF_STRING }, 300, w!("По сезону и времени суток"));
        if has_city {
            let _ = AppendMenuW(sub, if wsync { MF_STRING | MF_CHECKED } else { MF_STRING }, 301, w!("По погоде за окном"));
        }
        let _ = AppendMenuW(menu, MF_STRING | MF_DEFAULT, 1, w!("Настройки…"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, if enabled { MF_STRING | MF_CHECKED } else { MF_STRING }, 2, w!("Эффекты включены"));
        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Режим"));
        let _ = AppendMenuW(menu, MF_STRING, 3, w!("Буря!"));
        let _ = AppendMenuW(menu, MF_STRING, 4, w!("Очистить экран"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, 9, w!("Выход"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let id = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, None).0 as usize;
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match id {
            1 => open_settings(),
            2 => {
                with(|a| a.toggle());
            }
            3 => {
                with(|a| a.command(cmd::STORM));
            }
            4 => {
                with(|a| a.command(cmd::CLEAR));
            }
            9 => {
                let _ = DestroyWindow(hwnd);
            }
            300 => {
                with(|a| {
                    a.s.auto_season = !a.s.auto_season;
                    a.push(true);
                });
            }
            301 => {
                with(|a| {
                    a.s.weather_sync = !a.s.weather_sync;
                    a.sync_weather_thread();
                    a.push(true);
                });
            }
            c if c >= 100 && c < 100 + Mode::ALL.len() => {
                with(|a| a.set_mode(Mode::from_index(c - 100)));
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_HZ_CMD => {
            let c = wp.0;
            if c == cmd::EXIT {
                let _ = DestroyWindow(h);
            } else {
                with(|a| a.command(c));
            }
            LRESULT(0)
        }
        WM_HZ_STATUS => LRESULT(render::STATUS.load(Ordering::Relaxed) as isize),
        WM_HOTKEY => {
            let id = wp.0;
            if debounce(id) {
                match id {
                    1 => {
                        with(|a| a.toggle());
                    }
                    2 => open_settings(),
                    3 => {
                        with(|a| a.command(cmd::NEXT_MODE));
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_TRAY => {
            match lp.0 as u32 {
                WM_LBUTTONUP => open_settings(),
                WM_RBUTTONUP | WM_CONTEXTMENU => tray_menu(h),
                WM_MBUTTONUP => {
                    with(|a| a.toggle());
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wp.0 == crate::osd::TIMER_ID {
                with(|a| a.osd.tick(h));
            } else if wp.0 == TIMER_SEASON {
                // время суток и сезон меняются — пересчитываем эффективные настройки
                with(|a| a.push(false));
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => {
            let tc = with(|a| a.taskbar_created).unwrap_or(0);
            if tc != 0 && m == tc {
                with(|a| a.add_tray());
                return LRESULT(0);
            }
            DefWindowProcW(h, m, wp, lp)
        }
    }
}
