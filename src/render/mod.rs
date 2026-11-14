//! Поток рендера: окна-оверлеи на каждый монитор, симуляция, пауза в полноэкранных приложениях, темп кадров.

pub mod gpu;
pub mod sim;

use crate::ipc::st;
use crate::settings::{log, Mode, Settings};
use gpu::{Gpu, Inst, Target};
use sim::{Env, Field, Rng};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Power::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub enum Cmd {
    Settings(Box<Settings>),
    Clear,
    Storm,
    Exit,
}

pub static STATUS: AtomicU32 = AtomicU32::new(0);
static DISPLAY_CHANGED: AtomicBool = AtomicBool::new(false);

pub struct Handle {
    tx: Sender<Cmd>,
    thread_id: u32,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Handle {
    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_NULL, WPARAM(0), LPARAM(0));
        }
    }
    pub fn stop(&mut self) {
        self.send(Cmd::Exit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

pub fn spawn(s: Settings) -> Handle {
    let (tx, rx) = channel();
    let (idtx, idrx) = channel();
    let join = std::thread::Builder::new()
        .name("render".into())
        .spawn(move || {
            let _ = idtx.send(unsafe { GetCurrentThreadId() });
            run(rx, s);
        })
        .expect("render thread");
    let thread_id = idrx.recv().unwrap_or(0);
    Handle { tx, thread_id, join: Some(join) }
}

// ---------------------------------------------------------------- окна

const CLASS: PCWSTR = w!("Flurry.Overlay");

unsafe extern "system" fn overlay_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_NCHITTEST => LRESULT(-1), // HTTRANSPARENT
        WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE
        WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE => {
            DISPLAY_CHANGED.store(true, Ordering::Relaxed);
            if m == WM_DPICHANGED {
                return LRESULT(0);
            }
            DefWindowProcW(h, m, wp, lp)
        }
        _ => DefWindowProcW(h, m, wp, lp),
    }
}

fn register_class() {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_proc),
            hInstance: hinst.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct MonInfo {
    rect: RECT,
    work: RECT,
    scale: f32,
    primary: bool,
}

fn enum_monitors() -> Vec<MonInfo> {
    unsafe extern "system" fn cb(hm: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
        let v = &mut *(lp.0 as *mut Vec<MonInfo>);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(hm, &mut mi).as_bool() {
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(hm, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            v.push(MonInfo { rect: mi.rcMonitor, work: mi.rcWork, scale: dx as f32 / 96.0, primary: mi.dwFlags & 1 != 0 });
        }
        TRUE
    }
    let mut v: Vec<MonInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(HDC::default(), None, Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v.sort_by_key(|m| (!m.primary, m.rect.left, m.rect.top));
    v
}

struct Mon {
    info: MonInfo,
    hwnd: HWND,
    target: Option<Target>,
    field: Field,
    paused: bool,
    shown: bool,
}

impl Mon {
    fn new(gpu: &Gpu, info: MonInfo, seed: u64, capture_hidden: bool) -> Option<Mon> {
        let r = info.rect;
        let (w, h) = ((r.right - r.left) as u32, (r.bottom - r.top) as u32);
        unsafe {
            let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP;
            let hwnd = CreateWindowExW(
                ex,
                CLASS,
                w!("Flurry overlay"),
                WS_POPUP,
                r.left,
                r.top,
                w as i32,
                h as i32,
                None,
                None,
                HINSTANCE(GetModuleHandleW(None).unwrap_or_default().0),
                None,
            )
            .ok()?;
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            if capture_hidden {
                let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
            }
            let target = match gpu.target(hwnd, w, h) {
                Ok(t) => t,
                Err(e) => {
                    log(&format!("swapchain: {e}"));
                    let _ = DestroyWindow(hwnd);
                    return None;
                }
            };
            let ground = (info.work.bottom - r.top) as f32;
            Some(Mon { info, hwnd, target: Some(target), field: Field::new(w as f32, h as f32, ground, info.scale, seed), paused: false, shown: false })
        }
    }

    fn hide(&mut self) {
        if self.shown {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
        }
    }
}

impl Drop for Mon {
    fn drop(&mut self) {
        self.target = None;
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

// ---------------------------------------------------------------- состояние системы

fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Прямоугольник окна переднего плана, если оно «полноэкранное» (не рабочий стол/панель задач).
fn foreground_rect(ours: &[HWND]) -> Option<RECT> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || ours.contains(&fg) {
            return None;
        }
        let cls = class_name(fg);
        if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "XamlExplorerHostIslandWindow" | "Windows.UI.Core.CoreWindow") {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(fg, &mut r).ok()?;
        Some(r)
    }
}

fn covers(r: &RECT, m: &RECT) -> bool {
    r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
}

fn on_battery() -> bool {
    let mut ps = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut ps).is_ok() && ps.ACLineStatus == 0 }
}

