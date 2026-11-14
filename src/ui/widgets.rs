//! Виджеты в духе iOS: группы-карточки, переключатели, слайдеры, сегменты, иконки, градиенты, превью погоды.

use crate::settings::{Mode, Settings, Tint};
use eframe::egui::{self, epaint, Color32, FontFamily, FontId, Pos2, Rect, Response, Rounding, Sense, Shape, Stroke, Ui, Vec2};
use egui::{pos2, vec2};
use std::f32::consts::PI;

#[derive(Clone, Copy)]
pub struct Pal {
    pub dark: bool,
    pub bg: Color32,
    pub card: Color32,
    pub press: Color32,
    pub sep: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub accent: Color32,
    pub green: Color32,
    pub red: Color32,
    pub orange: Color32,
    pub track: Color32,
    pub fill: Color32,
    pub seg_bg: Color32,
    pub seg_pill: Color32,
}

pub const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Pal {
    pub fn new(dark: bool) -> Pal {
        if dark {
            Pal {
                dark,
                bg: rgb(0, 0, 0),
                card: rgb(28, 28, 30),
                press: rgb(44, 44, 46),
                sep: rgb(56, 56, 58),
                text: rgb(255, 255, 255),
                text2: rgb(142, 142, 147),
                accent: rgb(10, 132, 255),
                green: rgb(48, 209, 88),
                red: rgb(255, 69, 58),
                orange: rgb(255, 159, 10),
                track: rgb(57, 57, 61),
                fill: rgb(58, 58, 60),
                seg_bg: rgb(36, 36, 39),
                seg_pill: rgb(99, 99, 102),
            }
        } else {
            Pal {
                dark,
                bg: rgb(242, 242, 247),
                card: rgb(255, 255, 255),
                press: rgb(229, 229, 234),
                sep: rgb(218, 218, 222),
                text: rgb(0, 0, 0),
                text2: rgb(138, 138, 142),
                accent: rgb(0, 122, 255),
                green: rgb(52, 199, 89),
                red: rgb(255, 59, 48),
                orange: rgb(255, 149, 0),
                track: rgb(233, 233, 234),
                fill: rgb(229, 229, 234),
                seg_bg: rgb(227, 227, 232),
                seg_pill: rgb(255, 255, 255),
            }
        }
    }
}

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("sb".into()))
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn lerp_c(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn alpha(c: Color32, a: f32) -> Color32 {
    let a = a.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (c.a() as f32 * a) as u8)
}

pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}
pub fn shadow(p: &egui::Painter, rect: Rect, r: f32, a: f32, blur: f32, dy: f32) {
    let s = epaint::Shadow { offset: vec2(0.0, dy), blur, spread: 0.0, color: Color32::from_black_alpha((a * 255.0) as u8) };
    p.add(s.as_shape(rect, r));
}

/// Скруглённый прямоугольник с вертикальным градиентом (меш-веер, точный для линейного градиента).
pub fn gradient(p: &egui::Painter, rect: Rect, r: f32, top: Color32, bottom: Color32) {
    let mut mesh = epaint::Mesh::default();
    let col = |y: f32| lerp_c(top, bottom, (y - rect.top()) / rect.height().max(1.0));
    let c = rect.center();
    mesh.colored_vertex(c, col(c.y));
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -PI / 2.0),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), PI / 2.0),
        (pos2(rect.left() + r, rect.top() + r), PI),
    ];
    let seg = 8;
    for (cc, a0) in corners {
        for i in 0..=seg {
            let a = a0 + (PI / 2.0) * i as f32 / seg as f32;
            let pt = pos2(cc.x + r * a.cos(), cc.y + r * a.sin());
            mesh.colored_vertex(pt, col(pt.y));
        }
    }
    let n = mesh.vertices.len() as u32;
    for i in 1..n {
        let j = if i + 1 < n { i + 1 } else { 1 };
        mesh.add_triangle(0, i, j);
    }
    p.add(Shape::mesh(mesh));
}

// ---------------------------------------------------------------- переключатель

