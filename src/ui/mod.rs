//! Окно настроек (отдельный процесс `--settings`, закрывается — память освобождается полностью).

mod widgets;

use crate::ipc::{self, cmd, st};
use crate::settings::{self, Hotkey, Mode, MouseMode, Settings, Theme, Tint};
use eframe::egui::{self, Color32, Rect, Rounding, Sense, Stroke, Ui, UiBuilder};
use egui::{pos2, vec2, Align2};
use widgets::*;

pub const TITLE: &str = "Flurry — Настройки";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Main,
    Particles,
    Wind,
    Mouse,
    Effects,
    Performance,
    Hotkeys,
    General,
    About,
    Weather,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Page::Main => "Настройки",
            Page::Particles => "Частицы",
            Page::Wind => "Ветер",
            Page::Mouse => "Мышь",
            Page::Effects => "Эффекты",
            Page::Performance => "Производительность",
            Page::Hotkeys => "Горячие клавиши",
            Page::General => "Основные",
            Page::About => "О программе",
            Page::Weather => "Погода и время",
        }
    }
    fn parse(s: &str) -> Option<Page> {
        Some(match s {
            "particles" => Page::Particles,
            "wind" => Page::Wind,
            "mouse" => Page::Mouse,
            "effects" => Page::Effects,
            "performance" => Page::Performance,
            "hotkeys" => Page::Hotkeys,
            "general" => Page::General,
            "about" => Page::About,
            "weather" => Page::Weather,
            _ => return None,
        })
    }
}

/// Переход между страницами: p = 0 — верхняя страница на месте, 1 — уехала вправо.
struct Anim {
    p_from: f32,
    p_to: f32,
    start: f64,
    dur: f32,
}

#[derive(Default)]
struct ProcStats {
    pid: u32,
    last_cpu: u64,
    last_t: f64,
    cpu: f32,
    ws_mb: f32,
    private_mb: f32,
}

struct SettingsApp {
    s: Settings,
    dirty_at: Option<f64>,
    last_save: f64,
    file_mtime: Option<std::time::SystemTime>,
    next_poll: f64,
    stack: Vec<Page>,
    anim: Option<Anim>,
    swipe: Option<f32>,
    status: Option<u32>,
    stats: ProcStats,
    capturing: Option<usize>,
    reset_armed: f64,
    hero_mode: (Mode, Mode, f64),
    dial_shown: f32,
    themed_hwnd: Option<bool>,
    pal: Pal,
    demo: bool,
    demo_step: u32,
    profile: bool,
    prof: (f32, f32, u32, f64, f64, f64),
    wx: Option<crate::auto::WeatherState>,
    city_edit: String,
    /// Тема системы (читается из реестра раз в секунду в poll(), а не каждый кадр)
    sys_dark: bool,
}

pub fn run(page: Option<String>) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::*;
        let t = ipc::wide(TITLE);
        if let Ok(h) = FindWindowW(windows::core::PCWSTR::null(), windows::core::PCWSTR(t.as_ptr())) {
            if !h.is_invalid() {
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
                return;
            }
        }
    }
    let icon = {
        let n = 64usize;
        let mut rgba = vec![0u8; n * n * 4];
        for y in 0..n {
            for x in 0..n {
                let i = (y * n + x) * 4;
                let inside = (4..n - 4).contains(&x) && (4..n - 4).contains(&y);
                if inside {
                    let edge = x == 4 || y == 4 || x == n - 5 || y == n - 5;
                    let v = if edge { 0x3a } else { 0 };
                    rgba[i..i + 4].copy_from_slice(&[v, v, v, 255]);
                }
            }
        }
        egui::IconData { rgba, width: n as u32, height: n as u32 }
    };
    let mut vp = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_inner_size([430.0, 800.0])
        .with_min_inner_size([380.0, 520.0])
        .with_icon(icon);
    // для скриншотов/тестов: HZ_UI_POS="x,y" — открыть в точке и не забирать фокус
    let test_pos = std::env::var("HZ_UI_POS").ok().and_then(|v| {
        let mut it = v.split(',').filter_map(|x| x.trim().parse::<f32>().ok());
        Some((it.next()?, it.next()?))
    });
    if let Some((x, y)) = test_pos {
        vp = vp.with_position([x, y]).with_active(false).with_inner_size([430.0, 760.0]);
    }
    let options = eframe::NativeOptions {
        viewport: vp,
        centered: test_pos.is_none(),
        vsync: true,
        ..Default::default()
    };
    let start_page = page.as_deref().and_then(Page::parse);
    let _ = eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::new(start_page)))
        }),
    );
}

fn setup_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()) + "\\Fonts\\";
    let mut fallback = vec![];
    for (name, file) in [("segoe", "segoeui.ttf"), ("segoe_sb", "seguisb.ttf"), ("segoe_b", "segoeuib.ttf"), ("segoe_sym", "seguisym.ttf")] {
        if let Ok(b) = std::fs::read(format!("{dir}{file}")) {
            fonts.font_data.insert(name.into(), FontData::from_owned(b));
        }
    }
    let has = |f: &FontDefinitions, n: &str| f.font_data.contains_key(n);
    if has(&fonts, "segoe_sym") {
        fallback.push("segoe_sym".to_string());
    }
    fallback.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    let mk = |first: &str, f: &FontDefinitions| -> Vec<String> {
        let mut v = vec![];
        if has(f, first) {
            v.push(first.to_string());
        }
        if has(f, "segoe") && first != "segoe" {
            v.push("segoe".into());
        }
        v.extend(fallback.iter().cloned());
        v
    };
    let prop = mk("segoe", &fonts);
    let sb = mk("segoe_sb", &fonts);
    let b = mk("segoe_b", &fonts);
    fonts.families.insert(FontFamily::Proportional, prop);
    fonts.families.insert(FontFamily::Name("sb".into()), sb);
    fonts.families.insert(FontFamily::Name("bold".into()), b);
    ctx.set_fonts(fonts);
    ctx.style_mut(|s| {
        s.interaction.selectable_labels = false;
        s.spacing.scroll = egui::style::ScrollStyle::floating();
        s.spacing.item_spacing = vec2(0.0, 0.0);
        s.animation_time = 0.2;
    });
}

fn system_dark() -> bool {
    use windows::Win32::System::Registry::*;
    let mut v = 1u32;
    let mut sz = 4u32;
    unsafe {
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            windows::core::w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut _ as _),
            Some(&mut sz),
        );
        r.is_ok() && v == 0
    }
}

