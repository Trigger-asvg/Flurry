//! Всплывающая «пилюля» в стиле iOS сверху экрана (при горячих клавишах и переключениях из трея).
//! Рисуется один раз в 32-битный DIB, анимация — только сдвиг и прозрачность через UpdateLayeredWindow.

use crate::ipc::wide;
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForMonitor;
use windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct Osd {
    hwnd: HWND,
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    size: SIZE,
    pos: POINT,
    start: std::time::Instant,
    active: bool,
}

/// Таймер анимации; приходит владельцу (окну основного процесса) как WM_TIMER.
pub const TIMER_ID: usize = 77;
const SHOW: f32 = 1.6;

unsafe extern "system" fn proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if m == WM_NCHITTEST {
        return LRESULT(-1);
    }
    DefWindowProcW(h, m, wp, lp)
}

impl Osd {
    pub fn new() -> Osd {
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: hinst.into(),
                lpszClassName: w!("Flurry.Osd"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                w!("Flurry.Osd"),
                w!(""),
                WS_POPUP,
                0,
                0,
                10,
                10,
                None,
                None,
                HINSTANCE(hinst.0),
                None,
            )
            .unwrap_or_default();
            Osd { hwnd, dc: HDC::default(), bmp: HBITMAP::default(), old: HGDIOBJ::default(), size: SIZE::default(), pos: POINT::default(), start: std::time::Instant::now(), active: false }
        }
    }

    /// Показать текст; `owner` получит WM_TIMER с id 77 для анимации.
    pub fn show(&mut self, owner: HWND, text: &str, dot: [u8; 3]) {
        unsafe {
            // монитор под курсором
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let hm = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(hm, &mut mi);
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(hm, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let k = dx as f32 / 96.0;
            self.render(text, dot, k);
            let wa = mi.rcWork;
            self.pos = POINT { x: wa.left + ((wa.right - wa.left) - self.size.cx) / 2, y: wa.top + (22.0 * k) as i32 };
            self.start = std::time::Instant::now();
            self.active = true;
            self.frame();
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            SetTimer(owner, TIMER_ID, 15, None);
        }
    }

    /// Вызывается по WM_TIMER(77). Возвращает false, когда анимация закончилась.
    pub fn tick(&mut self, owner: HWND) -> bool {
        if !self.active {
            return false;
        }
        let alive = self.frame();
        if !alive {
            self.active = false;
            unsafe {
                let _ = KillTimer(owner, TIMER_ID);
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
        alive
    }

    fn frame(&mut self) -> bool {
        let t = self.start.elapsed().as_secs_f32();
        if t > SHOW + 0.35 {
            return false;
        }
        // появление: выезд сверху с лёгкой пружиной; исчезновение: подъём и затухание
        let (alpha, dy) = if t < 0.35 {
            let x = t / 0.35;
            let e = 1.0 - (1.0 - x).powi(3);
            let spring = (x * std::f32::consts::PI).sin() * 0.08 * (1.0 - x);
            (e, -(1.0 - e - spring) * 26.0)
        } else if t < SHOW {
            (1.0, 0.0)
        } else {
            let x = (t - SHOW) / 0.35;
            let e = x * x;
            (1.0 - e, -e * 14.0)
        };
        unsafe {
            let dst = POINT { x: self.pos.x, y: self.pos.y + dy as i32 };
            let src = POINT::default();
            let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: (alpha.clamp(0.0, 1.0) * 255.0) as u8, AlphaFormat: 1 };
            let _ = UpdateLayeredWindow(self.hwnd, HDC::default(), Some(&dst), Some(&self.size), self.dc, Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        }
        true
    }

    fn free(&mut self) {
        unsafe {
            if !self.dc.is_invalid() {
                SelectObject(self.dc, self.old);
                let _ = DeleteObject(self.bmp);
                let _ = DeleteDC(self.dc);
                self.dc = HDC::default();
            }
        }
    }

    fn render(&mut self, text: &str, dot: [u8; 3], k: f32) {
        self.free();
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(screen);
            ReleaseDC(None, screen);
            let font = CreateFontW(-((15.0 * k) as i32), 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET.0 as u32, 0, 0, ANTIALIASED_QUALITY.0 as u32, 0, w!("Segoe UI"));
            let oldf = SelectObject(dc, font);
            let wt = wide(text);
            let mut ts = SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &wt[..wt.len() - 1], &mut ts);
            let h = (44.0 * k) as i32;
            let pad = (20.0 * k) as i32;
            let dotd = (10.0 * k) as i32;
            let gap = (10.0 * k) as i32;
            let wdt = pad * 2 + dotd + gap + ts.cx;
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER { biSize: 40, biWidth: wdt, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
            let old = SelectObject(dc, bmp);
            // маска текста: белым по чёрному
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0x00FFFFFF));
            let tx = pad + dotd + gap;
            let ty = (h - ts.cy) / 2;
            let _ = TextOutW(dc, tx, ty, &wt[..wt.len() - 1]);
            let _ = GdiFlush();
            SelectObject(dc, oldf);
            let _ = DeleteObject(font);

            let px = std::slice::from_raw_parts_mut(bits as *mut u8, (wdt * h * 4) as usize);
            let rad = h as f32 / 2.0;
            let (bw, bh) = (wdt as f32, h as f32);
            let bg = [28.0f32, 28.0, 32.0];
            let bga = 0.86f32;
            let dcx = (pad + dotd / 2) as f32;
            let dcy = bh / 2.0;
            let dr = dotd as f32 / 2.0;
            for y in 0..h {
                for x in 0..wdt {
                    let i = ((y * wdt + x) * 4) as usize;
                    let cov = px[i + 1] as f32 / 255.0;
                    // скруглённая пилюля
                    let fx = x as f32 + 0.5;
                    let fy = y as f32 + 0.5;
                    let qx = (fx - bw / 2.0).abs() - (bw / 2.0 - rad);
                    let qy = (fy - bh / 2.0).abs() - (bh / 2.0 - rad);
                    let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - rad;
                    let shape = (0.5 - d).clamp(0.0, 1.0);
                    let mut a = shape * bga;
                    let mut c = [bg[0] * a, bg[1] * a, bg[2] * a];
                    // тонкая светлая обводка
                    let edge = (1.0 - (d + 0.8).abs()).clamp(0.0, 1.0) * 0.10;
                    c = [c[0] + 255.0 * edge, c[1] + 255.0 * edge, c[2] + 255.0 * edge];
                    a = (a + edge).min(1.0);
                    // цветная точка режима
                    let dd = ((fx - dcx).powi(2) + (fy - dcy).powi(2)).sqrt() - dr;
                    let dc_ = (0.5 - dd).clamp(0.0, 1.0);
                    if dc_ > 0.0 {
                        for j in 0..3 {
                            c[j] = c[j] * (1.0 - dc_) + dot[j] as f32 * dc_;
                        }
                        a = a * (1.0 - dc_) + dc_;
                    }
                    // текст
                    c = [c[0] * (1.0 - cov) + 255.0 * cov, c[1] * (1.0 - cov) + 255.0 * cov, c[2] * (1.0 - cov) + 255.0 * cov];
                    a = a * (1.0 - cov) + cov;
                    px[i] = c[2].min(255.0) as u8;
                    px[i + 1] = c[1].min(255.0) as u8;
                    px[i + 2] = c[0].min(255.0) as u8;
                    px[i + 3] = (a * 255.0).min(255.0) as u8;
                }
            }
            self.dc = dc;
            self.bmp = bmp;
            self.old = old;
            self.size = SIZE { cx: wdt, cy: h };
        }
    }
}

impl Drop for Osd {
    fn drop(&mut self) {
        self.free();
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
