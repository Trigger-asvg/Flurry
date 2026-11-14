//! Автоматика: время года, время суток и реальная погода → «эффективные» настройки, которые видит рендер.
//! Общая для основного процесса и окна настроек (чтобы превью показывало то же, что на экране).

use crate::settings::{Mode, Settings, Tint};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum DayPart {
    Night,
    Dawn,
    #[default]
    Day,
    Dusk,
}

impl DayPart {
    pub fn name(self) -> &'static str {
        match self {
            DayPart::Night => "ночь",
            DayPart::Dawn => "рассвет",
            DayPart::Day => "день",
            DayPart::Dusk => "закат",
        }
    }
}

/// Состояние погоды, которое основной процесс сохраняет в weather.json (его читает и окно настроек).
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct WeatherState {
    /// Для какого запроса (как ввёл пользователь)
    pub query: String,
    pub city: String,
    pub country: String,
    pub temp: f32,
    /// WMO weather code (Open-Meteo)
    pub code: i32,
    pub wind_kmh: f32,
    /// Откуда дует ветер, градусы (метеорологически)
    pub wind_from: f32,
    pub is_day: bool,
    /// Минуты от полуночи по местному времени города
    pub now_min: i32,
    pub sunrise_min: i32,
    pub sunset_min: i32,
    /// Когда получено (unix, сек)
    pub updated: u64,
    pub error: String,
}

/// Тот же ли город (без учёта регистра и пробелов по краям; работает и для кириллицы).
pub fn same_city(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl WeatherState {
    pub fn ok(&self) -> bool {
        self.error.is_empty() && self.updated > 0
    }
    /// Данные не старше 3 часов
    pub fn fresh(&self) -> bool {
        self.ok() && unix_now().saturating_sub(self.updated) < 3 * 3600
    }
    pub fn describe(&self) -> &'static str {
        describe_code(self.code)
    }
    /// Местное время города сейчас (с поправкой на прошедшее с запроса время)
    pub fn local_min(&self) -> i32 {
        let passed = (unix_now().saturating_sub(self.updated) / 60) as i32;
        (self.now_min + passed).rem_euclid(1440)
    }
    pub fn day_part(&self) -> DayPart {
        part_by_sun(self.local_min(), self.sunrise_min, self.sunset_min)
    }
}

pub fn describe_code(c: i32) -> &'static str {
    match c {
        0 => "Ясно",
        1 => "Преимущественно ясно",
        2 => "Переменная облачность",
        3 => "Пасмурно",
        45 | 48 => "Туман",
        51 | 53 | 55 => "Морось",
        56 | 57 => "Ледяная морось",
        61 => "Небольшой дождь",
        63 => "Дождь",
        65 => "Сильный дождь",
        66 | 67 => "Ледяной дождь",
        71 => "Небольшой снег",
        73 => "Снег",
        75 => "Сильный снег",
        77 => "Снежная крупа",
        80 => "Ливень",
        81 => "Ливень",
        82 => "Сильный ливень",
        85 => "Снегопад",
        86 => "Сильный снегопад",
        95 => "Гроза",
        96 | 99 => "Гроза с градом",
        _ => "—",
    }
}

fn part_by_sun(now: i32, rise: i32, set: i32) -> DayPart {
    let (rise, set) = if rise <= 0 || set <= rise { (7 * 60, 19 * 60) } else { (rise, set) };
    if (now - rise).abs() <= 45 {
        DayPart::Dawn
    } else if (now - set).abs() <= 45 {
        DayPart::Dusk
    } else if now > rise && now < set {
        DayPart::Day
    } else {
        DayPart::Night
    }
}

/// Местное время компьютера: (месяц, минуты от полуночи).
pub fn local_clock() -> (u32, i32) {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (t.wMonth as u32, t.wHour as i32 * 60 + t.wMinute as i32)
}

/// Время суток по часам компьютера (если город не задан): восход/закат по сезону примерно.
pub fn clock_day_part(month: u32, min: i32) -> DayPart {
    // грубо для средних широт: летом день длиннее
    let (rise, set) = match month {
        12 | 1 => (8 * 60 + 30, 16 * 60 + 30),
        2 | 11 => (7 * 60 + 45, 17 * 60 + 15),
        3 | 10 => (6 * 60 + 45, 18 * 60 + 15),
        4 | 9 => (6 * 60, 19 * 60 + 15),
        _ => (5 * 60, 21 * 60),
    };
    part_by_sun(min, rise, set)
}