impl SettingsApp {
    fn new(page: Option<Page>) -> Self {
        let s = settings::load();
        let mut stack = vec![Page::Main];
        if let Some(p) = page {
            stack.push(p);
        }
        let sys_dark = system_dark();
        let dark = match s.theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => sys_dark,
        };
        let m = s.mode;
        SettingsApp {
            s,
            dirty_at: None,
            last_save: 0.0,
            file_mtime: settings::modified(),
            next_poll: 0.0,
            stack,
            anim: None,
            swipe: None,
            status: ipc::status(),
            stats: ProcStats::default(),
            capturing: None,
            reset_armed: -10.0,
            hero_mode: (m, m, -10.0),
            dial_shown: f32::NAN,
            themed_hwnd: None,
            pal: Pal::new(dark),
            demo: std::env::var_os("HZ_UI_DEMO").is_some(),
            demo_step: 0,
            profile: std::env::var_os("HZ_PROFILE").is_some(),
            wx: crate::auto::load_wx(),
            city_edit: String::new(),
            sys_dark,
            prof: (0.0, 0.0, 0, 0.0, 0.0, 0.0),
        }
    }

    fn changed(&mut self, now: f64) {
        self.dirty_at.get_or_insert(now);
    }

    fn flush(&mut self, now: f64, force: bool) {
        if self.dirty_at.is_some() && (force || now - self.last_save > 0.08) {
            self.s.sanitize();
            settings::save(&self.s);
            self.file_mtime = settings::modified();
            self.last_save = now;
            self.dirty_at = None;
            if !ipc::post(cmd::RELOAD) {
                self.status = None;
            }
        }
    }

    fn poll(&mut self, now: f64) {
        if now < self.next_poll {
            return;
        }
        self.next_poll = now + 1.0;
        self.status = ipc::status();
        // настройки поменяли снаружи (горячая клавиша, трей)
        let m = settings::modified();
        if m != self.file_mtime && self.dirty_at.is_none() {
            self.file_mtime = m;
            let ns = settings::load();
            if ns != self.s {
                self.s = ns;
            }
        }
        self.wx = crate::auto::load_wx();
        self.sys_dark = system_dark();
        self.update_stats(now);
    }

    fn update_stats(&mut self, now: f64) {
        use windows::Win32::Foundation::*;
        use windows::Win32::System::ProcessStatus::*;
        use windows::Win32::System::Threading::*;
        let Some(pid) = ipc::main_pid() else {
            self.stats = ProcStats::default();
            return;
        };
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return };
            let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok() {
                let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
                let cpu = ft(k) + ft(u);
                if self.stats.pid == pid && self.stats.last_t > 0.0 {
                    let dt = (now - self.stats.last_t).max(0.1);
                    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
                    let pct = (cpu.saturating_sub(self.stats.last_cpu)) as f64 / 1e7 / dt / n * 100.0;
                    self.stats.cpu = if self.stats.cpu == 0.0 { pct as f32 } else { self.stats.cpu * 0.5 + pct as f32 * 0.5 };
                }
                self.stats.pid = pid;
                self.stats.last_cpu = cpu;
                self.stats.last_t = now;
            }
            let mut mc = PROCESS_MEMORY_COUNTERS_EX::default();
            if GetProcessMemoryInfo(h, &mut mc as *mut _ as *mut PROCESS_MEMORY_COUNTERS, std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32).is_ok() {
                self.stats.ws_mb = mc.WorkingSetSize as f32 / 1048576.0;
                self.stats.private_mb = mc.PrivateUsage as f32 / 1048576.0;
            }
            let _ = CloseHandle(h);
        }
    }

    fn apply_window_theme(&mut self) {
        if self.themed_hwnd == Some(self.pal.dark) {
            return;
        }
        use windows::Win32::Graphics::Dwm::*;
        use windows::Win32::UI::WindowsAndMessaging::*;
        unsafe {
            let t = ipc::wide(TITLE);
            let Ok(h) = FindWindowW(windows::core::PCWSTR::null(), windows::core::PCWSTR(t.as_ptr())) else { return };
            if h.is_invalid() {
                return;
            }
            let dark: i32 = self.pal.dark as i32;
            let _ = DwmSetWindowAttribute(h, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _, 4);
            let bg = self.pal.bg;
            let cref: u32 = bg.r() as u32 | (bg.g() as u32) << 8 | (bg.b() as u32) << 16;
            let _ = DwmSetWindowAttribute(h, DWMWA_CAPTION_COLOR, &cref as *const _ as _, 4);
            let _ = DwmSetWindowAttribute(h, DWMWA_BORDER_COLOR, &cref as *const _ as _, 4);
            let corner: i32 = 2; // DWMWCP_ROUND
            let _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const _ as _, 4);
        }
        self.themed_hwnd = Some(self.pal.dark);
    }

    fn push(&mut self, p: Page, now: f64) {
        if self.anim.is_some() || self.swipe.is_some() {
            return;
        }
        self.stack.push(p);
        self.anim = Some(Anim { p_from: 1.0, p_to: 0.0, start: now, dur: 0.42 });
    }

    fn pop(&mut self, now: f64, from: f32) {
        if self.stack.len() < 2 || (self.anim.is_some() && self.swipe.is_none()) {
            return;
        }
        self.end_capture();
        self.swipe = None;
        self.anim = Some(Anim { p_from: from, p_to: 1.0, start: now, dur: 0.38 * (1.0 - from).max(0.4) });
    }

    fn end_capture(&mut self) {
        if self.capturing.take().is_some() {
            ipc::post(cmd::HK_RESUME);
        }
    }
}

// ---------------------------------------------------------------- ряды карточек

struct Rows<'a> {
    ui: &'a mut Ui,
    pal: Pal,
    n: usize,
    w: f32,
}

impl<'a> Rows<'a> {
    fn row(&mut self, h: f32, sense: Sense, inset: f32) -> (Rect, egui::Response) {
        let (rect, resp) = self.ui.allocate_exact_size(vec2(self.w, h), sense);
        if self.n > 0 {
            self.ui.painter().line_segment([pos2(rect.left() + inset, rect.top()), pos2(rect.right(), rect.top())], Stroke::new(0.6_f32, self.pal.sep));
        }
        self.n += 1;
        if sense.click {
            let down = resp.is_pointer_button_down_on();
            let a = self.ui.ctx().animate_bool_with_time(resp.id.with("hl"), down, if down { 0.05 } else { 0.35 });
            if a > 0.0 {
                self.ui.painter().rect_filled(rect.shrink2(vec2(0.0, 0.5)), Rounding::same(10.0), alpha(self.pal.press, a));
            }
        }
        (rect, resp)
    }

    fn label(&self, rect: &Rect, x: f32, title: &str, sub: Option<&str>) {
        let p = self.ui.painter();
        if let Some(sub) = sub {
            p.text(pos2(x, rect.center().y - 9.0), Align2::LEFT_CENTER, title, font(15.0), self.pal.text);
            p.text(pos2(x, rect.center().y + 10.0), Align2::LEFT_CENTER, sub, font(12.0), self.pal.text2);
        } else {
            p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, title, font(15.0), self.pal.text);
        }
    }

    fn nav(&mut self, ic: Icon, bg: Color32, title: &str, value: &str) -> bool {
        let (rect, resp) = self.row(48.0, Sense::click(), 58.0);
        let p = self.ui.painter();
        let ir = Rect::from_center_size(pos2(rect.left() + 30.0, rect.center().y), vec2(30.0, 30.0));
        p.rect_filled(ir, Rounding::same(7.5), bg);
        icon(p, ir.center(), 22.0, ic, Color32::WHITE);
        self.label(&rect, rect.left() + 58.0, title, None);
        let chev = pos2(rect.right() - 18.0, rect.center().y);
        p.add(egui::Shape::line(vec![chev + vec2(-3.5, -6.5), chev + vec2(3.0, 0.0), chev + vec2(-3.5, 6.5)], Stroke::new(2.0_f32, alpha(self.pal.text2, 0.7))));
        if !value.is_empty() {
            p.text(pos2(rect.right() - 32.0, rect.center().y), Align2::RIGHT_CENTER, value, font(15.0), self.pal.text2);
        }
        resp.clicked()
    }

    fn toggle(&mut self, title: &str, sub: Option<&str>, v: &mut bool) -> bool {
        let h = if sub.is_some() { 58.0 } else { 48.0 };
        let (rect, resp) = self.row(h, Sense::click(), 16.0);
        self.label(&rect, rect.left() + 16.0, title, sub);
        let sr = Rect::from_center_size(pos2(rect.right() - 16.0 - 25.5, rect.center().y), vec2(51.0, 31.0));
        let sresp = self.ui.interact(sr, resp.id.with("sw"), Sense::click());
        let mut changed = false;
        if resp.clicked() || sresp.clicked() {
            *v = !*v;
            changed = true;
        }
        let pressed = if sresp.is_pointer_button_down_on() { &sresp } else { &resp };
        paint_switch(self.ui, &self.pal, sr, pressed, *v);
        changed
    }

    fn slider(&mut self, title: &str, v: &mut f32, min: f32, max: f32, fmt: &dyn Fn(f32) -> String) -> bool {
        let (rect, _) = self.row(78.0, Sense::hover(), 16.0);
        let p = self.ui.painter();
        p.text(pos2(rect.left() + 16.0, rect.top() + 20.0), Align2::LEFT_CENTER, title, font(15.0), self.pal.text);
        p.text(pos2(rect.right() - 16.0, rect.top() + 20.0), Align2::RIGHT_CENTER, fmt(*v), font(15.0), self.pal.text2);
        let sr = Rect::from_min_size(pos2(rect.left() + 4.0, rect.top() + 36.0), vec2(rect.width() - 8.0, 30.0));
        let mut changed = false;
        self.ui.allocate_new_ui(UiBuilder::new().max_rect(sr).id_salt(title), |ui| {
            changed = slider(ui, &self.pal, v, min, max, sr.width());
        });
        changed
    }

    fn segmented(&mut self, title: Option<&str>, idx: &mut usize, labels: &[&str]) -> bool {
        let h = if title.is_some() { 84.0 } else { 52.0 };
        let (rect, _) = self.row(h, Sense::hover(), 16.0);
        if let Some(t) = title {
            self.ui.painter().text(pos2(rect.left() + 16.0, rect.top() + 20.0), Align2::LEFT_CENTER, t, font(15.0), self.pal.text);
        }
        let sr = Rect::from_min_size(pos2(rect.left() + 12.0, rect.bottom() - 42.0), vec2(rect.width() - 24.0, 32.0));
        let mut changed = false;
        self.ui.allocate_new_ui(UiBuilder::new().max_rect(sr).id_salt(("seg", labels.len(), title)), |ui| {
            changed = segmented(ui, &self.pal, idx, labels, sr.width());
        });
        changed
    }

    fn button(&mut self, title: &str, color: Color32, center: bool) -> bool {
        let (rect, resp) = self.row(48.0, Sense::click(), 16.0);
        let p = self.ui.painter();
        if center {
            p.text(rect.center(), Align2::CENTER_CENTER, title, font(15.0), color);
        } else {
            p.text(pos2(rect.left() + 16.0, rect.center().y), Align2::LEFT_CENTER, title, font(15.0), color);
        }
        resp.clicked()
    }

    fn info(&mut self, title: &str, value: &str) {
        let (rect, _) = self.row(46.0, Sense::hover(), 16.0);
        let p = self.ui.painter();
        p.text(pos2(rect.left() + 16.0, rect.center().y), Align2::LEFT_CENTER, title, font(15.0), self.pal.text);
        p.text(pos2(rect.right() - 16.0, rect.center().y), Align2::RIGHT_CENTER, value, font(15.0), self.pal.text2);
    }
}

