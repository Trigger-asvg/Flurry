//! Реальная погода: Open-Meteo (бесплатно, без ключа). Запросы через системный WinHTTP — без лишних зависимостей.

use crate::auto::{unix_now, WeatherState};
use crate::ipc::wide;
use windows::core::{w, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

struct H(*mut core::ffi::c_void);
impl Drop for H {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// HTTPS GET → тело ответа.
pub fn get(host: &str, path: &str) -> Result<String, String> {
    unsafe {
        let ua = wide(concat!("Flurry/", env!("CARGO_PKG_VERSION")));
        let ses = H(WinHttpOpen(PCWSTR(ua.as_ptr()), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0));
        if ses.0.is_null() {
            return Err("нет WinHTTP".into());
        }
        let _ = WinHttpSetTimeouts(ses.0, 8000, 8000, 8000, 10000);
        let h = wide(host);
        let con = H(WinHttpConnect(ses.0, PCWSTR(h.as_ptr()), 443, 0));
        if con.0.is_null() {
            return Err("не удалось подключиться".into());
        }
        let p = wide(path);
        let req = H(WinHttpOpenRequest(con.0, w!("GET"), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE));
        if req.0.is_null() {
            return Err("ошибка запроса".into());
        }
        WinHttpSendRequest(req.0, None, None, 0, 0, 0).map_err(|_| "нет интернета".to_string())?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|_| "сервер не ответил".to_string())?;
        let mut status = 0u32;
        let mut len = 4u32;
        let mut idx = 0u32;
        let _ = WinHttpQueryHeaders(req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut status as *mut _ as _), &mut len, &mut idx);
        let mut body = Vec::new();
        loop {
            let mut avail = 0u32;
            if WinHttpQueryDataAvailable(req.0, &mut avail).is_err() || avail == 0 {
                break;
            }
            let mut buf = vec![0u8; avail as usize];
            let mut read = 0u32;
            if WinHttpReadData(req.0, buf.as_mut_ptr() as _, avail, &mut read).is_err() || read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
            if body.len() > 1 << 20 {
                break;
            }
        }
        if status != 200 {
            return Err(format!("ошибка сервера {status}"));
        }
        String::from_utf8(body).map_err(|_| "битый ответ".into())
    }
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o += &format!("%{b:02X}"),
        }
    }
    o
}

fn hhmm(s: &str) -> i32 {
    // "2026-11-12T07:45"
    s.rsplit('T').next().and_then(|t| {
        let mut it = t.split(':');
        Some(it.next()?.parse::<i32>().ok()? * 60 + it.next()?.parse::<i32>().ok()?)
    }).unwrap_or(-1)
}

/// Город → координаты; потом текущая погода. Возвращает готовое состояние (или с текстом ошибки).
pub fn fetch(city: &str) -> WeatherState {
    let mut st = WeatherState { query: city.trim().to_string(), ..Default::default() };
    match fetch_inner(city, &mut st) {
        Ok(()) => st.updated = unix_now(),
        Err(e) => st.error = e,
    }
    st
}

fn fetch_inner(city: &str, st: &mut WeatherState) -> Result<(), String> {
    if city.trim().is_empty() {
        return Err("Город не указан".into());
    }
    let g = get("geocoding-api.open-meteo.com", &format!("/v1/search?name={}&count=1&language=ru&format=json", enc(city)))?;
    let g: serde_json::Value = serde_json::from_str(&g).map_err(|_| "битый ответ геокодера")?;
    let r = g.get("results").and_then(|r| r.get(0)).ok_or("Город не найден")?;
    let lat = r["latitude"].as_f64().ok_or("Город не найден")?;
    let lon = r["longitude"].as_f64().ok_or("Город не найден")?;
    st.city = r["name"].as_str().unwrap_or(city).to_string();
    st.country = r["country"].as_str().unwrap_or("").to_string();

    let f = get(
        "api.open-meteo.com",
        &format!("/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=temperature_2m,is_day,weather_code,wind_speed_10m,wind_direction_10m&daily=sunrise,sunset&forecast_days=1&timezone=auto"),
    )?;
    let f: serde_json::Value = serde_json::from_str(&f).map_err(|_| "битый ответ погоды")?;
    let c = &f["current"];
    st.temp = c["temperature_2m"].as_f64().unwrap_or(0.0) as f32;
    st.code = c["weather_code"].as_i64().unwrap_or(0) as i32;
    st.wind_kmh = c["wind_speed_10m"].as_f64().unwrap_or(0.0) as f32;
    st.wind_from = c["wind_direction_10m"].as_f64().unwrap_or(0.0) as f32;
    st.is_day = c["is_day"].as_i64().unwrap_or(1) == 1;
    st.now_min = hhmm(c["time"].as_str().unwrap_or(""));
    st.sunrise_min = hhmm(f["daily"]["sunrise"][0].as_str().unwrap_or(""));
    st.sunset_min = hhmm(f["daily"]["sunset"][0].as_str().unwrap_or(""));
    if st.now_min < 0 {
        st.now_min = crate::auto::local_clock().1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encoding() {
        assert_eq!(enc("Moscow"), "Moscow");
        assert_eq!(enc("  New York "), "New%20York");
        assert_eq!(enc("Москва"), "%D0%9C%D0%BE%D1%81%D0%BA%D0%B2%D0%B0");
        assert_eq!(enc("a&b=c?"), "a%26b%3Dc%3F");
    }

    #[test]
    fn time_parsing() {
        assert_eq!(hhmm("2026-11-12T07:45"), 7 * 60 + 45);
        assert_eq!(hhmm("23:05"), 23 * 60 + 5);
        assert_eq!(hhmm(""), -1);
        assert_eq!(hhmm("garbage"), -1);
    }

    #[test]
    fn empty_city_is_error_without_network() {
        let st = fetch("   ");
        assert!(!st.ok());
        assert!(!st.error.is_empty());
    }
}