pub fn paint_switch(ui: &Ui, pal: &Pal, rect: Rect, resp: &Response, on: bool) {
    let ctx = ui.ctx();
    let t = ease_in_out(ctx.animate_bool_with_time(resp.id.with("on"), on, 0.25));
    let press = ease_out(ctx.animate_bool_with_time(resp.id.with("press"), resp.is_pointer_button_down_on(), 0.15));
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(rect.height() / 2.0), lerp_c(pal.track, pal.green, t));
    let kr = rect.height() / 2.0 - 2.0;
    let stretch = 7.0 * press;
    let left = rect.left() + 2.0;
    let right = rect.right() - 2.0 - kr * 2.0 - stretch;
    let x = left + (right - left) * t;
    let knob = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(kr * 2.0 + stretch, kr * 2.0));
    shadow(p, knob, kr, 0.22, 6.0, 2.0);
    p.rect_filled(knob, Rounding::same(kr), Color32::WHITE);
}

// ---------------------------------------------------------------- слайдер

pub fn slider(ui: &mut Ui, pal: &Pal, v: &mut f32, min: f32, max: f32, width: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click_and_drag());
    let mut changed = false;
    let pad = 14.0;
    let (l, r) = (rect.left() + pad, rect.right() - pad);
    if resp.dragged() || resp.clicked() {
        if let Some(pt) = resp.interact_pointer_pos() {
            let t = ((pt.x - l) / (r - l)).clamp(0.0, 1.0);
            let nv = min + (max - min) * t;
            if (nv - *v).abs() > f32::EPSILON {
                *v = nv;
                changed = true;
            }
        }
    }
    let t = ((*v - min) / (max - min)).clamp(0.0, 1.0);
    // плавное движение ползунка при клике по дорожке
    let shown = ui.ctx().animate_value_with_time(resp.id.with("v"), t, if resp.dragged() { 0.0 } else { 0.12 });
    let x = l + (r - l) * shown;
    let p = ui.painter();
    let cy = rect.center().y;
    p.rect_filled(Rect::from_min_max(pos2(l, cy - 2.0), pos2(r, cy + 2.0)), Rounding::same(2.0), pal.fill);
    p.rect_filled(Rect::from_min_max(pos2(l, cy - 2.0), pos2(x, cy + 2.0)), Rounding::same(2.0), pal.accent);
    let grow = ease_out(ui.ctx().animate_bool_with_time(resp.id.with("g"), resp.dragged() || resp.is_pointer_button_down_on(), 0.15));
    let kr = 13.0 + 2.0 * grow;
    let knob = Rect::from_center_size(pos2(x, cy), vec2(kr * 2.0, kr * 2.0));
    shadow(p, knob, kr, 0.25, 8.0, 2.0);
    p.circle_filled(pos2(x, cy), kr, Color32::WHITE);
    changed
}

// ---------------------------------------------------------------- сегменты

pub fn segmented(ui: &mut Ui, pal: &Pal, idx: &mut usize, labels: &[&str], width: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
    let n = labels.len().max(1);
    let sw = (rect.width() - 4.0) / n as f32;
    let mut changed = false;
    if resp.clicked() {
        if let Some(pt) = resp.interact_pointer_pos() {
            let i = (((pt.x - rect.left() - 2.0) / sw) as usize).min(n - 1);
            if i != *idx {
                *idx = i;
                changed = true;
            }
        }
    }
    let pos = ui.ctx().animate_value_with_time(resp.id.with("seg"), *idx as f32, 0.28);
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(9.0), pal.seg_bg);
    let pill = Rect::from_min_size(pos2(rect.left() + 2.0 + pos * sw, rect.top() + 2.0), vec2(sw, rect.height() - 4.0));
    shadow(p, pill, 7.0, if pal.dark { 0.3 } else { 0.12 }, 6.0, 1.5);
    p.rect_filled(pill, Rounding::same(7.0), pal.seg_pill);
    for (i, l) in labels.iter().enumerate() {
        let c = pos2(rect.left() + 2.0 + sw * (i as f32 + 0.5), rect.center().y);
        let sel = 1.0 - (pos - i as f32).abs().min(1.0);
        if i > 0 && (pos - i as f32).abs() > 0.9 && (pos - (i as f32 - 1.0)).abs() > 0.9 {
            p.line_segment([pos2(rect.left() + 2.0 + sw * i as f32, rect.top() + 9.0), pos2(rect.left() + 2.0 + sw * i as f32, rect.bottom() - 9.0)], Stroke::new(1.0_f32, alpha(pal.text2, 0.35)));
        }
        let f = if sel > 0.5 { semibold(13.0) } else { font(13.0) };
        p.text(c, egui::Align2::CENTER_CENTER, *l, f, pal.text);
    }
    changed
}