fn group<R>(ui: &mut Ui, pal: &Pal, header: Option<&str>, footer: Option<&str>, body: impl FnOnce(&mut Rows) -> R) -> R {
    let full = ui.available_width();
    let margin = 16.0;
    let w = (full - margin * 2.0).max(100.0);
    if let Some(h) = header {
        let (r, _) = ui.allocate_exact_size(vec2(full, 34.0), Sense::hover());
        ui.painter().text(pos2(r.left() + margin + 16.0, r.bottom() - 8.0), Align2::LEFT_BOTTOM, h.to_uppercase(), font(12.0), pal.text2);
    } else {
        ui.add_space(14.0);
    }
    let top = ui.cursor().min;
    let bg = ui.painter().add(egui::Shape::Noop);
    let rect = Rect::from_min_size(pos2(top.x + margin, top.y), vec2(w, f32::INFINITY));
    let mut out = None;
    let r = ui.allocate_new_ui(UiBuilder::new().max_rect(rect), |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let mut rows = Rows { ui, pal: *pal, n: 0, w };
        out = Some(body(&mut rows));
    });
    let card = Rect::from_min_size(rect.min, vec2(w, r.response.rect.height()));
    ui.painter().set(bg, egui::Shape::rect_filled(card, Rounding::same(12.0), pal.card));
    if let Some(f) = footer {
        ui.add_space(6.0);
        let galley = ui.painter().layout(f.to_string(), font(12.0), pal.text2, w - 32.0);
        let (fr, _) = ui.allocate_exact_size(vec2(full, galley.size().y + 4.0), Sense::hover());
        ui.painter().galley(pos2(fr.left() + margin + 16.0, fr.top()), galley, pal.text2);
    }
    out.unwrap()
}

// ---------------------------------------------------------------- приложение

impl eframe::App for SettingsApp {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        self.pal.bg.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        if self.profile {
            let cpu = frame.info().cpu_usage.unwrap_or(0.0);
            self.prof.0 += cpu;
            self.prof.1 = self.prof.1.max(cpu);
            self.prof.2 += 1;
            let gap = now - self.prof.4;
            if gap > self.prof.5 { self.prof.5 = gap; }
            self.prof.4 = now;
            if now - self.prof.3 >= 2.0 {
                settings::log(&format!("ui frames={} cpu_avg={:.2}ms cpu_max={:.2}ms max_gap={:.1}ms", self.prof.2, self.prof.0 * 1000.0 / self.prof.2.max(1) as f32, self.prof.1 * 1000.0, self.prof.5 * 1000.0));
                self.prof = (0.0, 0.0, 0, now, now, 0.0);
            }
        }
        self.poll(now);
        let dark = match self.s.theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => self.sys_dark,
        };
        if dark != self.pal.dark {
            self.pal = Pal::new(dark);
        }
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        self.apply_window_theme();
        self.handle_keys(ctx, now);
        // демо-навигация для автоматических скриншотов (HZ_UI_DEMO=1)
        if self.demo {
            let step = (now / 1.5) as u32;
            if step != self.demo_step {
                self.demo_step = step;
                match step % 4 {
                    1 => self.push(Page::Wind, now),
                    3 => self.pop(now, 0.0),
                    _ => {}
                }
            }
        }

        let pal = self.pal;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(pal.bg)).show(ctx, |ui| {
            let full = ui.max_rect();
            // анимация перехода
            let mut p_top = 0.0f32;
            let mut two = false;
            if let Some(a) = &self.anim {
                let t = ((now - a.start) as f32 / a.dur).clamp(0.0, 1.0);
                let e = if a.p_to > a.p_from { ease_out(t) } else { 1.0 - (1.0 - t).powi(4) };
                p_top = a.p_from + (a.p_to - a.p_from) * e;
                two = true;
                if t >= 1.0 {
                    if a.p_to >= 1.0 {
                        self.stack.pop();
                    }
                    self.anim = None;
                    two = false;
                    p_top = 0.0;
                }
                ctx.request_repaint();
            }
            if let Some(sp) = self.swipe {
                p_top = sp;
                two = true;
            }
            let n = self.stack.len();
            if two && n >= 2 {
                let under = self.stack[n - 2];
                let top = self.stack[n - 1];
                let w = full.width();
                let ur = full.translate(vec2(-0.3 * w * (1.0 - p_top), 0.0));
                self.page_in(ui, under, ur, full, now, false);
                ui.painter().rect_filled(full, Rounding::ZERO, Color32::from_black_alpha((70.0 * (1.0 - p_top)) as u8));
                let tr = full.translate(vec2(w * p_top, 0.0));
                // тень слева от верхней страницы
                for i in 0..10 {
                    let x = tr.left() - i as f32 * 1.5;
                    ui.painter().rect_filled(Rect::from_min_max(pos2(x - 1.5, full.top()), pos2(x, full.bottom())), Rounding::ZERO, Color32::from_black_alpha(((10 - i) as f32 * 2.0 * (1.0 - p_top)) as u8));
                }
                ui.painter().rect_filled(tr, Rounding::ZERO, pal.bg);
                self.page_in(ui, top, tr, full, now, true);
            } else {
                let top = *self.stack.last().unwrap();
                self.page_in(ui, top, full, full, now, true);
            }
            // свайп от левого края — назад, как в iOS
            if n >= 2 && self.anim.is_none() {
                let edge = Rect::from_min_size(full.min, vec2(22.0, full.height()));
                let resp = ui.interact(edge, ui.id().with("swipe"), Sense::drag());
                if resp.dragged() {
                    let dx = resp.interact_pointer_pos().map(|p| p.x - full.left()).unwrap_or(0.0);
                    self.swipe = Some((dx / full.width()).clamp(0.0, 1.0));
                    ctx.request_repaint();
                }
                if resp.drag_stopped() {
                    let p = self.swipe.unwrap_or(0.0);
                    let vel = ctx.input(|i| i.pointer.velocity().x);
                    if p > 0.35 || vel > 600.0 {
                        self.pop(now, p);
                    } else {
                        self.swipe = None;
                        self.anim = Some(Anim { p_from: p, p_to: 0.0, start: now, dur: 0.25 });
                    }
                }
            }
        });

        self.flush(now, false);
        if self.dirty_at.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(90));
        }
        // живые превью: всегда плавно, пока окно видно (свёрнуто — не рисуем)
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if minimized {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        } else {
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.end_capture();
        self.flush(0.0, true);
    }
}