/// Режим по времени года (и суток: летом днём — бабочки, ночью — светлячки).
pub fn season_mode(month: u32, part: DayPart) -> Mode {
    match month {
        12 | 1 | 2 => Mode::Snow,
        3 | 4 => Mode::Sakura,
        5 => Mode::Dandelions,
        6..=8 => {
            if part == DayPart::Night || part == DayPart::Dusk {
                Mode::Fireflies
            } else {
                Mode::Butterflies
            }
        }
        _ => Mode::Leaves,
    }
}

pub fn season_name(month: u32) -> &'static str {
    match month {
        12 | 1 | 2 => "зима",
        3..=5 => "весна",
        6..=8 => "лето",
        _ => "осень",
    }
}

/// Что сейчас происходит и почему — для подписи в интерфейсе.
#[derive(Clone, Debug, Default)]
pub struct Why {
    pub source: &'static str,
    pub part: DayPart,
}

/// Настройки, которые реально уходят в рендер.
pub fn effective(s: &Settings, wx: Option<&WeatherState>) -> (Settings, Why) {
    let mut e = s.clone();
    let (month, min) = local_clock();
    let wx = wx.filter(|w| s.weather_sync && w.fresh() && same_city(&w.query, &s.city));
    let part = match wx {
        Some(w) => w.day_part(),
        None => clock_day_part(month, min),
    };
    let mut why = Why { source: "вручную", part };

    let season = season_mode(month, if s.time_of_day { part } else { DayPart::Day });
    if s.auto_season {
        e.mode = season;
        why.source = "по сезону";
    }

    if let Some(w) = wx {
        why.source = "по погоде";
        let c = w.code;
        let snow = matches!(c, 71..=77 | 85 | 86);
        let rain = matches!(c, 51..=67 | 80..=82 | 95..=99);
        if snow {
            e.mode = Mode::Snow;
            e.density *= match c {
                71 | 77 => 0.6,
                75 | 86 => 1.7,
                _ => 1.0,
            };
        } else if rain {
            e.mode = Mode::Rain;
            e.density *= match c {
                51 | 56 | 61 | 80 => 0.55,
                53 | 63 | 81 | 66 => 1.0,
                _ => 1.6,
            };
            e.lightning = c >= 95;
        } else {
            e.mode = season;
            // в туман и пасмурно частиц меньше
            if matches!(c, 3 | 45 | 48) {
                e.density *= 0.75;
            }
        }
        if s.weather_wind {
            // экран — вид «сбоку»: берём только восточно-западную составляющую ветра
            let toward = (w.wind_from + 180.0).to_radians();
            let east = toward.sin();
            e.wind_angle = if east >= 0.0 { 90.0 } else { 270.0 };
            let k = (w.wind_kmh / 45.0).clamp(0.0, 1.0);
            e.wind = (k * (0.35 + 0.65 * east.abs())).clamp(0.02, 1.0);
            e.gusts = w.wind_kmh > 12.0;
        }
    }

    if s.time_of_day && s.tint == Tint::Natural {
        match part {
            DayPart::Night => {
                if matches!(e.mode, Mode::Snow | Mode::Rain | Mode::Bubbles | Mode::Dandelions) {
                    e.tint = Tint::Ice;
                }
                e.opacity *= 0.9;
            }
            DayPart::Dawn | DayPart::Dusk => {
                if matches!(e.mode, Mode::Snow | Mode::Dandelions | Mode::Bubbles) {
                    e.tint = Tint::Gold;
                }
            }
            DayPart::Day => {}
        }
    }
    e.sanitize();
    (e, why)
}

pub fn wx_path() -> std::path::PathBuf {
    crate::settings::dir().join("weather.json")
}

pub fn load_wx() -> Option<WeatherState> {
    std::fs::read_to_string(wx_path()).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
}

pub fn save_wx(w: &WeatherState) {
    let _ = std::fs::create_dir_all(crate::settings::dir());
    if let Ok(j) = serde_json::to_string_pretty(w) {
        let tmp = crate::settings::dir().join("weather.json.tmp");
        if std::fs::write(&tmp, j).is_ok() {
            let _ = std::fs::rename(tmp, wx_path());
        }
    }
}