// ---------------------------------------------------------------- иконки

#[derive(Clone, Copy)]
pub enum Icon {
    Flake,
    Wind,
    Cursor,
    Bolt,
    Gauge,
    Key,
    Gear,
    Info,
    Sparkle,
    Next,
    Drop,
    Cloud,
}

pub fn icon(p: &egui::Painter, c: Pos2, s: f32, kind: Icon, col: Color32) {
    let st = Stroke::new(s * 0.11, col);
    match kind {
        Icon::Flake => {
            for i in 0..6 {
                let a = i as f32 * PI / 3.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c, c + d * s * 0.42], st);
                let m = c + d * s * 0.26;
                let b1 = vec2((a + 0.8).cos(), (a + 0.8).sin());
                let b2 = vec2((a - 0.8).cos(), (a - 0.8).sin());
                p.line_segment([m, m + b1 * s * 0.12], Stroke::new(s * 0.08, col));
                p.line_segment([m, m + b2 * s * 0.12], Stroke::new(s * 0.08, col));
            }
        }
        Icon::Wind => {
            for (i, (len, dy)) in [(0.62, -0.22), (0.78, 0.0), (0.5, 0.22)].iter().enumerate() {
                let y = c.y + dy * s;
                let x0 = c.x - s * 0.4;
                let x1 = x0 + len * s;
                p.line_segment([pos2(x0, y), pos2(x1, y)], st);
                if i != 1 {
                    p.circle_stroke(pos2(x1, y - dy.signum() * s * 0.08), s * 0.08, Stroke::new(s * 0.09, col));
                }
            }
        }
        Icon::Cursor => {
            let pts = vec![
                c + vec2(-0.22, -0.38) * s,
                c + vec2(-0.22, 0.26) * s,
                c + vec2(-0.06, 0.12) * s,
                c + vec2(0.06, 0.38) * s,
                c + vec2(0.16, 0.33) * s,
                c + vec2(0.04, 0.08) * s,
                c + vec2(0.25, 0.08) * s,
            ];
            p.add(Shape::closed_line(pts, Stroke::new(s * 0.08, col)));
        }
        Icon::Bolt => {
            let pts = vec![c + vec2(0.06, -0.42) * s, c + vec2(-0.24, 0.06) * s, c + vec2(-0.02, 0.06) * s, c + vec2(-0.08, 0.42) * s, c + vec2(0.24, -0.08) * s, c + vec2(0.02, -0.08) * s];
            p.add(Shape::convex_polygon(pts[..3].to_vec(), col, Stroke::NONE));
            p.add(Shape::closed_line(pts, Stroke::new(s * 0.06, col)));
            p.add(Shape::convex_polygon(vec![c + vec2(0.06, -0.42) * s, c + vec2(-0.24, 0.06) * s, c + vec2(0.0, 0.0) * s], col, Stroke::NONE));
            p.add(Shape::convex_polygon(vec![c + vec2(-0.08, 0.42) * s, c + vec2(0.24, -0.08) * s, c + vec2(0.0, 0.0) * s], col, Stroke::NONE));
        }
        Icon::Gauge => {
            let r = s * 0.36;
            let n = 16;
            let pts: Vec<Pos2> = (0..=n).map(|i| {
                let a = PI * 0.8 + (PI * 1.4) * i as f32 / n as f32;
                c + vec2(a.cos(), a.sin()) * r + vec2(0.0, s * 0.06)
            }).collect();
            p.add(Shape::line(pts, st));
            let a = -PI * 0.3;
            p.line_segment([c + vec2(0.0, s * 0.06), c + vec2(0.0, s * 0.06) + vec2(a.cos(), a.sin()) * r * 0.8], st);
            p.circle_filled(c + vec2(0.0, s * 0.06), s * 0.07, col);
        }
        Icon::Key => {
            let r = Rect::from_center_size(c, vec2(s * 0.78, s * 0.52));
            p.rect_stroke(r, Rounding::same(s * 0.1), Stroke::new(s * 0.08, col));
            for (i, y) in [-0.1, 0.06].iter().enumerate() {
                let k = if i == 0 { 4 } else { 3 };
                for j in 0..k {
                    let x = c.x - s * 0.24 + j as f32 * s * 0.48 / (k - 1).max(1) as f32;
                    p.circle_filled(pos2(x, c.y + y * s), s * 0.035, col);
                }
            }
            p.line_segment([c + vec2(-0.14, 0.17) * s, c + vec2(0.14, 0.17) * s], Stroke::new(s * 0.06, col));
        }
        Icon::Gear => {
            for i in 0..8 {
                let a = i as f32 * PI / 4.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * s * 0.22, c + d * s * 0.4], Stroke::new(s * 0.14, col));
            }
            p.circle_stroke(c, s * 0.2, Stroke::new(s * 0.13, col));
        }
        Icon::Info => {
            p.circle_filled(c + vec2(0.0, -0.24) * s, s * 0.07, col);
            p.line_segment([c + vec2(0.0, -0.08) * s, c + vec2(0.0, 0.32) * s], Stroke::new(s * 0.13, col));
        }
        Icon::Sparkle => {
            let pts: Vec<Pos2> = (0..8).map(|i| {
                let a = i as f32 * PI / 4.0 - PI / 2.0;
                let r = if i % 2 == 0 { 0.42 } else { 0.12 };
                c + vec2(a.cos(), a.sin()) * r * s
            }).collect();
            for i in 0..8 {
                p.add(Shape::convex_polygon(vec![c, pts[i], pts[(i + 1) % 8]], col, Stroke::NONE));
            }
        }
        Icon::Next => {
            for dx in [-0.12, 0.12] {
                let o = c + vec2(dx * s, 0.0);
                p.add(Shape::line(vec![o + vec2(-0.1, -0.22) * s, o + vec2(0.1, 0.0) * s, o + vec2(-0.1, 0.22) * s], st));
            }
        }
        Icon::Cloud => {
            p.circle_filled(c + vec2(0.16, -0.16) * s, s * 0.17, alpha(col, 0.9));
            for i in 0..8 {
                let a = i as f32 * PI / 4.0;
                let d = vec2(a.cos(), a.sin());
                let o = c + vec2(0.16, -0.16) * s;
                p.line_segment([o + d * s * 0.22, o + d * s * 0.3], Stroke::new(s * 0.06, col));
            }
            p.circle_filled(c + vec2(-0.12, 0.08) * s, s * 0.17, col);
            p.circle_filled(c + vec2(0.08, 0.04) * s, s * 0.2, col);
            p.rect_filled(Rect::from_min_max(c + vec2(-0.3, 0.08) * s, c + vec2(0.3, 0.25) * s), Rounding::same(s * 0.08), col);
        }
        Icon::Drop => {
            let mut pts = Vec::new();
            for i in 0..=20 {
                let t = i as f32 / 20.0 * 2.0 * PI;
                let r = 0.26 * s;
                let x = t.sin() * r * (1.0 - 0.0);
                let y = -t.cos() * r;
                let pinch = if y < 0.0 { 1.0 + (-y / r) * 0.9 } else { 1.0 };
                pts.push(c + vec2(x / pinch, y * 1.25 + 0.06 * s));
            }
            p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
        }
    }
}

