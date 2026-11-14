//! Настройки (общие для процесса оверлея и окна настроек). Хранятся в %APPDATA%\Flurry\settings.json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Snow,
    Sakura,
    Leaves,
    Rain,
    Fireflies,
    Confetti,
    Bubbles,
    Butterflies,
    Dandelions,
}

impl Mode {
    pub const ALL: [Mode; 9] = [
        Mode::Snow,
        Mode::Sakura,
        Mode::Dandelions,
        Mode::Butterflies,
        Mode::Leaves,
        Mode::Rain,
        Mode::Fireflies,
        Mode::Confetti,
        Mode::Bubbles,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Mode::Snow => "Снег",
            Mode::Sakura => "Сакура",
            Mode::Leaves => "Листья",
            Mode::Rain => "Дождь",
            Mode::Fireflies => "Светлячки",
            Mode::Confetti => "Конфетти",
            Mode::Bubbles => "Пузыри",
            Mode::Butterflies => "Бабочки",
            Mode::Dandelions => "Одуванчики",
        }
    }
    pub fn index(self) -> usize {
        Mode::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }
    pub fn from_index(i: usize) -> Mode {
        Mode::ALL[i % Mode::ALL.len()]
    }
    pub fn next(self) -> Mode {
        Mode::from_index(self.index() + 1)
    }
    /// Акцентный цвет режима (для OSD и интерфейса), sRGB.
    pub fn accent(self) -> [u8; 3] {
        match self {
            Mode::Snow => [140, 200, 255],
            Mode::Sakura => [255, 150, 190],
            Mode::Leaves => [255, 150, 60],
            Mode::Rain => [100, 160, 255],
            Mode::Fireflies => [210, 255, 90],
            Mode::Confetti => [255, 90, 160],
            Mode::Bubbles => [120, 230, 255],
            Mode::Butterflies => [255, 170, 60],
            Mode::Dandelions => [250, 245, 210],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseMode {
    Off,
    #[default]
    Repel,
    Attract,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tint {
    #[default]
    Natural,
    Ice,
    Gold,
    Pink,
    Mint,
    Rainbow,
}

impl Tint {
    pub const ALL: [Tint; 6] = [Tint::Natural, Tint::Ice, Tint::Gold, Tint::Pink, Tint::Mint, Tint::Rainbow];
    pub fn name(self) -> &'static str {
        match self {
            Tint::Natural => "Как есть",
            Tint::Ice => "Лёд",
            Tint::Gold => "Золото",
            Tint::Pink => "Роза",
            Tint::Mint => "Мята",
            Tint::Rainbow => "Радуга",
        }
    }
    /// Цвет оттенка (линейный 0..1). Для Natural/Rainbow не используется.
    pub fn rgb(self) -> [f32; 3] {
        match self {
            Tint::Natural => [1.0, 1.0, 1.0],
            Tint::Ice => [0.62, 0.85, 1.0],
            Tint::Gold => [1.0, 0.80, 0.38],
            Tint::Pink => [1.0, 0.55, 0.78],
            Tint::Mint => [0.5, 1.0, 0.8],
            Tint::Rainbow => [1.0, 1.0, 1.0],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

/// Сочетание клавиш: модификаторы как в RegisterHotKey (1 Alt, 2 Ctrl, 4 Shift, 8 Win) + виртуальный код клавиши.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hotkey {
    pub mods: u32,
    pub vk: u32,
}

impl Hotkey {
    pub const fn new(mods: u32, vk: u32) -> Self {
        Self { mods, vk }
    }
    pub fn is_set(self) -> bool {
        self.vk != 0
    }
    pub fn describe(self) -> String {
        if !self.is_set() {
            return "Не задано".into();
        }
        let mut s = String::new();
        if self.mods & 2 != 0 { s += "Ctrl + "; }
        if self.mods & 1 != 0 { s += "Alt + "; }
        if self.mods & 4 != 0 { s += "Shift + "; }
        if self.mods & 8 != 0 { s += "Win + "; }
        s += &vk_name(self.vk);
        s
    }
}

pub fn vk_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x20 => "Space".into(),
        0x0D => "Enter".into(),
        0x09 => "Tab".into(),
        0x08 => "Backspace".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x25 => "←".into(),
        0x26 => "↑".into(),
        0x27 => "→".into(),
        0x28 => "↓".into(),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0xBA => ";".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0xC0 => "`".into(),
        0xDB => "[".into(),
        0xDC => "\\".into(),
        0xDD => "]".into(),
        0xDE => "'".into(),
        0x13 => "Pause".into(),
        _ => format!("0x{vk:02X}"),
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub mode: Mode,
    pub auto_season: bool,

    // частицы
    pub speed: f32,
    pub density: f32,
    pub size: f32,
    pub opacity: f32,
    pub depth3d: bool,
    pub bokeh: bool,
    pub tint: Tint,

    // ветер
    /// Направление, куда дует ветер, в градусах по компасу (0 — вверх, 90 — вправо, 180 — вниз, 270 — влево).
    pub wind_angle: f32,
    /// 0..1
    pub wind: f32,
    pub gusts: bool,
    /// Покачивание частиц 0..2
    pub turbulence: f32,

    // мышь
    pub mouse: MouseMode,
    pub mouse_strength: f32,
    pub mouse_radius: f32,
    pub cursor_trail: bool,
    pub pop_bubbles: bool,

    // эффекты погоды
    pub rain_fog: bool,
    pub lightning: bool,
    pub splashes: bool,
    /// Длительность плавной смены режима, сек
    pub crossfade: f32,

    // производительность
    /// 0 — авто (30…60 по скорости частиц), иначе фиксированная частота
    pub fps: u32,
    pub pause_fullscreen: bool,
    pub battery_saver: bool,
    pub primary_only: bool,
    pub hide_from_capture: bool,

    // основное
    pub autostart: bool,
    pub show_osd: bool,
    pub theme: Theme,

    // погода и время
    /// Синхронизация с реальной погодой в городе
    pub weather_sync: bool,
    pub city: String,
    /// Брать ветер (силу и сторону) из реальной погоды
    pub weather_wind: bool,
    /// Время суток: ночью холодные оттенки, на рассвете/закате — золото; летом днём бабочки, ночью светлячки
    pub time_of_day: bool,

    pub hk_toggle: Hotkey,
    pub hk_settings: Hotkey,
    pub hk_mode: Hotkey,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: Mode::Snow,
            auto_season: false,
            speed: 1.0,
            density: 1.0,
            size: 1.0,
            opacity: 0.9,
            depth3d: true,
            bokeh: true,
            tint: Tint::Natural,
            wind_angle: 90.0,
            wind: 0.15,
            gusts: true,
            turbulence: 1.0,
            mouse: MouseMode::Repel,
            mouse_strength: 1.0,
            mouse_radius: 1.0,
            cursor_trail: true,
            pop_bubbles: true,
            rain_fog: true,
            lightning: true,
            splashes: true,
            crossfade: 1.5,
            fps: 60,
            pause_fullscreen: true,
            battery_saver: true,
            primary_only: false,
            hide_from_capture: false,
            autostart: true,
            show_osd: true,
            theme: Theme::System,
            weather_sync: false,
            city: String::new(),
            weather_wind: true,
            time_of_day: true,
            hk_toggle: Hotkey::new(2 | 1, 0x53),   // Ctrl+Alt+S
            hk_settings: Hotkey::new(2 | 1, 0x4F), // Ctrl+Alt+O
            hk_mode: Hotkey::new(2 | 1, 0x4D),     // Ctrl+Alt+M
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) {
        self.speed = self.speed.clamp(0.1, 3.0);
        self.density = self.density.clamp(0.05, 3.0);
        self.size = self.size.clamp(0.4, 2.5);
        self.opacity = self.opacity.clamp(0.05, 1.0);
        self.wind = self.wind.clamp(0.0, 1.0);
        self.wind_angle = self.wind_angle.rem_euclid(360.0);
        self.turbulence = self.turbulence.clamp(0.0, 2.0);
        self.mouse_strength = self.mouse_strength.clamp(0.1, 2.0);
        self.mouse_radius = self.mouse_radius.clamp(0.3, 2.5);
        self.crossfade = self.crossfade.clamp(0.0, 5.0);
        if ![0, 30, 60, 120, 144].contains(&self.fps) {
            self.fps = 0;
        }
        for hk in [&mut self.hk_toggle, &mut self.hk_settings, &mut self.hk_mode] {
            hk.mods &= 0xF; // только Alt/Ctrl/Shift/Win
            if hk.vk > 0xFE {
                *hk = Hotkey::new(0, 0);
            }
        }
        if self.city.chars().count() > 100 {
            self.city = self.city.chars().take(100).collect();
        }
    }

    pub fn hotkeys(&self) -> [(i32, Hotkey); 3] {
        [(1, self.hk_toggle), (2, self.hk_settings), (3, self.hk_mode)]
    }
}

pub fn dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Flurry")
}

pub fn path() -> PathBuf {
    dir().join("settings.json")
}

pub fn exists() -> bool {
    path().exists()
}

pub fn load() -> Settings {
    let mut s = std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str::<Settings>(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    s.sanitize();
    s
}

pub fn save(s: &Settings) {
    let _ = std::fs::create_dir_all(dir());
    if let Ok(json) = serde_json::to_string_pretty(s) {
        let tmp = dir().join("settings.json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path());
        }
    }
}

pub fn modified() -> Option<std::time::SystemTime> {
    std::fs::metadata(path()).and_then(|m| m.modified()).ok()
}

pub fn log(msg: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(dir());
    let file = dir().join("log.txt");
    // простая ротация: лог не растёт бесконечно (приложение может работать месяцами)
    if std::fs::metadata(&file).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&file, dir().join("log.old.txt"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{t}] {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_cycle_and_cli_names() {
        assert_eq!(Mode::ALL.len(), 9);
        for (i, m) in Mode::ALL.iter().enumerate() {
            assert_eq!(m.index(), i);
            assert_eq!(Mode::from_index(i), *m);
        }
        assert_eq!(Mode::Bubbles.next(), Mode::Snow);
        // имена для --mode из README
        let names: Vec<String> = Mode::ALL.iter().map(|m| format!("{m:?}").to_lowercase()).collect();
        assert_eq!(names, ["snow", "sakura", "dandelions", "butterflies", "leaves", "rain", "fireflies", "confetti", "bubbles"]);
    }

    #[test]
    fn sanitize_clamps_everything() {
        let mut s = Settings { speed: 99.0, density: -1.0, size: 0.0, opacity: 7.0, wind: 3.0, wind_angle: -90.0, fps: 75, ..Settings::default() };
        s.hk_toggle = Hotkey::new(0xFFFF, 0x53);
        s.hk_mode = Hotkey::new(2, 0x1234);
        s.city = "я".repeat(500);
        s.sanitize();
        assert_eq!(s.speed, 3.0);
        assert_eq!(s.density, 0.05);
        assert_eq!(s.size, 0.4);
        assert_eq!(s.opacity, 1.0);
        assert_eq!(s.wind, 1.0);
        assert_eq!(s.wind_angle, 270.0);
        assert_eq!(s.fps, 0);
        assert_eq!(s.hk_toggle.mods, 0xF);
        assert!(!s.hk_mode.is_set());
        assert_eq!(s.city.chars().count(), 100);
    }

    #[test]
    fn json_roundtrip_and_partial_files() {
        let s = Settings::default();
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&j).unwrap(), s);
        // старый/неполный файл: недостающие поля берутся по умолчанию
        let p: Settings = serde_json::from_str(r#"{"mode":"Rain","speed":2.0}"#).unwrap();
        assert_eq!(p.mode, Mode::Rain);
        assert_eq!(p.speed, 2.0);
        assert_eq!(p.hk_toggle, Settings::default().hk_toggle);
    }

    #[test]
    fn hotkey_names() {
        assert_eq!(Settings::default().hk_toggle.describe(), "Ctrl + Alt + S");
        assert_eq!(Hotkey::new(0, 0).describe(), "Не задано");
        assert_eq!(Hotkey::new(4 | 8, 0x70).describe(), "Shift + Win + F1");
        assert_eq!(vk_name(0x65), "Num 5");
        assert_eq!(vk_name(0xFF), "0xFF");
    }
}