impl SettingsApp {
    fn handle_keys(&mut self, ctx: &egui::Context, now: f64) {
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            if let egui::Event::Key { key, physical_key, pressed: true, modifiers, .. } = e {
                if let Some(slot) = self.capturing {
                    let k = physical_key.unwrap_or(key);
                    if k == egui::Key::Escape {
                        self.end_capture();
                        continue;
                    }
                    if matches!(k, egui::Key::Backspace | egui::Key::Delete) && !modifiers.any() {
                        self.set_hotkey(slot, Hotkey::new(0, 0));
                        self.changed(now);
                        self.flush(now, true);
                        self.end_capture();
                        continue;
                    }
                    if let Some(vk) = egui_to_vk(k) {
                        let mut mods = 0;
                        if modifiers.alt { mods |= 1; }
                        if modifiers.ctrl { mods |= 2; }
                        if modifiers.shift { mods |= 4; }
                        let is_f = (0x70..=0x87).contains(&vk);
                        if mods != 0 || is_f {
                            self.set_hotkey(slot, Hotkey::new(mods, vk));
                            self.changed(now);
                            self.flush(now, true);
                            self.end_capture();
                        }
                    }
                } else if key == egui::Key::Escape || key == egui::Key::Backspace {
                    self.pop(now, 0.0);
                }
            }
            if let egui::Event::PointerButton { button: egui::PointerButton::Extra1, pressed: true, .. } = e {
                self.pop(now, 0.0);
            }
        }
    }

    fn set_hotkey(&mut self, slot: usize, hk: Hotkey) {
        match slot {
            0 => self.s.hk_toggle = hk,
            1 => self.s.hk_settings = hk,
            _ => self.s.hk_mode = hk,
        }
    }

    /// Рисует страницу в прямоугольнике `rect` (может быть сдвинут при анимации).
    fn page_in(&mut self, ui: &mut Ui, page: Page, rect: Rect, clip: Rect, now: f64, interactive: bool) {
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect).id_salt(("page", page as u8)));
        child.set_clip_rect(clip.intersect(rect.expand(30.0)));
        if !interactive || self.anim.is_some() {
            child.disable();
        }
        let pal = self.pal;
        if page == Page::Main {
            egui::ScrollArea::vertical().id_salt("main").auto_shrink([false; 2]).show(&mut child, |ui| {
                ui.set_width(rect.width());
                self.main_page(ui, now);
                ui.add_space(30.0);
            });
            return;
        }
        // навигационная панель
        let bar = Rect::from_min_size(rect.min, vec2(rect.width(), 50.0));
        child.allocate_new_ui(UiBuilder::new().max_rect(bar), |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(130.0, 50.0), Sense::click());
            let a = ui.ctx().animate_bool_with_time(resp.id, resp.is_pointer_button_down_on(), 0.1);
            let col = alpha(pal.accent, 1.0 - 0.5 * a);
            let c = pos2(r.left() + 18.0, r.center().y);
            ui.painter().add(egui::Shape::line(vec![c + vec2(5.0, -9.0), c + vec2(-4.0, 0.0), c + vec2(5.0, 9.0)], Stroke::new(2.6_f32, col)));
            let back_title = self.stack.iter().rev().nth(1).map(|p| p.title()).unwrap_or("Назад");
            let back_title = if back_title == page.title() { "Назад" } else { back_title };
            ui.painter().text(pos2(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, back_title, font(16.0), col);
            if resp.clicked() {
                self.pop(now, 0.0);
            }
        });
        child.painter().text(bar.center(), Align2::CENTER_CENTER, page.title(), semibold(16.0), pal.text);
        let body = Rect::from_min_max(pos2(rect.left(), bar.bottom()), rect.max);
        child.painter().line_segment([pos2(rect.left(), bar.bottom()), pos2(rect.right(), bar.bottom())], Stroke::new(0.5_f32, alpha(pal.sep, 0.6)));
        child.allocate_new_ui(UiBuilder::new().max_rect(body), |ui| {
            egui::ScrollArea::vertical().id_salt(("pg", page as u8)).auto_shrink([false; 2]).show(ui, |ui| {
                ui.set_width(body.width());
                match page {
                    Page::Particles => self.particles_page(ui, now),
                    Page::Wind => self.wind_page(ui, now),
                    Page::Mouse => self.mouse_page(ui, now),
                    Page::Effects => self.effects_page(ui, now),
                    Page::Performance => self.perf_page(ui, now),
                    Page::Hotkeys => self.hotkeys_page(ui, now),
                    Page::General => self.general_page(ui, now),
                    Page::About => self.about_page(ui, now),
                    Page::Weather => self.weather_page(ui, now),
                    Page::Main => {}
                }
                ui.add_space(40.0);
            });
        });
    }

    fn status_line(&self) -> (String, Color32) {
        let pal = &self.pal;
        match self.status {
            None => ("Не запущено".into(), pal.red),
            Some(v) => {
                if v & st::ENABLED == 0 {
                    ("Выключено".into(), pal.text2)
                } else if v & st::GPU_FAIL != 0 {
                    ("Ошибка видеокарты".into(), pal.red)
                } else if v & st::LOCKED != 0 {
                    ("Пауза · экран заблокирован".into(), pal.orange)
                } else if v & st::FULLSCREEN != 0 && v & st::RUNNING == 0 {
                    ("Пауза · полноэкранное приложение".into(), pal.orange)
                } else if v & st::FULLSCREEN != 0 {
                    (format!("Работает · {} FPS · часть экранов на паузе", st::fps(v)), pal.green)
                } else {
                    let eco = if v & st::ECO != 0 { " · экономия" } else { "" };
                    (format!("Работает · {} FPS{}", st::fps(v), eco), pal.green)
                }
            }
        }
    }

    fn eff(&self) -> (Settings, crate::auto::Why) {
        crate::auto::effective(&self.s, self.wx.as_ref())
    }

    fn set_mode(&mut self, m: Mode, now: f64) {
        self.s.auto_season = false;
        self.s.weather_sync = false;
        self.s.mode = m;
        self.s.enabled = true;
        self.changed(now);
    }

    // ------------------------------------------------------------ главная

    fn main_page(&mut self, ui: &mut Ui, now: f64) {
        let pal = self.pal;
        let w = ui.available_width();
        let t = now as f32;
        // крупный заголовок
        let (r, _) = ui.allocate_exact_size(vec2(w, 64.0), Sense::hover());
        ui.painter().text(pos2(r.left() + 20.0, r.bottom() - 6.0), Align2::LEFT_BOTTOM, "Flurry", bold(30.0), pal.text);

        // ---- герой: живое превью текущего режима + главный переключатель
        let (eff, why) = self.eff();
        if eff.mode != self.hero_mode.1 {
            let cur = self.hero_mode.1;
            self.hero_mode = (cur, eff.mode, if self.hero_mode.2 < -5.0 { now - 10.0 } else { now });
        }
        let (hr, hresp) = ui.allocate_exact_size(vec2(w, 196.0), Sense::hover());
        let card = Rect::from_min_max(pos2(hr.left() + 16.0, hr.top() + 8.0), pos2(hr.right() - 16.0, hr.bottom() - 8.0));
        let (m_from, m_to, m_t) = self.hero_mode;
        let k = ease_in_out(((now - m_t) / 0.6) as f32);
        let (a1, b1) = mode_gradient(m_from);
        let (a2, b2) = mode_gradient(m_to);
        let off = ui.ctx().animate_bool_with_time(hresp.id.with("off"), !self.s.enabled || self.status.is_none(), 0.4);
        let gray = |c: Color32| {
            let l = ((c.r() as u32 + c.g() as u32 + c.b() as u32) / 3) as u8;
            lerp_c(c, Color32::from_rgb(l / 2, l / 2, l / 2), off * 0.8)
        };
        let p = ui.painter();
        shadow(p, card, 18.0, if pal.dark { 0.5 } else { 0.18 }, 22.0, 6.0);
        gradient(p, card, 18.0, gray(lerp_c(a1, a2, k)), gray(lerp_c(b1, b2, k)));
        let inner = card.shrink(2.0);
        if k < 1.0 {
            preview(p, inner, m_from, &eff, t, (1.0 - k) * (1.0 - off * 0.6), 1.0);
        }
        preview(p, inner, m_to, &eff, t, k * (1.0 - off * 0.6), 1.0);
        // текст
        let (status, scol) = self.status_line();
        p.text(pos2(card.left() + 20.0, card.bottom() - 44.0), Align2::LEFT_BOTTOM, if why.source != "вручную" { format!("{} · {}", eff.mode.name(), why.source) } else { eff.mode.name().to_string() }, bold(26.0), Color32::WHITE);
        let pulse = 0.65 + 0.35 * ((t * 2.4).sin() * 0.5 + 0.5);
        let dot = pos2(card.left() + 25.0, card.bottom() - 26.0);
        p.circle_filled(dot, 7.0, alpha(scol, 0.25 * pulse));
        p.circle_filled(dot, 4.0, scol);
        p.text(pos2(card.left() + 37.0, card.bottom() - 26.0), Align2::LEFT_CENTER, status, font(13.5), alpha(Color32::WHITE, 0.85));
        // главный переключатель
        let sr = Rect::from_min_size(pos2(card.right() - 20.0 - 51.0, card.top() + 18.0), vec2(51.0, 31.0));
        let sresp = ui.interact(sr, ui.id().with("main_sw"), Sense::click());
        let mut en = self.s.enabled && self.status.is_some();
        if sresp.clicked() {
            if self.status.is_none() {
                launch_main();
                self.s.enabled = true;
                self.next_poll = now + 0.7;
            } else {
                self.s.enabled = !self.s.enabled;
            }
            en = self.s.enabled;
            self.changed(now);
        }
        let mut pal_hero = pal;
        pal_hero.track = Color32::from_white_alpha(60);
        paint_switch(ui, &pal_hero, sr, &sresp, en);

        // ---- быстрые действия
        ui.add_space(4.0);
        let (ar, _) = ui.allocate_exact_size(vec2(w, 92.0), Sense::hover());
        let acts = [(Icon::Bolt, "Буря", pal.orange), (Icon::Sparkle, "Очистить", pal.accent), (Icon::Next, "Дальше", Color32::from_rgb(175, 82, 222))];
        let cw = (ar.width() - 32.0) / 3.0;
        for (i, (ic, label, col)) in acts.iter().enumerate() {
            let c = pos2(ar.left() + 16.0 + cw * (i as f32 + 0.5), ar.top() + 34.0);
            let br = Rect::from_center_size(c, vec2(cw - 12.0, 60.0));
            let resp = ui.interact(br, ui.id().with(("act", i)), Sense::click());
            let down = ui.ctx().animate_bool_with_time(resp.id, resp.is_pointer_button_down_on(), 0.12);
            let sc = 1.0 - 0.08 * down;
            let rr = Rect::from_center_size(c, vec2(56.0, 56.0) * sc);
            let p = ui.painter();
            p.rect_filled(rr, Rounding::same(28.0 * sc), lerp_c(pal.card, pal.press, down));
            icon(p, c, 26.0 * sc, *ic, *col);
            p.text(pos2(c.x, ar.top() + 78.0), Align2::CENTER_CENTER, *label, font(12.5), pal.text2);
            if resp.clicked() {
                match i {
                    0 => {
                        ipc::post(cmd::STORM);
                    }
                    1 => {
                        ipc::post(cmd::CLEAR);
                    }
                    _ => {
                        let m = eff.mode.next();
                        self.set_mode(m, now);
                    }
                }
            }
        }

        // ---- режимы
        let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
        ui.painter().text(pos2(r.left() + 32.0, r.bottom() - 6.0), Align2::LEFT_BOTTOM, "РЕЖИМ", font(12.0), pal.text2);
        let cols = 4;
        let gap = 10.0;
        let tw = (w - 32.0 - gap * (cols as f32 - 1.0)) / cols as f32;
        let th = tw * 1.12;
        let tiles: Vec<Option<Mode>> = Mode::ALL.iter().map(|m| Some(*m)).chain(std::iter::once(None)).collect();
        let rows = tiles.len().div_ceil(cols);
        let (gr, _) = ui.allocate_exact_size(vec2(w, rows as f32 * (th + gap)), Sense::hover());
        for (i, tile) in tiles.iter().enumerate() {
            let (cx, cy) = (i % cols, i / cols);
            let rect = Rect::from_min_size(pos2(gr.left() + 16.0 + cx as f32 * (tw + gap), gr.top() + cy as f32 * (th + gap)), vec2(tw, th));
            let resp = ui.interact(rect, ui.id().with(("tile", i)), Sense::click());
            let selected = match tile {
                Some(m) => *m == self.s.mode && !self.s.auto_season && !self.s.weather_sync,
                None => self.s.auto_season || self.s.weather_sync,
            };
            let sel = ease_out(ui.ctx().animate_bool_with_time(resp.id.with("sel"), selected, 0.3));
            let down = ui.ctx().animate_bool_with_time(resp.id.with("down"), resp.is_pointer_button_down_on(), 0.1);
            let hover = ui.ctx().animate_bool_with_time(resp.id.with("hov"), resp.hovered(), 0.15);
            let scale = 1.0 - 0.06 * down + 0.02 * hover;
            let rr = Rect::from_center_size(rect.center(), rect.size() * scale);
            let mode = tile.unwrap_or(eff_auto_mode(&self.s, self.wx.as_ref()));
            let (ga, gb) = mode_gradient(mode);
            let p = ui.painter();
            shadow(p, rr, 14.0, if pal.dark { 0.0 } else { 0.08 }, 10.0, 2.0);
            p.rect_filled(rr, Rounding::same(14.0), pal.card);
            let prev = Rect::from_min_max(rr.min + vec2(5.0, 5.0), pos2(rr.right() - 5.0, rr.bottom() - 24.0));
            gradient(p, prev, 10.0, ga, gb);
            preview(p, prev.shrink(1.0), mode, &self.s, t + i as f32 * 3.0, 1.0, 0.75);
            if tile.is_none() {
                p.text(prev.center(), Align2::CENTER_CENTER, "АВТО", semibold(12.0), Color32::WHITE);
            }
            let label = match tile {
                Some(m) => m.name(),
                None => "Авто",
            };
            p.text(pos2(rr.center().x, rr.bottom() - 12.0), Align2::CENTER_CENTER, label, if selected { semibold(12.0) } else { font(12.0) }, if selected { pal.text } else { pal.text2 });
            if sel > 0.01 {
                p.rect_stroke(rr.expand(2.5 * sel), Rounding::same(16.0), Stroke::new(2.5 * sel, alpha(pal.accent, sel)));
                let bc = pos2(prev.right() - 9.0, prev.top() + 9.0);
                p.circle_filled(bc, 7.5 * sel, pal.accent);
                p.add(egui::Shape::line(vec![bc + vec2(-3.2, 0.2) * sel, bc + vec2(-0.8, 2.6) * sel, bc + vec2(3.4, -2.4) * sel], Stroke::new(1.6_f32, Color32::WHITE)));
            }
            if resp.clicked() {
                match tile {
                    Some(m) => {
                        self.set_mode(*m, now);
                        self.s.enabled = true;
                    }
                    None => {
                        self.s.auto_season = true;
                        if !self.s.city.trim().is_empty() {
                            self.s.weather_sync = true;
                        }
                        self.s.enabled = true;
                        self.changed(now);
                    }
                }
            }
        }

        // ---- разделы
        let s = &self.s;
        let wind_v = if s.wind < 0.01 { "Штиль".to_string() } else { format!("{} · {}%", compass_name(s.wind_angle), (s.wind * 100.0).round()) };
        let mouse_v = match s.mouse {
            MouseMode::Off => "Выкл.",
            MouseMode::Repel => "Отталкивает",
            MouseMode::Attract => "Притягивает",
        };
        let fps_v = if s.fps == 0 { "Авто".to_string() } else { format!("{} FPS", s.fps) };
        let mut go = None;
        let wx_v = match (&self.wx, s.weather_sync) {
            (Some(w), true) if w.ok() => format!("{} {:+.0}°", w.city, w.temp),
            (_, true) => "Обновляю…".to_string(),
            _ if s.auto_season => "По сезону".to_string(),
            _ => "Выкл.".to_string(),
        };
        group(ui, &pal, Some("Погода"), None, |r| {
            if r.nav(Icon::Cloud, rgb(0, 150, 255), "Погода и время", &wx_v) { go = Some(Page::Weather); }
            if r.nav(Icon::Flake, rgb(10, 132, 255), "Частицы", &format!("×{:.1}", s.density)) { go = Some(Page::Particles); }
            if r.nav(Icon::Wind, rgb(50, 173, 230), "Ветер", &wind_v) { go = Some(Page::Wind); }
            if r.nav(Icon::Cursor, rgb(175, 82, 222), "Мышь", mouse_v) { go = Some(Page::Mouse); }
            if r.nav(Icon::Drop, rgb(255, 149, 0), "Эффекты", "") { go = Some(Page::Effects); }
        });
        group(ui, &pal, Some("Система"), None, |r| {
            if r.nav(Icon::Gauge, rgb(52, 199, 89), "Производительность", &fps_v) { go = Some(Page::Performance); }
            if r.nav(Icon::Key, rgb(142, 142, 147), "Горячие клавиши", "") { go = Some(Page::Hotkeys); }
            if r.nav(Icon::Gear, rgb(99, 99, 102), "Основные", "") { go = Some(Page::General); }
        });
        group(ui, &pal, None, None, |r| {
            if r.nav(Icon::Info, rgb(88, 86, 214), "О программе", &format!("v{VERSION}")) { go = Some(Page::About); }
        });
        if let Some(p) = go {
            self.push(p, now);
        }
        ui.add_space(14.0);
        let (fr, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
        ui.painter().text(fr.center(), Align2::CENTER_CENTER, format!("{} — вкл/выкл · {} — сменить режим", self.s.hk_toggle.describe(), self.s.hk_mode.describe()), font(12.0), pal.text2);
    }

    // ------------------------------------------------------------ страницы

    fn particles_page(&mut self, ui: &mut Ui, now: f64) {
        let pal = self.pal;
        let mut ch = false;
        let s = &mut self.s;
        group(ui, &pal, Some("Количество и движение"), None, |r| {
            ch |= r.slider("Скорость", &mut s.speed, 0.1, 3.0, &|v| format!("×{v:.1}"));
            ch |= r.slider("Плотность", &mut s.density, 0.05, 3.0, &|v| format!("×{v:.2}"));
            ch |= r.slider("Размер", &mut s.size, 0.4, 2.5, &|v| format!("×{v:.1}"));
            ch |= r.slider("Непрозрачность", &mut s.opacity, 0.05, 1.0, &|v| format!("{}%", (v * 100.0).round()));
        });
        group(ui, &pal, Some("Глубина"), Some("Объём: дальние частицы мельче, тусклее и медленнее. Боке: редкие крупные размытые снежинки «у самого стекла»."), |r| {
            ch |= r.toggle("Объём (3D)", None, &mut s.depth3d);
            ch |= r.toggle("Боке", Some("Только для снега"), &mut s.bokeh);
        });
        // оттенок — круглые образцы
        let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        ui.painter().text(pos2(hr.left() + 32.0, hr.bottom() - 8.0), Align2::LEFT_BOTTOM, "ОТТЕНОК", font(12.0), pal.text2);
        let w = ui.available_width();
        let (cr, _) = ui.allocate_exact_size(vec2(w, 92.0), Sense::hover());
        let card = Rect::from_min_max(pos2(cr.left() + 16.0, cr.top()), pos2(cr.right() - 16.0, cr.bottom()));
        ui.painter().rect_filled(card, Rounding::same(12.0), pal.card);
        let n = Tint::ALL.len();
        let cw = card.width() / n as f32;
        for (i, t) in Tint::ALL.iter().enumerate() {
            let c = pos2(card.left() + cw * (i as f32 + 0.5), card.top() + 34.0);
            let resp = ui.interact(Rect::from_center_size(pos2(c.x, card.center().y), vec2(cw, card.height())), ui.id().with(("tint", i)), Sense::click());
            let sel = ease_out(ui.ctx().animate_bool_with_time(resp.id, s.tint == *t, 0.25));
            let p = ui.painter();
            let r = 15.0;
            if *t == Tint::Rainbow {
                rainbow_circle(p, c, r);
            } else {
                p.circle_filled(c, r, tint_swatch(*t));
            }
            p.circle_stroke(c, r, Stroke::new(0.8_f32, alpha(pal.text2, 0.4)));
            if sel > 0.01 {
                p.circle_stroke(c, r + 4.0, Stroke::new(2.2_f32, alpha(pal.accent, sel)));
            }
            p.text(pos2(c.x, card.top() + 72.0), Align2::CENTER_CENTER, t.name(), font(11.5), if s.tint == *t { pal.text } else { pal.text2 });
            if resp.clicked() {
                s.tint = *t;
                ch = true;
            }
        }
        // превью
        let (pr, _) = ui.allocate_exact_size(vec2(w, 130.0), Sense::hover());
        let card = Rect::from_min_max(pos2(pr.left() + 16.0, pr.top() + 16.0), pos2(pr.right() - 16.0, pr.bottom()));
        let (ga, gb) = mode_gradient(s.mode);
        gradient(ui.painter(), card, 12.0, ga, gb);
        preview(ui.painter(), card, s.mode, s, now as f32, 1.0, 1.0);
        if ch {
            self.changed(now);
        }
    }

    fn wind_page(&mut self, ui: &mut Ui, now: f64) {
        let pal = self.pal;
        let w = ui.available_width();
        let t = now as f32;
        // компас
        let (hr, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
        ui.painter().text(pos2(hr.left() + 32.0, hr.bottom() - 8.0), Align2::LEFT_BOTTOM, "НАПРАВЛЕНИЕ", font(12.0), pal.text2);
        let (dr, _) = ui.allocate_exact_size(vec2(w, 280.0), Sense::hover());
        let card = Rect::from_min_max(pos2(dr.left() + 16.0, dr.top()), pos2(dr.right() - 16.0, dr.bottom()));
        ui.painter().rect_filled(card, Rounding::same(12.0), pal.card);
        let c = card.center();
        let rad = 112.0f32.min(card.width() / 2.0 - 30.0);
        let resp = ui.interact(Rect::from_center_size(c, vec2(rad * 2.0 + 40.0, rad * 2.0 + 40.0)), ui.id().with("dial"), Sense::click_and_drag());
        let mut ch = false;
        if resp.dragged() || resp.clicked() {
            if let Some(pt) = resp.interact_pointer_pos() {
                let d = pt - c;
                if d.length() > 8.0 {
                    let mut a = d.x.atan2(-d.y).to_degrees().rem_euclid(360.0);
                    // мягкая «примагниченность» к основным направлениям
                    let snap = (a / 45.0).round() * 45.0;
                    if (a - snap).abs() < 6.0 {
                        a = snap.rem_euclid(360.0);
                    }
                    a = a.round();
                    if a != self.s.wind_angle {
                        self.s.wind_angle = a;
                        ch = true;
                    }
                }
            }
        }
        // плавная стрелка по кратчайшему пути
        if self.dial_shown.is_nan() {
            self.dial_shown = self.s.wind_angle;
        }
        let mut diff = (self.s.wind_angle - self.dial_shown + 540.0).rem_euclid(360.0) - 180.0;
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.05);
        diff *= 1.0 - (-dt * 14.0).exp();
        self.dial_shown = (self.dial_shown + diff).rem_euclid(360.0);
        let shown = self.dial_shown;
        let p = ui.painter();
        p.circle_filled(c, rad, if pal.dark { rgb(22, 22, 24) } else { rgb(246, 246, 250) });
        p.circle_stroke(c, rad, Stroke::new(1.0_f32, alpha(pal.text2, 0.3)));
        for i in 0..72 {
            let a = (i as f32 * 5.0).to_radians();
            let d = vec2(a.sin(), -a.cos());
            let major = i % 18 == 0;
            let mid = i % 9 == 0;
            let l = if major { 14.0 } else if mid { 10.0 } else { 5.0 };
            p.line_segment([c + d * (rad - 4.0), c + d * (rad - 4.0 - l)], Stroke::new(if major { 2.0 } else { 1.0_f32 }, alpha(pal.text2, if major { 0.9 } else { 0.45 })));
        }
        for (i, l) in ["С", "В", "Ю", "З"].iter().enumerate() {
            let d = vec_from_deg(i as f32 * 90.0);
            p.text(c + d * (rad - 32.0), Align2::CENTER_CENTER, *l, semibold(15.0), if i == 0 { pal.red } else { pal.text });
        }
        // бегущие струйки ветра внутри
        let dir = vec_from_deg(shown);
        let perp = vec2(-dir.y, dir.x);
        let strength = self.s.wind;
        let inner_p = p.with_clip_rect(Rect::from_center_size(c, vec2(rad * 1.2, rad * 1.2)));
        if strength > 0.01 {
            for i in 0..9 {
                let off = (i as f32 - 4.0) * 12.0;
                let ph = ((t * (0.4 + strength * 1.8) + i as f32 * 0.37) % 1.0) * 2.0 - 1.0;
                let len = 16.0 + 26.0 * strength;
                let a0 = c + perp * off + dir * (ph * rad * 0.75);
                let fadev = 1.0 - ph.abs();
                inner_p.line_segment([a0 - dir * len * 0.5, a0 + dir * len * 0.5], Stroke::new(2.0_f32, alpha(pal.accent, 0.12 + 0.35 * fadev)));
            }
        }
        // стрелка
        let tip = c + dir * (rad - 46.0);
        let tail = c - dir * (rad - 60.0);
        p.line_segment([tail, tip - dir * 10.0], Stroke::new(4.0_f32, pal.accent));
        p.add(egui::Shape::convex_polygon(vec![tip + dir * 6.0, tip - dir * 14.0 + perp * 10.0, tip - dir * 14.0 - perp * 10.0], pal.accent, Stroke::NONE));
        p.circle_filled(c, 26.0, pal.card);
        p.circle_stroke(c, 26.0, Stroke::new(1.0_f32, alpha(pal.text2, 0.3)));
        p.text(c + vec2(0.0, -4.0), Align2::CENTER_CENTER, format!("{}°", self.s.wind_angle.round() as i32), semibold(14.0), pal.text);
        p.text(c + vec2(0.0, 11.0), Align2::CENTER_CENTER, compass_name(self.s.wind_angle), font(10.0), pal.text2);

        let pal = self.pal;
        let s = &mut self.s;
        group(ui, &pal, None, Some("Перетащите стрелку, чтобы задать направление. Порывы — ветер то стихает, то усиливается."), |r| {
            ch |= r.slider("Сила", &mut s.wind, 0.0, 1.0, &|v| if v < 0.01 { "Штиль".into() } else { format!("{}%", (v * 100.0).round()) });
            ch |= r.toggle("Порывы", None, &mut s.gusts);
            ch |= r.slider("Покачивание", &mut s.turbulence, 0.0, 2.0, &|v| format!("×{v:.1}"));
        });
        group(ui, &pal, Some("Быстрый выбор"), None, |r| {
            let mut idx = usize::MAX;
            let opts = [(270.0, "←"), (225.0, "↙"), (180.0, "↓"), (135.0, "↘"), (90.0, "→")];
            for (i, (a, _)) in opts.iter().enumerate() {
                if (s.wind_angle - a).abs() < 0.5 {
                    idx = i;
                }
            }
            let mut sel = if idx == usize::MAX { 99 } else { idx };
            let labels: Vec<&str> = opts.iter().map(|o| o.1).collect();
            if r.segmented(None, &mut sel, &labels) && sel < opts.len() {
                s.wind_angle = opts[sel].0;
                ch = true;
            }
        });
        if ch {
            self.changed(now);
        }
    }

    fn mouse_page(&mut self, ui: &mut Ui, now: f64) {
        let pal = self.pal;
        let mut ch = false;
        let s = &mut self.s;
        group(ui, &pal, Some("Курсор"), Some("Частицы разлетаются от курсора или кружат вокруг него. Быстрое движение мышью сдувает их."), |r| {
            let mut idx = match s.mouse {
                MouseMode::Off => 0,
                MouseMode::Repel => 1,
                MouseMode::Attract => 2,
            };
            if r.segmented(None, &mut idx, &["Выкл.", "Отталкивать", "Притягивать"]) {
                s.mouse = [MouseMode::Off, MouseMode::Repel, MouseMode::Attract][idx];
                ch = true;
            }
            ch |= r.slider("Сила", &mut s.mouse_strength, 0.1, 2.0, &|v| format!("×{v:.1}"));
            ch |= r.slider("Радиус", &mut s.mouse_radius, 0.3, 2.5, &|v| format!("{} px", (130.0 * v).round()));
        });
        group(ui, &pal, Some("Плюшки"), None, |r| {
            ch |= r.toggle("Шлейф за курсором", Some("Искры при быстром движении"), &mut s.cursor_trail);
            ch |= r.toggle("Лопать пузыри", Some("Режим «Пузыри»"), &mut s.pop_bubbles);
        });
        if ch {
            self.changed(now);
        }
    }

    fn effects_page(&mut self, ui: &mut Ui, now: f64) {
        let pal = self.pal;
        let mut ch = false;
        let s = &mut self.s;
        group(ui, &pal, Some("Дождь"), None, |r| {
            ch |= r.toggle("Туман", Some("Дымка у нижнего края экрана"), &mut s.rain_fog);
            ch |= r.toggle("Гроза", Some("Редкие вспышки молний"), &mut s.lightning);
            ch |= r.toggle("Брызги", Some("Капли разбиваются о панель задач"), &mut s.splashes);
        });
        group(ui, &pal, Some("Переходы"), Some("Новый режим плавно проявляется, а старый — растворяется."), |r| {
            ch |= r.slider("Смена режима", &mut s.crossfade, 0.0, 5.0, &|v| if v < 0.05 { "Мгновенно".into() } else { format!("{v:.1} с") });
        });
        group(ui, &pal, None, Some("Кнопка «Буря» на главной: 9 секунд сильного ветра и в 2,5 раза больше частиц."), |r| {
            if r.button("Буря!", pal.orange, true) {
                ipc::post(cmd::STORM);
            }
        });
        if ch {
            self.changed(now);
        }
    }

    fn perf_page(&mut self, ui: &mut Ui, now: f64) {
        let pal = self.pal;
        let mut ch = false;
        let status = self.status;
        let stats = (self.stats.cpu, self.stats.ws_mb, self.stats.private_mb);
        let s = &mut self.s;
        group(ui, &pal, Some("Частота кадров"), Some("Авто: 30–60 кадров — ровно столько, чтобы движение оставалось плавным. Частицы рисует видеокарта (Direct3D 11)."), |r| {
            let opts = [0u32, 30, 60, 120, 144];
            let mut idx = opts.iter().position(|f| *f == s.fps).unwrap_or(0);
            if r.segmented(None, &mut idx, &["Авто", "30", "60", "120", "144"]) {
                s.fps = opts[idx];
                ch = true;
            }
        });
        group(ui, &pal, Some("Экономия"), None, |r| {
            ch |= r.toggle("Пауза в полноэкранных", Some("Игры, видео, презентации"), &mut s.pause_fullscreen);
            ch |= r.toggle("Экономия на батарее", Some("30 FPS без зарядки"), &mut s.battery_saver);
            ch |= r.toggle("Только основной монитор", None, &mut s.primary_only);
            ch |= r.toggle("Скрывать от записи экрана", Some("Не попадает в скриншоты и стримы"), &mut s.hide_from_capture);
        });
        group(ui, &pal, Some("Сейчас"), None, |r| match status {
            Some(v) => {
                r.info("Кадров в секунду", &format!("{}", st::fps(v)));
                r.info("Мониторов", &format!("{}", st::monitors(v)));
                r.info("Процессор", &format!("{:.2}%", stats.0));
                r.info("Память", &format!("{:.0} МБ (личная {:.0} МБ)", stats.1, stats.2));
            }
            None => r.info("Статус", "не запущено"),
        });
        if ch {
            self.changed(now);
        }
    }

    fn hotkeys_page(&mut self, ui: &mut Ui, now: f64) {
        let pal = self.pal;
        let mut start = None;
        let capturing = self.capturing;
        let t = now as f32;
        let items = [("Вкл/выкл эффекты", self.s.hk_toggle), ("Открыть настройки", self.s.hk_settings), ("Следующий режим", self.s.hk_mode)];
        group(ui, &pal, Some("Сочетания"), Some("Нажмите на строку, затем новое сочетание (с Ctrl, Alt или Shift; F1–F24 можно без них). Esc — отмена, Backspace — убрать."), |r| {
            for (i, (title, hk)) in items.iter().enumerate() {
                let (rect, resp) = r.row(48.0, Sense::click(), 16.0);
                let p = r.ui.painter();
                p.text(pos2(rect.left() + 16.0, rect.center().y), Align2::LEFT_CENTER, *title, font(15.0), pal.text);
                let active = capturing == Some(i);
                let txt = if active { "Нажмите сочетание…".to_string() } else { hk.describe() };
                let galley = p.layout_no_wrap(txt, if active { font(13.0) } else { semibold(13.0) }, if active { pal.accent } else { pal.text });
                let bw = galley.size().x + 20.0;
                let br = Rect::from_min_size(pos2(rect.right() - 16.0 - bw, rect.center().y - 14.0), vec2(bw, 28.0));
                let pulse = if active { 0.5 + 0.5 * (t * 5.0).sin() } else { 0.0 };
                p.rect_filled(br, Rounding::same(8.0), if active { alpha(pal.accent, 0.12 + 0.1 * pulse) } else { pal.fill });
                p.galley(pos2(br.left() + 10.0, br.center().y - galley.size().y / 2.0), galley, pal.text);
                if resp.clicked() {
                    start = Some(i);
                }
            }
        });
        let mut osd = self.s.show_osd;
        let mut ch = false;
        group(ui, &pal, None, None, |r| {
            ch |= r.toggle("Всплывающая подсказка", Some("Показывать режим при нажатии клавиш"), &mut osd);
        });
        if ch {
            self.s.show_osd = osd;
            self.changed(now);
        }
        if let Some(i) = start {
            if self.capturing.is_none() {
                ipc::post(cmd::HK_SUSPEND);
            }
            self.capturing = Some(i);
        }
    }

    fn general_page(&mut self, ui: &mut Ui, now: f64) {
        let pal = self.pal;
        let mut ch = false;
        let armed = now - self.reset_armed < 3.0;
        let mut reset = false;
        let s = &mut self.s;
        group(ui, &pal, None, Some("Автозапуск добавляет Flurry в автозагрузку Windows (без окна, сразу в трей)."), |r| {
            ch |= r.toggle("Запускать вместе с Windows", None, &mut s.autostart);
        });
        group(ui, &pal, Some("Оформление"), None, |r| {
            let mut idx = match s.theme {
                Theme::System => 0,
                Theme::Dark => 1,
                Theme::Light => 2,
            };
            if r.segmented(None, &mut idx, &["Как в системе", "Тёмная", "Светлая"]) {
                s.theme = [Theme::System, Theme::Dark, Theme::Light][idx];
                ch = true;
            }
        });
        group(ui, &pal, None, None, |r| {
            if r.button(if armed { "Нажмите ещё раз для сброса" } else { "Сбросить все настройки" }, pal.red, true) {
                reset = true;
            }
        });
        if reset {
            if armed {
                let keep_auto = self.s.autostart;
                self.s = Settings { autostart: keep_auto, ..Settings::default() };
                self.reset_armed = -10.0;
                ch = true;
            } else {
                self.reset_armed = now;
            }
        }
        if ch {
            self.changed(now);
        }
    }

    fn weather_page(&mut self, ui: &mut Ui, now: f64) {
        use crate::auto::*;
        let pal = self.pal;
        let w = ui.available_width();
        let mut ch = false;
        let wx = self.wx.clone();
        let (eff, why) = self.eff();
        let (month, _) = local_clock();

        // карточка «сейчас»
        let (cr, _) = ui.allocate_exact_size(vec2(w, 178.0), Sense::hover());
        let card = Rect::from_min_max(pos2(cr.left() + 16.0, cr.top() + 14.0), pos2(cr.right() - 16.0, cr.bottom()));
        let (ga, gb) = mode_gradient(eff.mode);
        let p = ui.painter();
        shadow(p, card, 16.0, if pal.dark { 0.45 } else { 0.15 }, 18.0, 5.0);
        gradient(p, card, 16.0, ga, gb);
        preview(p, card.shrink(2.0), eff.mode, &eff, now as f32, 0.8, 1.0);
        let white = Color32::WHITE;
        match (&wx, self.s.weather_sync) {
            (Some(x), true) if x.ok() && crate::auto::same_city(&x.query, &self.s.city) => {
                p.text(pos2(card.left() + 20.0, card.top() + 16.0), Align2::LEFT_TOP, format!("{}{}", x.city, if x.country.is_empty() { String::new() } else { format!(", {}", x.country) }), semibold(15.0), white);
                p.text(pos2(card.left() + 18.0, card.top() + 36.0), Align2::LEFT_TOP, format!("{:+.0}°", x.temp), bold(46.0), white);
                p.text(pos2(card.left() + 20.0, card.top() + 100.0), Align2::LEFT_TOP, x.describe(), semibold(15.0), white);
                let mins = (unix_now().saturating_sub(x.updated) / 60) as i64;
                let upd = if mins < 1 { "только что".to_string() } else { format!("{mins} мин назад") };
                p.text(pos2(card.left() + 20.0, card.top() + 122.0), Align2::LEFT_TOP, format!("Ветер {:.0} км/ч, {} · {} · обновлено {}", x.wind_kmh, compass16(x.wind_from), x.day_part().name(), upd), font(12.0), alpha(white, 0.85));
            }
            (Some(x), true) if !x.error.is_empty() && crate::auto::same_city(&x.query, &self.s.city) => {
                p.text(pos2(card.left() + 20.0, card.top() + 20.0), Align2::LEFT_TOP, "Не удалось получить погоду", semibold(16.0), white);
                p.text(pos2(card.left() + 20.0, card.top() + 44.0), Align2::LEFT_TOP, &x.error, font(13.0), alpha(white, 0.85));
            }
            (_, true) if self.s.city.trim().is_empty() => {
                p.text(pos2(card.left() + 20.0, card.top() + 20.0), Align2::LEFT_TOP, "Укажите город ниже", semibold(16.0), white);
            }
            (_, true) => {
                let dots = ".".repeat(((now * 3.0) as usize) % 4);
                p.text(pos2(card.left() + 20.0, card.top() + 20.0), Align2::LEFT_TOP, format!("Узнаю погоду{dots}"), semibold(16.0), white);
            }
            _ => {
                p.text(pos2(card.left() + 20.0, card.top() + 20.0), Align2::LEFT_TOP, format!("Сейчас {}, {}", season_name(month), why.part.name()), semibold(16.0), white);
                p.text(pos2(card.left() + 20.0, card.top() + 44.0), Align2::LEFT_TOP, "Погода за окном выключена", font(13.0), alpha(white, 0.85));
            }
        }
        p.text(pos2(card.right() - 18.0, card.top() + 16.0), Align2::RIGHT_TOP, format!("{} · {}", eff.mode.name(), why.source), semibold(13.0), alpha(white, 0.9));

        // город
        if self.city_edit.is_empty() && !self.s.city.is_empty() && !ui.ctx().memory(|m| m.has_focus(egui::Id::new("city"))) {
            self.city_edit = self.s.city.clone();
        }
        let mut search = false;
        let s = &mut self.s;
        group(ui, &pal, Some("Погода за окном"), Some("Снег и дождь — когда они идут на самом деле, их сила и ветер — как на улице. В ясную погоду — эффект по сезону. Данные: Open-Meteo, раз в 15 минут."), |r| {
            ch |= r.toggle("Синхронизировать с погодой", None, &mut s.weather_sync);
            let (rect, _) = r.row(52.0, Sense::hover(), 16.0);
            r.ui.painter().text(pos2(rect.left() + 16.0, rect.center().y), Align2::LEFT_CENTER, "Город", font(15.0), pal.text);
            let er = Rect::from_min_max(pos2(rect.left() + 86.0, rect.top() + 9.0), pos2(rect.right() - 90.0, rect.bottom() - 9.0));
            r.ui.painter().rect_filled(er, Rounding::same(8.0), pal.fill);
            let mut resp = None;
            r.ui.allocate_new_ui(UiBuilder::new().max_rect(er.shrink2(vec2(10.0, 6.0))), |ui| {
                let te = egui::TextEdit::singleline(&mut self.city_edit)
                    .id(egui::Id::new("city"))
                    .hint_text("Например, Москва")
                    .font(font(15.0))
                    .text_color(pal.text)
                    .frame(false)
                    .desired_width(er.width() - 20.0);
                resp = Some(ui.add(te));
            });
            let br = Rect::from_min_max(pos2(rect.right() - 82.0, rect.top() + 9.0), pos2(rect.right() - 12.0, rect.bottom() - 9.0));
            let bresp = r.ui.interact(br, egui::Id::new("city_btn"), Sense::click());
            let down = r.ui.ctx().animate_bool_with_time(bresp.id, bresp.is_pointer_button_down_on(), 0.1);
            r.ui.painter().rect_filled(br, Rounding::same(8.0), alpha(pal.accent, 1.0 - 0.3 * down));
            r.ui.painter().text(br.center(), Align2::CENTER_CENTER, "Найти", semibold(14.0), Color32::WHITE);
            let enter = resp.as_ref().map(|r| r.lost_focus()).unwrap_or(false) && r.ui.input(|i| i.key_pressed(egui::Key::Enter));
            if bresp.clicked() || enter {
                search = true;
            }
            ch |= r.toggle("Реальный ветер", Some("Сила и сторона ветра — как на улице"), &mut s.weather_wind);
        });
        if search {
            let c = self.city_edit.trim().to_string();
            if !c.is_empty() {
                self.s.city = c;
                self.s.weather_sync = true;
                ch = true;
                self.changed(now);
                self.flush(now, true);
                ipc::post(cmd::WEATHER_REFRESH);
            }
        }
        let s = &mut self.s;
        let summer = if s.time_of_day { "лето — бабочки днём и светлячки ночью" } else { "лето — бабочки" };
        let foot = format!("Зима — снег, март–апрель — сакура, май — одуванчики, {summer}, осень — листья. Ночью — холодные оттенки, на рассвете и закате — золотые.");
        group(ui, &pal, Some("Время года и суток"), Some(&foot), |r| {
            ch |= r.toggle("Режим по сезону", None, &mut s.auto_season);
            ch |= r.toggle("Время суток", Some("Оттенки и режим меняются в течение дня"), &mut s.time_of_day);
        });
        group(ui, &pal, None, None, |r| {
            if r.button("Обновить погоду сейчас", pal.accent, true) {
                ipc::post(cmd::WEATHER_REFRESH);
            }
        });
        if ch {
            if self.s.weather_sync || self.s.auto_season {
                self.s.enabled = true;
            }
            self.changed(now);
        }
    }

    fn about_page(&mut self, ui: &mut Ui, now: f64) {
        let pal = self.pal;
        let w = ui.available_width();
        let (r, _) = ui.allocate_exact_size(vec2(w, 232.0), Sense::hover());
        let c = pos2(r.center().x, r.top() + 78.0);
        let bob = ((now as f32) * 1.4).sin() * 2.0;
        let sq = Rect::from_center_size(c + vec2(0.0, bob), vec2(84.0, 84.0));
        let p = ui.painter();
        shadow(p, sq, 4.0, 0.45, 26.0, 10.0 - bob);
        p.rect_filled(sq, Rounding::same(3.0), Color32::BLACK);
        p.rect_stroke(sq, Rounding::same(3.0), Stroke::new(1.0_f32, rgb(58, 58, 58)));
        p.text(pos2(c.x, r.top() + 150.0), Align2::CENTER_CENTER, "Flurry", bold(22.0), pal.text);
        p.text(pos2(c.x, r.top() + 176.0), Align2::CENTER_CENTER, format!("Версия {VERSION}"), font(13.0), pal.text2);
        p.text(pos2(c.x, r.top() + 196.0), Align2::CENTER_CENTER, "Живая погода поверх рабочего стола", font(13.0), pal.text2);
        p.text(pos2(c.x, r.top() + 214.0), Align2::CENTER_CENTER, crate::brand::signature(), font(12.0), pal.text2);
        let mut act = 0;
        group(ui, &pal, None, None, |r| {
            if r.button("Открыть папку настроек", pal.accent, false) { act = 1; }
            if r.button("Перезапустить", pal.accent, false) { act = 2; }
            if r.button("Выйти из Flurry", pal.red, false) { act = 3; }
        });
        group(ui, &pal, Some("Технологии"), Some("Оверлей не перехватывает клики и не попадает в Alt+Tab. Окно настроек — отдельный процесс: закрыли — память освободилась."), |r| {
            r.info("Язык", "Rust");
            r.info("Графика", "Direct3D 11 + DirectComposition");
            r.info("Интерфейс", "egui");
        });
        match act {
            1 => {
                let _ = std::fs::create_dir_all(settings::dir());
                let _ = std::process::Command::new("explorer").arg(settings::dir()).spawn();
            }
            2 => {
                // ждём, пока основной процесс действительно завершится (иначе новый экземпляр
                // увидит мьютекс старого и сразу выйдет — перезапуск «молча» не сработает)
                wait_main_exit(|| {
                    ipc::post(cmd::EXIT);
                });
                launch_main();
                self.next_poll = now + 0.8;
            }
            3 => {
                ipc::post(cmd::EXIT);
                self.next_poll = now + 0.3;
            }
            _ => {}
        }
    }
}