// ---------------------------------------------------------------- превью погоды (рисуется в egui)

fn hash(i: u32, k: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E3779B1) ^ k.wrapping_mul(0x85EBCA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A2D39);
    x ^= x >> 15;
    (x & 0xFFFFFF) as f32 / 16_777_216.0
}

pub fn mode_gradient(m: Mode) -> (Color32, Color32) {
    match m {
        Mode::Snow => (rgb(20, 40, 92), rgb(66, 110, 180)),
        Mode::Sakura => (rgb(70, 40, 92), rgb(214, 120, 160)),
        Mode::Leaves => (rgb(70, 36, 20), rgb(196, 112, 44)),
        Mode::Rain => (rgb(24, 32, 46), rgb(70, 90, 120)),
        Mode::Fireflies => (rgb(6, 18, 30), rgb(22, 60, 52)),
        Mode::Confetti => (rgb(60, 20, 90), rgb(40, 90, 200)),
        Mode::Bubbles => (rgb(10, 70, 100), rgb(60, 170, 190)),
        Mode::Butterflies => (rgb(40, 110, 170), rgb(120, 190, 90)),
        Mode::Dandelions => (rgb(70, 130, 190), rgb(150, 200, 120)),
    }
}

fn tint_col(t: Tint, c: [f32; 3], seed: f32, time: f32) -> [f32; 3] {
    match t {
        Tint::Natural => c,
        Tint::Rainbow => crate::render::sim::hsv((seed + time * 0.04).fract(), 0.55, 1.0),
        _ => {
            let tc = t.rgb();
            [c[0] * 0.2 + tc[0] * 0.8, c[1] * 0.2 + tc[1] * 0.8, c[2] * 0.2 + tc[2] * 0.8]
        }
    }
}