fn notification_state() -> i32 {
    unsafe { SHQueryUserNotificationState().map(|s| s.0).unwrap_or(5) }
}

// ---------------------------------------------------------------- темп кадров

type WaitClock = unsafe extern "system" fn(u32, *const HANDLE, u32) -> u32;

struct Pacer {
    timer: HANDLE,
    clock: Option<WaitClock>,
}

impl Pacer {
    fn new() -> Pacer {
        unsafe {
            let timer = CreateWaitableTimerExW(None, PCWSTR::null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0)
                .or_else(|_| CreateWaitableTimerExW(None, PCWSTR::null(), 0, TIMER_ALL_ACCESS.0))
                .unwrap_or_default();
            // DCompositionWaitForCompositorClock (Windows 11): просыпаемся ровно на такт композитора — без рывков
            let no_clock = std::env::var_os("HZ_NO_CLOCK").is_some();
            let clock = GetModuleHandleW(w!("dcomp.dll"))
                .ok()
                .filter(|_| !no_clock)
                .and_then(|m| GetProcAddress(m, windows::core::s!("DCompositionWaitForCompositorClock")))
                .map(|f| std::mem::transmute::<_, WaitClock>(f));
            Pacer { timer, clock }
        }
    }

    /// Ждёт до `until` (секунды от старта), просыпаясь также на сообщения окна.
    fn wait(&self, start: Instant, until: f64) {
        let now = start.elapsed().as_secs_f64();
        let left = until - now;
        if left <= 0.0005 {
            return;
        }
        unsafe {
            if let Some(clock) = self.clock {
                // ждём такты композитора, пока не подойдёт время кадра
                loop {
                    let left = until - start.elapsed().as_secs_f64();
                    if left <= 0.002 {
                        return;
                    }
                    let r = clock(0, std::ptr::null(), (left * 1000.0).ceil() as u32 + 1);
                    if r == 0x102 {
                        return; // WAIT_TIMEOUT
                    }
                    let left = until - start.elapsed().as_secs_f64();
                    if left <= 0.004 {
                        return;
                    }
                }
            }
            let due = -((left * 10_000_000.0) as i64);
            if !self.timer.is_invalid() && SetWaitableTimer(self.timer, &due, 0, None, None, false).is_ok() {
                MsgWaitForMultipleObjectsEx(Some(&[self.timer]), 100, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            } else {
                std::thread::sleep(std::time::Duration::from_secs_f64(left));
            }
        }
    }
}

// ---------------------------------------------------------------- цикл

/// «Авто» FPS: 60, и только если все частицы ползут совсем медленно (≤1.2 px за кадр при 30) — 30.
/// Переключение с задержкой, чтобы частота не «прыгала» туда-сюда.
fn auto_fps(s: &Settings, wind: (f32, f32), storm: f32, mouse_v: (f32, f32), mons: &[Mon]) -> u32 {
    if storm > 0.0 || mons.iter().any(|m| m.field.busy()) {
        return 60;
    }
    if (mouse_v.0 * mouse_v.0 + mouse_v.1 * mouse_v.1).sqrt() > 40.0 && s.mouse != crate::settings::MouseMode::Off {
        return 60;
    }
    let fall = match s.mode {
        Mode::Rain | Mode::Confetti | Mode::Butterflies => return 60,
        Mode::Snow => 105.0,
        Mode::Sakura => 70.0,
        Mode::Leaves => 100.0,
        Mode::Fireflies => 32.0,
        Mode::Bubbles => 70.0,
        Mode::Dandelions => 22.0,
    };
    let k = mons.iter().map(|m| m.info.scale).fold(1.0f32, f32::max);
    let w = (wind.0 * wind.0 + wind.1 * wind.1).sqrt() * 1.6;
    let px_s = (fall * s.speed + w * 1.25 + 30.0 * s.turbulence) * k;
    if px_s <= 36.0 { 30 } else { 60 }
}

fn pump() -> bool {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return false;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}

fn wind_vec(s: &Settings, gust: f32) -> (f32, f32) {
    let a = s.wind_angle.to_radians();
    let mag = s.wind * 320.0 * gust;
    (a.sin() * mag, -a.cos() * mag)
}

fn run(rx: Receiver<Cmd>, mut s: Settings) {
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    register_class();
    let start = Instant::now();
    let pacer = Pacer::new();
    let mut gpu: Option<Gpu> = None;
    let mut gpu_fail_until = 0.0f64;
    let mut mons: Vec<Mon> = Vec::new();
    let mut infos: Vec<MonInfo> = Vec::new();
    let mut inst: Vec<Inst> = Vec::with_capacity(4096);
    let mut rng = Rng::new(0x5eed);

    let mut last = 0.0f64;
    let mut next_frame = 0.0f64;
    let mut next_house = 0.0f64;
    let mut next_top = 0.0f64;
    let mut fade = 0.0f32; // общее появление/исчезновение
    let mut storm_until = 0.0f64;
    let mut flash = 0.0f32;
    let mut next_lightning = 6.0f64;
    let mut locked = false;
    let mut global_pause = false;
    let mut battery = false;
    let mut frames = 0u32;
    let mut fps_t = 0.0f64;
    let mut fps = 0u32;
    let mut prev_mouse: Option<(i32, i32)> = None;
    let mut mouse_v = (0.0f32, 0.0f32);
    let mut rebuild = true;
    let mut sleeping = false;
    let mut next_trim = 4.0f64;
    let mut frame_no = 0u32;
    let mut hitches = 0u32;
    let mut auto_cur = 60u32;
    let mut auto_since = 0.0f64;
    let mut bg_flash_prev = false;
    let profile = std::env::var_os("HZ_PROFILE").is_some();
    let mut prof = [0.0f64; 4];

    'outer: loop {
        if !pump() {
            break;
        }
        while let Ok(c) = rx.try_recv() {
            match c {
                Cmd::Exit => break 'outer,
                Cmd::Settings(ns) => {
                    if ns.primary_only != s.primary_only || ns.hide_from_capture != s.hide_from_capture {
                        rebuild = true;
                    }
                    s = *ns;
                }
                Cmd::Clear => {
                    for m in &mut mons {
                        m.field.clear();
                    }
                }
                Cmd::Storm => {
                    storm_until = start.elapsed().as_secs_f64() + 9.0;
                    if s.mode == Mode::Rain && s.lightning {
                        next_lightning = start.elapsed().as_secs_f64() + 0.5;
                    }
                }
            }
        }
        let now = start.elapsed().as_secs_f64();

        // --- раз в секунду: мониторы, полноэкранные приложения, блокировка, батарея
        if now >= next_house || DISPLAY_CHANGED.swap(false, Ordering::Relaxed) {
            next_house = now + 1.0;
            let ns = notification_state();
            locked = ns == 1; // QUNS_NOT_PRESENT — экран заблокирован / нет пользователя
            global_pause = s.pause_fullscreen && (ns == 3 || ns == 4); // D3D fullscreen, презентация
            battery = s.battery_saver && on_battery();
            let mut cur = enum_monitors();
            if s.primary_only {
                cur.retain(|m| m.primary);
            }
            if cur != infos {
                infos = cur;
                rebuild = true;
            }
            let ours: Vec<HWND> = mons.iter().map(|m| m.hwnd).collect();
            let fg = if s.pause_fullscreen { foreground_rect(&ours) } else { None };
            for m in &mut mons {
                m.paused = fg.map(|r| covers(&r, &m.info.rect)).unwrap_or(false);
            }
        }

        let want = s.enabled && !locked && !global_pause;
        if want {
            fade = (fade + ((now - last).min(0.1) as f32) / 0.5).min(1.0);
        } else {
            fade = (fade - ((now - last).min(0.1) as f32) / 0.4).max(0.0);
        }
        let active = want || fade > 0.0;

        // --- статус для окна настроек
        let mut bits = 0u32;
        if s.enabled { bits |= st::ENABLED; }
        if global_pause || mons.iter().any(|m| m.paused) { bits |= st::FULLSCREEN; }
        if locked { bits |= st::LOCKED; }
        if battery { bits |= st::ECO; }
        if active && gpu.is_some() { bits |= st::RUNNING; }
        if gpu.is_none() && gpu_fail_until > now { bits |= st::GPU_FAIL; }
        bits |= (fps.min(255)) << 8;
        bits |= (mons.len() as u32).min(255) << 16;
        STATUS.store(bits, Ordering::Relaxed);

        if !active {
            // полный покой: окна скрыты, ресурсы GPU отпущены, поток спит до сообщения
            if !mons.is_empty() || gpu.is_some() {
                mons.clear();
                if let Some(g) = &gpu {
                    g.trim();
                }
                gpu = None;
                rebuild = true;
                unsafe {
                    let _ = windows::Win32::System::ProcessStatus::EmptyWorkingSet(GetCurrentProcess());
                }
            }
            sleeping = true;
            fps = 0;
            last = now;
            unsafe {
                MsgWaitForMultipleObjectsEx(None, 1000, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            }
            continue;
        }
        if sleeping {
            sleeping = false;
            last = now;
        }

        if gpu.is_none() {
            if now < gpu_fail_until {
                unsafe { MsgWaitForMultipleObjectsEx(None, 500, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
                continue;
            }
            match Gpu::new() {
                Ok(g) => {
                    if g.warp {
                        log("hardware D3D11 unavailable, using WARP");
                    }
                    gpu = Some(g);
                    rebuild = true;
                    next_trim = now + 4.0;
                }
                Err(e) => {
                    log(&format!("gpu init: {e}"));
                    gpu_fail_until = now + 5.0;
                    continue;
                }
            }
        }
        if rebuild {
            rebuild = false;
            mons.clear();
            if infos.is_empty() {
                infos = enum_monitors();
                if s.primary_only {
                    infos.retain(|m| m.primary);
                }
            }
            let g = gpu.as_ref().unwrap();
            for (i, info) in infos.iter().enumerate() {
                if let Some(m) = Mon::new(g, *info, 0x1234 + i as u64 * 7919 + now as u64, s.hide_from_capture) {
                    mons.push(m);
                }
            }
            next_house = 0.0; // пересчитать паузы
        }

        // --- время кадра
        let gap = now - last;
        if gap > prof[3] { prof[3] = gap; }
        if gap > 0.030 { hitches += 1; }
        let dt = ((now - last) as f32).clamp(0.0, 0.05);
        last = now;
        // время для анимаций — f32; без «заворачивания» точность падает через несколько суток работы
        // (на 10-е сутки шаг f32 ≈ 60 мс — покачивание и мерцание начинают дёргаться)
        let t = (now % 3600.0) as f32;

        // мышь
        let mut cur = POINT::default();
        let mouse = unsafe { GetCursorPos(&mut cur).is_ok() }.then_some((cur.x, cur.y));
        if let (Some((x, y)), Some((px, py))) = (mouse, prev_mouse) {
            if dt > 0.0 {
                let a = (dt * 12.0).min(1.0);
                mouse_v.0 += (((x - px) as f32 / dt) - mouse_v.0) * a;
                mouse_v.1 += (((y - py) as f32 / dt) - mouse_v.1) * a;
            }
        }
        prev_mouse = mouse;

        // порывы ветра и буря
        let storm = if now < storm_until { (((storm_until - now) as f32).min(1.5) / 1.5).min(((9.0 - (storm_until - now)) as f32 * 1.5).clamp(0.0, 1.0)) } else { 0.0 };
        let gust = if s.gusts {
            1.0 + 0.55 * ((t * 0.21).sin() * (t * 0.67 + 1.3).sin()).max(-0.4) + 0.25 * (t * 1.9).sin() * (t * 0.31).sin()
        } else {
            1.0
        };
        let mut wind = wind_vec(&s, gust);
        if storm > 0.0 {
            let a = s.wind_angle.to_radians();
            let extra = 380.0 * storm * (1.0 + 0.4 * (t * 2.3).sin());
            let (sx, sy) = if s.wind < 0.02 { (1.0, 0.0) } else { (a.sin(), -a.cos()) };
            wind.0 += sx * extra;
            wind.1 += sy * extra;
        }

        // молния
        flash = (flash - dt * 3.5).max(0.0);
        let rain_mode = s.mode == Mode::Rain;
        if rain_mode && s.lightning && now >= next_lightning {
            flash = rng.r(0.10, 0.22);
            next_lightning = now + rng.r(7.0, 22.0) as f64 * if storm > 0.0 { 0.3 } else { 1.0 };
            // двойная вспышка
            if rng.f() < 0.5 {
                next_lightning = next_lightning.min(now + 0.18);
            }
        }
        if !rain_mode {
            next_lightning = next_lightning.max(now + 3.0);
        }

        let g = gpu.as_mut().unwrap();
        let mut failed = false;
        for (i, m) in mons.iter_mut().enumerate() {
            if m.paused {
                m.hide();
                continue;
            }
            let r = m.info.rect;
            let local_mouse = mouse.and_then(|(x, y)| {
                let inside = x >= r.left - 50 && x < r.right + 50 && y >= r.top - 50 && y < r.bottom + 50;
                inside.then(|| ((x - r.left) as f32, (y - r.top) as f32, mouse_v.0, mouse_v.1))
            });
            let env = Env { s: &s, dt, time: t, wind, storm, flash, mouse: local_mouse };
            let q0 = Instant::now();
            m.field.update(&env);
            inst.clear();
            m.field.emit(&env, &mut inst);
            let op = s.opacity * fade * fade * (3.0 - 2.0 * fade);
            prof[0] += q0.elapsed().as_secs_f64();
            let q1 = Instant::now();
            if let Some(tg) = m.target.as_mut() {
                if let Err(e) = g.render(tg, &inst, t + i as f32 * 3.7, op) {
                    log(&format!("present: {e}"));
                    failed = true;
                    break;
                }
                // туман и молнии — отдельный крошечный слой, обновляется ~15 раз в секунду (вспышки — каждый кадр)
                if frame_no & 3 == 0 || flash > 0.0 || bg_flash_prev {
                    inst.clear();
                    m.field.emit_bg(&env, gpu::BG_DIV as f32, &mut inst);
                    if let Err(e) = g.render_bg(tg, &inst, t, op) {
                        log(&format!("present bg: {e}"));
                        failed = true;
                        break;
                    }
                }
            }
            prof[1] += q1.elapsed().as_secs_f64();
            if !m.shown {
                unsafe {
                    let _ = ShowWindow(m.hwnd, SW_SHOWNOACTIVATE);
                    let _ = SetWindowPos(m.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
                m.shown = true;
            }
        }
        if failed {
            // устройство потеряно (обновление драйвера и т.п.) — пересоздаём всё
            mons.clear();
            gpu = None;
            gpu_fail_until = now + 1.0;
            continue;
        }
        if now >= next_top {
            next_top = now + 2.0;
            for m in &mons {
                if m.shown {
                    unsafe {
                        let _ = SetWindowPos(m.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
                    }
                }
            }
        }

        frames += 1;
        frame_no = frame_no.wrapping_add(1);
        bg_flash_prev = flash > 0.0;
        if now - fps_t >= 1.0 {
            if profile {
                log(&format!("prof fps={} sim={:.2}ms render={:.2}ms wait={:.1}ms max_gap={:.1}ms hitches={} inst={}", frames, prof[0] * 1000.0 / frames as f64, prof[1] * 1000.0 / frames as f64, prof[2] * 1000.0 / frames as f64, prof[3] * 1000.0, hitches, inst.len()));
                hitches = 0;
                prof = [0.0; 4];
            }
            fps = (frames as f64 / (now - fps_t)).round() as u32;
            frames = 0;
            fps_t = now;
        }

        // --- следующий кадр
        let target_fps = if battery {
            30
        } else if s.fps == 0 {
            let want_fps = auto_fps(&s, wind, storm, mouse_v, &mons);
            if want_fps > auto_cur {
                auto_cur = want_fps;
                auto_since = now;
            } else if want_fps < auto_cur && now - auto_since > 4.0 {
                auto_cur = want_fps;
            } else if want_fps == auto_cur {
                auto_since = now;
            }
            auto_cur
        } else {
            s.fps
        };
        if now >= next_trim {
            // драйвер и загрузчик оставляют много однажды тронутых страниц — отдаём их системе
            next_trim = f64::INFINITY;
            unsafe {
                let _ = windows::Win32::System::ProcessStatus::EmptyWorkingSet(GetCurrentProcess());
            }
        }
        let period = 1.0 / target_fps as f64;
        next_frame += period;
        let now2 = start.elapsed().as_secs_f64();
        if next_frame < now2 - period {
            next_frame = now2; // отстали — не догоняем пачкой
        }
        if mons.iter().all(|m| m.paused) {
            next_frame = now2 + 0.25;
        }
        let qw = Instant::now();
        pacer.wait(start, next_frame - 0.001);
        prof[2] += qw.elapsed().as_secs_f64();
    }
    mons.clear();
}