/// Отправляет команду выхода и ждёт (до 3 с), пока основной процесс завершится и освободит мьютекс.
fn wait_main_exit(send_exit: impl FnOnce()) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::*;
    let proc = ipc::main_pid().and_then(|pid| unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid).ok() });
    send_exit();
    match proc {
        Some(h) => unsafe {
            WaitForSingleObject(h, 3000);
            let _ = CloseHandle(h);
        },
        None => std::thread::sleep(std::time::Duration::from_millis(400)),
    }
}

fn launch_main() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--tray").spawn();
    }
}

/// Какой режим выберет «Авто» прямо сейчас (для плитки).
fn eff_auto_mode(s: &Settings, wx: Option<&crate::auto::WeatherState>) -> Mode {
    let mut t = s.clone();
    t.auto_season = true;
    t.weather_sync = !s.city.trim().is_empty();
    crate::auto::effective(&t, wx).0.mode
}

fn compass_name(deg: f32) -> &'static str {
    const N: [&str; 8] = ["вверх", "вверх-вправо", "вправо", "вниз-вправо", "вниз", "вниз-влево", "влево", "вверх-влево"];
    N[(((deg.rem_euclid(360.0) + 22.5) / 45.0) as usize) % 8]
}

fn egui_to_vk(k: egui::Key) -> Option<u32> {
    use egui::Key::*;
    let name = k.name();
    if name.len() == 1 {
        let c = name.chars().next().unwrap();
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase() as u32);
        }
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x6F + n);
        }
    }
    Some(match k {
        Space => 0x20,
        Enter => 0x0D,
        Tab => 0x09,
        Insert => 0x2D,
        Home => 0x24,
        End => 0x23,
        PageUp => 0x21,
        PageDown => 0x22,
        ArrowLeft => 0x25,
        ArrowUp => 0x26,
        ArrowRight => 0x27,
        ArrowDown => 0x28,
        Minus => 0xBD,
        Equals | Plus => 0xBB,
        Comma => 0xBC,
        Period => 0xBE,
        Semicolon => 0xBA,
        Slash => 0xBF,
        Backslash => 0xDC,
        OpenBracket => 0xDB,
        CloseBracket => 0xDD,
        Backtick => 0xC0,
        Quote => 0xDE,
        _ => return None,
    })
}