fn col3(c: [f32; 3], a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Мини-погода в прямоугольнике: учитывает ветер, скорость, плотность, размер и оттенок из настроек.
pub fn preview(p: &egui::Painter, rect: Rect, m: Mode, s: &Settings, time: f32, a: f32, scale: f32) {
    if a <= 0.01 {
        return;
    }
    let p = p.with_clip_rect(rect.intersect(p.clip_rect()));
    let (w, h) = (rect.width(), rect.height());
    let area = (w * h) / (400.0 * 170.0);
    let base = match m {
        Mode::Snow => 70.0,
        Mode::Sakura => 18.0,
        Mode::Leaves => 12.0,
        Mode::Rain => 70.0,
        Mode::Fireflies => 14.0,
        Mode::Confetti => 34.0,
        Mode::Bubbles => 9.0,
        Mode::Butterflies => 6.0,
        Mode::Dandelions => 16.0,
    };
    let n = ((base * area * s.density.clamp(0.2, 3.0)).ceil() as u32).clamp(3, 260);
    let ang = s.wind_angle.to_radians();
    let wind = vec2(ang.sin(), -ang.cos()) * s.wind * 70.0;
    let t = time * s.speed;
    let sz = s.size * scale;
    for i in 0..n {
        let (h1, h2, h3, h4) = (hash(i, 1), hash(i, 2), hash(i, 3), hash(i, 4));
        let depth = 0.45 + 0.55 * h3;
        let (fall, wk) = match m {
            Mode::Snow => (22.0 + 26.0 * h4, 1.0),
            Mode::Sakura => (16.0 + 14.0 * h4, 1.15),
            Mode::Leaves => (20.0 + 18.0 * h4, 1.25),
            Mode::Rain => (210.0 + 90.0 * h4, 1.1),
            Mode::Fireflies => (0.0, 0.1),
            Mode::Confetti => (26.0 + 22.0 * h4, 0.85),
            Mode::Bubbles => (-(14.0 + 14.0 * h4), 0.6),
            Mode::Butterflies => (0.0, 0.2),
            Mode::Dandelions => (4.0 + 6.0 * h4, 1.7),
        };
        let v = vec2(wind.x * wk, fall + wind.y * wk) * depth * scale;
        let sway = (t * (0.6 + h2) + h1 * 20.0).sin() * 8.0 * s.turbulence * scale;
        let m_ = 20.0 * scale;
        let (ww, hh) = (w + 2.0 * m_, h + 2.0 * m_);
        let mut x = (h1 * ww + v.x * t).rem_euclid(ww) - m_;
        let mut y = (h2 * hh + v.y * t).rem_euclid(hh) - m_;
        x += sway;
        if m == Mode::Butterflies {
            x = h1 * w + (t * (0.5 + h2 * 0.5) + h3 * 9.0).sin() * 40.0 * scale;
            y = h2 * h * 0.85 + (t * (0.7 + h4 * 0.4) + h1 * 7.0).cos() * 16.0 * scale + (t * 6.0 + h1 * 9.0).sin() * 3.0;
        }
        if m == Mode::Fireflies {
            x = h1 * w + (t * (0.3 + h2 * 0.4) + h3 * 9.0).sin() * 24.0 * scale;
            y = h2 * h * 0.9 + (t * (0.25 + h4 * 0.3) + h1 * 7.0).cos() * 14.0 * scale;
        }
        let pos = pos2(rect.left() + x, rect.top() + y);
        let al = a * (0.55 + 0.45 * depth);
        let rot = t * (h4 - 0.5) * 3.0 + h1 * 6.0;
        match m {
            Mode::Snow => {
                let c = tint_col(s.tint, [0.95, 0.97, 1.0], h1, time);
                p.circle_filled(pos, (0.9 + 1.6 * h3) * sz, col3(c, al * 0.95));
            }
            Mode::Sakura => {
                let c = tint_col(s.tint, [1.0, 0.68 + 0.15 * h3, 0.8], h1, time);
                let flip = (t * (2.0 + h2 * 2.0) + h3 * 6.0).cos();
                let pts: Vec<Pos2> = (0..10).map(|k| {
                    let q = k as f32 / 10.0 * 2.0 * PI;
                    let lx = q.cos() * 3.4 * sz * flip.abs().max(0.15);
                    let ly = q.sin() * 4.4 * sz;
                    pos + vec2(lx * rot.cos() - ly * rot.sin(), lx * rot.sin() + ly * rot.cos())
                }).collect();
                p.add(Shape::convex_polygon(pts, col3(c, al), Stroke::NONE));
            }
            Mode::Leaves => {
                const PAL: [[f32; 3]; 4] = [[0.95, 0.52, 0.12], [0.86, 0.24, 0.10], [0.98, 0.76, 0.22], [0.62, 0.34, 0.12]];
                let c = tint_col(s.tint, PAL[(h4 * 4.0) as usize % 4], h1, time);
                let flip = (t * (1.5 + h2) + h3 * 6.0).cos().abs().max(0.2);
                let pts: Vec<Pos2> = [(0.0, -6.0), (3.4 * flip, -1.0), (0.0, 5.5), (-3.4 * flip, -1.0)]
                    .iter()
                    .map(|(lx, ly)| {
                        let (lx, ly) = (lx * sz, ly * sz);
                        pos + vec2(lx * rot.cos() - ly * rot.sin(), lx * rot.sin() + ly * rot.cos())
                    })
                    .collect();
                p.add(Shape::convex_polygon(pts, col3(c, al), Stroke::NONE));
            }
            Mode::Rain => {
                let c = tint_col(s.tint, [0.75, 0.85, 1.0], h1, time);
                let dir = v.normalized();
                p.line_segment([pos, pos - dir * (8.0 + 6.0 * h3) * sz.sqrt()], Stroke::new(1.0_f32, col3(c, al * 0.6)));
            }
            Mode::Fireflies => {
                let pulse = 0.2 + 0.8 * ((t * (1.0 + h2) * 2.0 + h1 * 9.0).sin() * 0.5 + 0.5).powi(2);
                let c = tint_col(s.tint, [0.8, 1.0, 0.35], h1, time);
                p.circle_filled(pos, 7.0 * sz, col3(c, a * 0.12 * pulse));
                p.circle_filled(pos, 3.2 * sz, col3(c, a * 0.25 * pulse));
                p.circle_filled(pos, 1.4 * sz, col3([1.0, 1.0, 0.85], a * pulse));
            }
            Mode::Confetti => {
                const PAL: [[f32; 3]; 6] = [[1.0, 0.25, 0.35], [1.0, 0.75, 0.1], [0.2, 0.8, 0.4], [0.2, 0.55, 1.0], [0.75, 0.35, 1.0], [0.2, 0.9, 0.9]];
                let c = tint_col(s.tint, PAL[(h4 * 6.0) as usize % 6], h1, time);
                let flip = (t * (5.0 + h2 * 4.0)).cos();
                let (hx, hy) = (2.0 * sz * flip, 3.2 * sz);
                let rr = rot * 2.0;
                let pts: Vec<Pos2> = [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)]
                    .iter()
                    .map(|(lx, ly)| pos + vec2(lx * rr.cos() - ly * rr.sin(), lx * rr.sin() + ly * rr.cos()))
                    .collect();
                let pts = if flip < 0.0 { pts.into_iter().rev().collect() } else { pts };
                p.add(Shape::convex_polygon(pts, col3(c, al), Stroke::NONE));
            }
            Mode::Butterflies => {
                const PAL: [[f32; 3]; 5] = [[1.0, 0.55, 0.1], [0.3, 0.6, 1.0], [1.0, 0.88, 0.25], [0.97, 0.97, 0.95], [1.0, 0.45, 0.7]];
                let c = tint_col(s.tint, PAL[(h4 * 5.0) as usize % 5], h1, time);
                let flap = (t * (12.0 + h2 * 4.0) + h1 * 9.0).sin().abs().max(0.15);
                let wsz = 4.5 * sz;
                for side in [-1.0f32, 1.0] {
                    let up = pos + vec2(side * wsz * 0.55 * flap, -wsz * 0.3);
                    let lo = pos + vec2(side * wsz * 0.42 * flap, wsz * 0.35);
                    p.add(Shape::convex_polygon(ellipse(up, wsz * 0.55 * flap, wsz * 0.5), col3(c, al), Stroke::NONE));
                    p.add(Shape::convex_polygon(ellipse(lo, wsz * 0.4 * flap, wsz * 0.35), col3(c, al * 0.9), Stroke::NONE));
                }
                p.line_segment([pos + vec2(0.0, -wsz * 0.55), pos + vec2(0.0, wsz * 0.6)], Stroke::new(1.3_f32, col3([0.15, 0.12, 0.1], al)));
            }
            Mode::Dandelions => {
                let c = tint_col(s.tint, [1.0, 1.0, 0.95], h1, time);
                let r = (3.0 + 1.5 * h3) * sz;
                let head = pos + vec2(0.0, -r * 0.3);
                for k in 0..9 {
                    let a = -PI / 2.0 + (k as f32 - 4.0) * 0.32 + rot.sin() * 0.15;
                    p.line_segment([head, head + vec2(a.cos(), a.sin()) * r], Stroke::new(0.7_f32, col3(c, al * 0.8)));
                }
                p.line_segment([head, pos + vec2(0.0, r * 0.9)], Stroke::new(0.8_f32, col3(c, al * 0.8)));
                p.circle_filled(pos + vec2(0.0, r * 0.95), 0.9 * sz, col3([0.6, 0.5, 0.35], al));
            }
            Mode::Bubbles => {
                let r = (5.0 + 8.0 * h3) * sz;
                let c = tint_col(s.tint, [0.85, 0.95, 1.0], h1, time);
                p.circle_filled(pos, r, col3(c, a * 0.06));
                p.circle_stroke(pos, r, Stroke::new(1.2_f32, col3(c, al * 0.7)));
                p.circle_filled(pos + vec2(-0.4, -0.4) * r, r * 0.18, col3([1.0, 1.0, 1.0], al * 0.8));
            }
        }
    }
}