pub fn compass16(from_deg: f32) -> &'static str {
    const N: [&str; 8] = ["северный", "северо-восточный", "восточный", "юго-восточный", "южный", "юго-западный", "западный", "северо-западный"];
    N[(((from_deg.rem_euclid(360.0) + 22.5) / 45.0) as usize) % 8]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wx(code: i32, wind_kmh: f32, wind_from: f32) -> WeatherState {
        WeatherState { query: "Москва".into(), city: "Москва".into(), code, wind_kmh, wind_from, now_min: 12 * 60, sunrise_min: 7 * 60, sunset_min: 17 * 60, updated: unix_now(), ..Default::default() }
    }

    fn synced() -> Settings {
        Settings { weather_sync: true, city: " москва ".into(), ..Settings::default() }
    }

    #[test]
    fn day_parts_by_sun() {
        assert_eq!(part_by_sun(7 * 60, 7 * 60, 19 * 60), DayPart::Dawn);
        assert_eq!(part_by_sun(12 * 60, 7 * 60, 19 * 60), DayPart::Day);
        assert_eq!(part_by_sun(19 * 60 + 30, 7 * 60, 19 * 60), DayPart::Dusk);
        assert_eq!(part_by_sun(23 * 60, 7 * 60, 19 * 60), DayPart::Night);
        assert_eq!(part_by_sun(3 * 60, 7 * 60, 19 * 60), DayPart::Night);
        // битые данные о солнце -> разумные значения по умолчанию
        assert_eq!(part_by_sun(12 * 60, -1, -1), DayPart::Day);
    }

    #[test]
    fn seasons() {
        assert_eq!(season_mode(1, DayPart::Day), Mode::Snow);
        assert_eq!(season_mode(4, DayPart::Day), Mode::Sakura);
        assert_eq!(season_mode(5, DayPart::Day), Mode::Dandelions);
        assert_eq!(season_mode(7, DayPart::Day), Mode::Butterflies);
        assert_eq!(season_mode(7, DayPart::Night), Mode::Fireflies);
        assert_eq!(season_mode(10, DayPart::Night), Mode::Leaves);
        assert_eq!(season_name(12), "зима");
    }

    #[test]
    fn weather_drives_mode() {
        let s = synced();
        let (e, why) = effective(&s, Some(&wx(75, 0.0, 0.0)));
        assert_eq!(e.mode, Mode::Snow);
        assert_eq!(why.source, "по погоде");
        assert!(e.density > s.density, "сильный снег — больше частиц");

        let (e, _) = effective(&s, Some(&wx(95, 0.0, 0.0)));
        assert_eq!(e.mode, Mode::Rain);
        assert!(e.lightning);

        let (e, _) = effective(&s, Some(&wx(61, 0.0, 0.0)));
        assert_eq!(e.mode, Mode::Rain);
        assert!(!e.lightning);
    }

    #[test]
    fn weather_ignored_when_off_stale_or_other_city() {
        let mut s = synced();
        s.mode = Mode::Bubbles;
        s.weather_sync = false;
        assert_eq!(effective(&s, Some(&wx(75, 0.0, 0.0))).0.mode, Mode::Bubbles);

        s.weather_sync = true;
        let mut old = wx(75, 0.0, 0.0);
        old.updated = unix_now() - 4 * 3600;
        assert_eq!(effective(&s, Some(&old)).0.mode, Mode::Bubbles);

        let mut other = wx(75, 0.0, 0.0);
        other.query = "Париж".into();
        assert_eq!(effective(&s, Some(&other)).0.mode, Mode::Bubbles);
    }

    #[test]
    fn real_wind_direction() {
        let s = synced();
        // западный ветер (дует с запада) -> частицы летят вправо
        let (e, _) = effective(&s, Some(&wx(0, 30.0, 270.0)));
        assert_eq!(e.wind_angle, 90.0);
        assert!(e.wind > 0.3);
        // восточный -> влево
        let (e, _) = effective(&s, Some(&wx(0, 30.0, 90.0)));
        assert_eq!(e.wind_angle, 270.0);
        // штиль
        let (e, _) = effective(&s, Some(&wx(0, 0.0, 90.0)));
        assert!(e.wind <= 0.02 + f32::EPSILON);
        assert!(!e.gusts);
    }

    #[test]
    fn compass_and_codes() {
        assert_eq!(compass16(0.0), "северный");
        assert_eq!(compass16(359.0), "северный");
        assert_eq!(compass16(-90.0), "западный");
        assert_eq!(compass16(135.0), "юго-восточный");
        assert_eq!(describe_code(0), "Ясно");
        assert_eq!(describe_code(1234), "—");
    }
}