fn ellipse(c: Pos2, rx: f32, ry: f32) -> Vec<Pos2> {
    (0..12).map(|k| {
        let q = k as f32 / 12.0 * 2.0 * PI;
        c + vec2(q.cos() * rx, q.sin() * ry)
    }).collect()
}

pub fn tint_swatch(t: Tint) -> Color32 {
    match t {
        Tint::Natural => rgb(240, 244, 255),
        Tint::Ice => rgb(140, 210, 255),
        Tint::Gold => rgb(255, 204, 90),
        Tint::Pink => rgb(255, 140, 196),
        Tint::Mint => rgb(120, 240, 200),
        Tint::Rainbow => rgb(255, 255, 255),
    }
}

/// Круг-«радуга» (конический градиент веером).
pub fn rainbow_circle(p: &egui::Painter, c: Pos2, r: f32) {
    let mut mesh = epaint::Mesh::default();
    mesh.colored_vertex(c, Color32::WHITE);
    let n = 36;
    for i in 0..=n {
        let a = i as f32 / n as f32;
        let h = crate::render::sim::hsv(a, 0.75, 1.0);
        let ang = a * 2.0 * PI - PI / 2.0;
        mesh.colored_vertex(c + vec2(ang.cos(), ang.sin()) * r, col3(h, 1.0));
    }
    for i in 1..=n as u32 {
        mesh.add_triangle(0, i, i + 1);
    }
    p.add(Shape::mesh(mesh));
}

pub fn vec_from_deg(deg: f32) -> Vec2 {
    let a = deg.to_radians();
    vec2(a.sin(), -a.cos())
}
