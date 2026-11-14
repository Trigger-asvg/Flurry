//! Симуляция частиц одного монитора. Все скорости задаются для масштаба 100% и умножаются на DPI-масштаб монитора.

use super::gpu::Inst;
use crate::settings::{Mode, MouseMode, Settings, Tint};
use std::f32::consts::PI;

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }
    #[inline]
    pub fn u(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
    #[inline]
    pub fn f(&mut self) -> f32 {
        (self.u() >> 8) as f32 / 16_777_216.0
    }
    #[inline]
    pub fn r(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
}

/// Общие для всех мониторов параметры кадра.
pub struct Env<'a> {
    pub s: &'a Settings,
    pub dt: f32,
    pub time: f32,
    /// ветер, px/с при 100%
    pub wind: (f32, f32),
    /// усиление «бури» 0..1
    pub storm: f32,
    pub flash: f32,
    /// курсор в координатах монитора + скорость (px/с)
    pub mouse: Option<(f32, f32, f32, f32)>,
}

#[derive(Clone, Copy, Default)]
struct P {
    x: f32,
    y: f32,
    ex: f32,
    ey: f32,
    fall: f32,
    size: f32,
    depth: f32,
    rot: f32,
    vrot: f32,
    flip: f32,
    vflip: f32,
    phase: f32,
    freq: f32,
    amp: f32,
    age: f32,
    life: f32,
    seed: f32,
    head: f32,
    kind: u8,
    col: [f32; 3],
    alpha: f32,
    bokeh: bool,
}

struct Layer {
    mode: Mode,
    weight: f32,
    target: f32,
    parts: Vec<P>,
    filled: bool,
}

pub struct Field {
    pub w: f32,
    pub h: f32,
    /// низ рабочей области (над панелью задач) — сюда падают капли
    pub ground: f32,
    pub k: f32,
    layers: Vec<Layer>,
    fx: Vec<P>,
    rng: Rng,
    last_mouse: Option<(f32, f32)>,
}

const FX_SPARK: u8 = 8;
const FX_RING: u8 = 11;

fn base_count(m: Mode) -> f32 {
    match m {
        Mode::Snow => 300.0,
        Mode::Sakura => 70.0,
        Mode::Leaves => 42.0,
        Mode::Rain => 480.0,
        Mode::Fireflies => 45.0,
        Mode::Confetti => 140.0,
        Mode::Bubbles => 26.0,
        Mode::Butterflies => 14.0,
        Mode::Dandelions => 48.0,
    }
}

fn wind_k(m: Mode) -> f32 {
    match m {
        Mode::Snow => 1.0,
        Mode::Sakura => 1.15,
        Mode::Leaves => 1.25,
        Mode::Rain => 1.1,
        Mode::Fireflies => 0.12,
        Mode::Confetti => 0.85,
        Mode::Bubbles => 0.6,
        Mode::Butterflies => 0.25,
        Mode::Dandelions => 1.7,
    }
}

/// Направление «падения»: +1 вниз, −1 вверх (пузыри), 0 — светлячки
fn gravity(m: Mode) -> f32 {
    match m {
        Mode::Bubbles => -1.0,
        Mode::Fireflies | Mode::Butterflies => 0.0,
        _ => 1.0,
    }
}

fn mean_fall(m: Mode) -> f32 {
    match m {
        Mode::Snow => 75.0,
        Mode::Sakura => 55.0,
        Mode::Leaves => 75.0,
        Mode::Rain => 950.0,
        Mode::Fireflies => 0.0,
        Mode::Confetti => 105.0,
        Mode::Bubbles => 50.0,
        Mode::Butterflies => 0.0,
        Mode::Dandelions => 14.0,
    }
}

/// Режимы, где частицы блуждают сами (без «падения»).
fn wanders(m: Mode) -> bool {
    matches!(m, Mode::Fireflies | Mode::Butterflies)
}

impl Field {
    pub fn new(w: f32, h: f32, ground: f32, k: f32, seed: u64) -> Field {
        Field { w, h, ground, k, layers: Vec::new(), fx: Vec::new(), rng: Rng::new(seed), last_mouse: None }
    }

    pub fn clear(&mut self) {
        for l in &mut self.layers {
            l.parts.clear();
            l.filled = true; // появятся заново от краёв, а не сразу по всему экрану
        }
        self.fx.clear();
    }

    /// Идёт смена режима или живут короткие эффекты (искры, брызги) — нужна полная плавность.
    pub fn busy(&self) -> bool {
        self.layers.len() > 1 || !self.fx.is_empty()
    }

    fn target_count(&self, m: Mode, e: &Env) -> usize {
        let area = (self.w * self.h) / (1920.0 * 1080.0 * self.k * self.k).max(1.0);
        let storm = 1.0 + e.storm * 1.5;
        (base_count(m) * e.s.density * area.clamp(0.2, 6.0) * storm).round() as usize
    }

    fn sync_layers(&mut self, e: &Env) {
        let mode = e.s.mode;
        if !self.layers.iter().any(|l| l.mode == mode && l.target > 0.0) {
            let instant = self.layers.is_empty() || e.s.crossfade <= 0.01;
            self.layers.push(Layer { mode, weight: if instant { 1.0 } else { 0.0 }, target: 1.0, parts: Vec::new(), filled: false });
        }
        let fade = if e.s.crossfade <= 0.01 { 1e9 } else { e.dt / e.s.crossfade };
        for l in &mut self.layers {
            l.target = if l.mode == mode { 1.0 } else { 0.0 };
            if l.weight < l.target {
                l.weight = (l.weight + fade).min(1.0);
            } else if l.weight > l.target {
                l.weight = (l.weight - fade).max(0.0);
            }
        }
        self.layers.retain(|l| l.target > 0.0 || (l.weight > 0.0 && !l.parts.is_empty()));
    }

    fn spawn(&mut self, mode: Mode, e: &Env, anywhere: bool) -> P {
        let k = self.k;
        let r = &mut self.rng;
        let depth = if e.s.depth3d { r.r(0.35, 1.0) } else { 0.8 };
        let mut p = P {
            depth,
            rot: r.r(0.0, 2.0 * PI),
            vrot: r.r(-1.0, 1.0),
            flip: r.r(0.0, 2.0 * PI),
            vflip: r.r(1.5, 3.5) * if r.f() < 0.5 { -1.0 } else { 1.0 },
            phase: r.r(0.0, 2.0 * PI),
            freq: r.r(0.3, 0.9),
            amp: r.r(14.0, 34.0),
            life: f32::INFINITY,
            seed: r.f(),
            head: r.r(0.0, 2.0 * PI),
            alpha: 1.0,
            col: [1.0, 1.0, 1.0],
            ..Default::default()
        };
        match mode {
            Mode::Snow => {
                p.fall = r.r(45.0, 105.0);
                p.size = r.r(1.9, 4.8);
                p.alpha = r.r(0.6, 0.95);
                p.col = [0.93, 0.96, 1.0];
                p.kind = if p.size > 3.4 && r.f() < 0.45 { 1 } else { 0 };
                if e.s.bokeh && r.f() < 0.025 {
                    p.bokeh = true;
                    p.depth = 1.5;
                    p.size = r.r(14.0, 26.0);
                    p.alpha = r.r(0.10, 0.18);
                    p.kind = 0;
                }
            }
            Mode::Sakura => {
                p.fall = r.r(32.0, 70.0);
                p.size = r.r(8.0, 12.5);
                p.amp = r.r(30.0, 70.0);
                p.vrot = r.r(-1.4, 1.4);
                let t = r.f();
                p.col = [1.0, 0.62 + 0.2 * t, 0.74 + 0.14 * t];
                p.alpha = r.r(0.85, 1.0);
                p.kind = 2;
            }
            Mode::Leaves => {
                p.fall = r.r(48.0, 100.0);
                p.size = r.r(13.0, 20.0);
                p.amp = r.r(40.0, 90.0);
                p.freq = r.r(0.25, 0.6);
                p.vrot = r.r(-1.8, 1.8);
                const PAL: [[f32; 3]; 5] = [[0.95, 0.52, 0.12], [0.86, 0.24, 0.10], [0.98, 0.76, 0.22], [0.62, 0.34, 0.12], [0.80, 0.60, 0.15]];
                p.col = PAL[(r.u() % 5) as usize];
                p.kind = 3;
            }
            Mode::Rain => {
                p.fall = r.r(780.0, 1150.0);
                p.size = r.r(11.0, 22.0); // половина длины
                p.alpha = r.r(0.22, 0.45);
                p.col = [0.78, 0.86, 1.0];
                p.kind = 4;
                p.amp = 0.0;
            }
            Mode::Fireflies => {
                p.fall = r.r(14.0, 32.0);
                p.size = r.r(11.0, 17.0);
                p.col = [0.78, 1.0, 0.32];
                p.freq = r.r(0.5, 1.4);
                p.kind = 5;
            }
            Mode::Confetti => {
                p.fall = r.r(70.0, 140.0);
                p.size = r.r(4.8, 7.5);
                p.vflip = r.r(4.0, 9.0);
                p.vrot = r.r(-3.0, 3.0);
                const PAL: [[f32; 3]; 7] = [[1.0, 0.25, 0.35], [1.0, 0.75, 0.1], [0.2, 0.8, 0.4], [0.2, 0.55, 1.0], [0.75, 0.35, 1.0], [1.0, 0.45, 0.8], [0.2, 0.9, 0.9]];
                p.col = PAL[(r.u() % 7) as usize];
                p.kind = 6;
            }
            Mode::Butterflies => {
                p.fall = r.r(45.0, 85.0);
                p.size = r.r(18.0, 27.0);
                p.freq = r.r(0.6, 1.6);
                p.vflip = r.r(11.0, 16.0);
                const PAL: [[f32; 3]; 6] = [[1.0, 0.55, 0.1], [0.3, 0.6, 1.0], [1.0, 0.88, 0.25], [0.97, 0.97, 0.95], [1.0, 0.45, 0.7], [0.55, 0.9, 0.5]];
                p.col = PAL[(r.u() % 6) as usize];
                p.kind = 12;
            }
            Mode::Dandelions => {
                p.fall = r.r(-4.0, 22.0);
                p.size = r.r(10.0, 15.0);
                p.amp = r.r(25.0, 60.0);
                p.freq = r.r(0.2, 0.5);
                p.vrot = 0.0;
                p.rot = r.r(-0.35, 0.35);
                p.alpha = r.r(0.75, 0.95);
                p.col = [1.0, 0.99, 0.94];
                p.kind = 13;
            }
            Mode::Bubbles => {
                p.fall = r.r(30.0, 70.0);
                p.size = r.r(10.0, 28.0);
                p.amp = r.r(10.0, 25.0);
                p.alpha = r.r(0.65, 0.95);
                p.kind = 7;
            }
        }
        let (w, h) = (self.w, self.h);
        let m = (p.size * 3.0 + 30.0) * k;
        if anywhere || wanders(mode) {
            p.x = self.rng.r(0.0, w);
            p.y = self.rng.r(0.0, h);
            if !anywhere {
                p.age = 0.0;
            }
            if anywhere {
                p.age = -self.rng.r(0.0, 1.2); // плавное появление при первом заполнении
            }
            return p;
        }
        // с какого края входить — по среднему направлению движения слоя
        let wk = wind_k(mode);
        let vx = e.wind.0 * wk;
        let vy = gravity(mode) * mean_fall(mode) + e.wind.1 * wk;
        let wt = vy.max(0.0) * w;
        let wb = (-vy).max(0.0) * w;
        let wl = vx.max(0.0) * h;
        let wr = (-vx).max(0.0) * h;
        let sum = wt + wb + wl + wr;
        let pick = self.rng.f() * sum.max(1e-3);
        if pick < wt {
            p.x = self.rng.r(-m, w + m);
            p.y = -m * 0.9;
        } else if pick < wt + wb {
            p.x = self.rng.r(-m, w + m);
            p.y = h + m * 0.9;
        } else if pick < wt + wb + wl {
            p.x = -m * 0.9;
            p.y = self.rng.r(-m, h + m);
        } else {
            p.x = w + m * 0.9;
            p.y = self.rng.r(-m, h + m);
        }
        p.age = 1.0;
        p
    }

    pub fn update(&mut self, e: &Env) {
        self.sync_layers(e);
        let dt = e.dt;
        let k = self.k;
        let s = e.s;
        let sp = s.speed * (1.0 + e.storm * 0.6);

        // мышь: отталкивание / притяжение + «сдувание» движением курсора
        let mouse = e.mouse.filter(|_| s.mouse != MouseMode::Off);
        let rad = 130.0 * k * s.mouse_radius;
        let str_ = s.mouse_strength;

        for li in 0..self.layers.len() {
            let mode = self.layers[li].mode;
            let spawning = self.layers[li].target > 0.0;
            let target = if spawning { self.target_count(mode, e) } else { 0 };
            if !self.layers[li].filled {
                self.layers[li].filled = true;
                let n = target;
                for _ in 0..n {
                    let p = self.spawn(mode, e, true);
                    self.layers[li].parts.push(p);
                }
            }
            // добираем недостающие (от краёв)
            let have = self.layers[li].parts.len();
            if spawning && have < target {
                let add = ((target - have) as f32 * (dt * 1.5).min(1.0)).ceil() as usize;
                for _ in 0..add.min(target - have) {
                    let p = self.spawn(mode, e, false);
                    self.layers[li].parts.push(p);
                }
            }
            let wk = wind_k(mode);
            let g = gravity(mode);
            let (w, h, ground) = (self.w, self.h, self.ground);
            let mut parts = std::mem::take(&mut self.layers[li].parts);
            let mut i = 0;
            let mut over = parts.len().saturating_sub(target);
            while i < parts.len() {
                let p = &mut parts[i];
                p.age += dt;
                p.rot += p.vrot * dt;
                p.flip += p.vflip * dt;
                let d = p.depth;
                let (mut vx, mut vy);
                if wanders(mode) {
                    // блуждание
                    p.head += (((e.time * p.freq + p.phase).sin() * 1.3) + (e.time * 0.37 + p.seed * 9.0).sin()) * dt;
                    vx = p.head.cos() * p.fall + e.wind.0 * wk;
                    vy = p.head.sin() * p.fall * 0.7 + e.wind.1 * wk;
                    if mode == Mode::Butterflies {
                        // порхание: подпрыгивания в такт взмахам
                        vy += (e.time * 5.0 + p.phase).sin() * 35.0;
                        p.head += (e.time * 0.9 + p.seed * 7.0).sin() * dt * 1.5;
                    }
                    // мягкие стенки
                    let mx = 40.0 * k;
                    if p.x < mx { p.head *= 0.97; vx += 20.0; }
                    if p.x > w - mx { vx -= 20.0; }
                    if p.y < mx { vy += 20.0; }
                    if p.y > h - mx { vy -= 20.0; }
                } else {
                    let sway = (e.time * p.freq + p.phase).cos() * p.amp * p.freq * s.turbulence;
                    vx = e.wind.0 * wk * d + sway;
                    vy = g * p.fall * d + e.wind.1 * wk * d;
                    if mode == Mode::Bubbles {
                        vx += (e.time * 1.7 + p.phase).sin() * 6.0;
                    }
                }
                // внешний импульс (мышь)
                if let Some((mx, my, mvx, mvy)) = mouse {
                    let dx = p.x - mx;
                    let dy = p.y - my;
                    let d2 = dx * dx + dy * dy;
                    if d2 < rad * rad {
                        let dist = d2.sqrt().max(1.0);
                        let f = 1.0 - dist / rad;
                        let (nx, ny) = (dx / dist, dy / dist);
                        match s.mouse {
                            MouseMode::Repel => {
                                let push = if mode == Mode::Rain { 5200.0 } else { 2400.0 };
                                p.ex += nx * f * f * push * str_ * k * dt;
                                p.ey += ny * f * f * push * str_ * k * dt;
                            }
                            MouseMode::Attract => {
                                let pull = 900.0 * str_ * k * dt * f;
                                p.ex += (-nx * 0.7 - ny * 0.9) * pull;
                                p.ey += (-ny * 0.7 + nx * 0.9) * pull;
                            }
                            MouseMode::Off => {}
                        }
                        p.ex += mvx * f * f * 2.5 * dt * str_;
                        p.ey += mvy * f * f * 2.5 * dt * str_;
                        if mode == Mode::Bubbles && s.pop_bubbles && dist < p.size * k * s.size + 6.0 * k {
                            p.life = 0.0; // лопнуть
                        }
                    }
                }
                let damp = (-2.4 * dt).exp();
                p.ex *= damp;
                p.ey *= damp;
                let lim = 1600.0 * k;
                p.ex = p.ex.clamp(-lim, lim);
                p.ey = p.ey.clamp(-lim, lim);

                p.x += (vx * sp * k + p.ex) * dt;
                p.y += (vy * sp * k + p.ey) * dt;
                if mode == Mode::Rain {
                    // поворачиваем каплю по скорости
                    p.rot = (-(vx * sp * k + p.ex)).atan2(vy * sp * k + p.ey);
                }

                let m = (p.size * 3.0 + 30.0) * k * s.size.max(1.0);
                let mut dead = p.x < -m || p.x > w + m || p.y < -m || p.y > h + m || p.life <= 0.0;
                let mut splash = None;
                let mut pop = None;
                if mode == Mode::Rain && p.y > ground && p.ey.abs() < 300.0 * k {
                    dead = true;
                    if s.splashes {
                        splash = Some((p.x, ground));
                    }
                }
                if mode == Mode::Bubbles && p.life <= 0.0 {
                    pop = Some((p.x, p.y, p.size));
                }
                if mode == Mode::Bubbles && p.y < -m * 0.5 + 10.0 && p.age > 2.0 && self.rng.f() < dt * 0.6 {
                    // лопаются у верхнего края
                    pop = Some((p.x, p.y, p.size));
                    dead = true;
                }
                if let Some((x, y)) = splash {
                    if self.rng.f() < 0.35 {
                        self.fx.push(P { x, y, kind: FX_RING, life: 0.28, size: self.rng.r(3.0, 7.0), alpha: 0.5, col: [0.8, 0.88, 1.0], ..Default::default() });
                    }
                }
                if let Some((x, y, sz)) = pop {
                    self.fx.push(P { x, y, kind: FX_RING, life: 0.25, size: sz * 1.1, alpha: 0.8, col: [0.9, 0.97, 1.0], ..Default::default() });
                    for _ in 0..5 {
                        let a = self.rng.r(0.0, 2.0 * PI);
                        let v = self.rng.r(60.0, 160.0) * k;
                        self.fx.push(P { x, y, ex: a.cos() * v, ey: a.sin() * v, kind: FX_SPARK, life: 0.35, size: 3.0, alpha: 0.8, col: [0.85, 0.95, 1.0], ..Default::default() });
                    }
                    dead = true;
                }
                if dead {
                    if spawning && over == 0 {
                        parts[i] = self.spawn(mode, e, false);
                        i += 1;
                    } else {
                        over = over.saturating_sub(1);
                        parts.swap_remove(i);
                    }
                } else {
                    i += 1;
                }
            }
            self.layers[li].parts = parts;
        }

        // шлейф за курсором
        if let Some((mx, my, mvx, mvy)) = e.mouse {
            if s.cursor_trail {
                let v = (mvx * mvx + mvy * mvy).sqrt();
                if let Some((lx, ly)) = self.last_mouse {
                    let dist = ((mx - lx).powi(2) + (my - ly).powi(2)).sqrt();
                    if v > 250.0 * k && dist < 400.0 * k {
                        let n = ((dist / (9.0 * k)) as usize).min(14);
                        let col = trail_color(s.mode, &mut self.rng);
                        for j in 0..n {
                            let t = j as f32 / n.max(1) as f32;
                            self.fx.push(P {
                                x: lx + (mx - lx) * t + self.rng.r(-4.0, 4.0) * k,
                                y: ly + (my - ly) * t + self.rng.r(-4.0, 4.0) * k,
                                ex: -mvx * 0.05 + self.rng.r(-30.0, 30.0) * k,
                                ey: -mvy * 0.05 + self.rng.r(10.0, 60.0) * k,
                                kind: FX_SPARK,
                                life: self.rng.r(0.35, 0.7),
                                size: self.rng.r(2.5, 5.0),
                                alpha: 0.9,
                                rot: self.rng.r(0.0, PI),
                                col,
                                ..Default::default()
                            });
                        }
                    }
                }
                self.last_mouse = Some((mx, my));
            }
        } else {
            self.last_mouse = None;
        }

        // эффекты с конечным временем жизни
        let mut i = 0;
        while i < self.fx.len() {
            let p = &mut self.fx[i];
            p.age += dt;
            p.x += p.ex * dt;
            p.y += p.ey * dt;
            p.ex *= (-3.0 * dt).exp();
            p.ey *= (-3.0 * dt).exp();
            if p.age >= p.life {
                self.fx.swap_remove(i);
            } else {
                i += 1;
            }
        }
        if self.fx.len() > 3000 {
            self.fx.drain(0..self.fx.len() - 3000);
        }
    }

    /// Фон низкого разрешения: туман и вспышки молний. `div` — во сколько раз слой меньше экрана.
    pub fn emit_bg(&self, e: &Env, div: f32, out: &mut Vec<Inst>) {
        let (w, h) = (self.w / div, self.h / div);
        let rain_w: f32 = self.layers.iter().filter(|l| l.mode == Mode::Rain).map(|l| l.weight).sum();
        if e.s.rain_fog && rain_w > 0.01 {
            out.push(Inst { a: [w * 0.5, h * 0.78, w * 0.5 + 1.0, h * 0.22 + 0.5], b: [0.0, 9.0, 0.0, 0.0], c: [0.78, 0.84, 0.95, 0.16 * rain_w.min(1.0)] });
        }
        if e.flash > 0.003 {
            out.push(Inst { a: [w * 0.5, h * 0.5, w * 0.5 + 1.0, h * 0.5 + 1.0], b: [0.0, 10.0, 0.0, 0.0], c: [0.85, 0.9, 1.0, e.flash] });
        }
    }

    pub fn emit(&self, e: &Env, out: &mut Vec<Inst>) {
        let s = e.s;
        let k = self.k;
        let sz = s.size;
        for l in &self.layers {
            let lw = l.weight * l.weight * (3.0 - 2.0 * l.weight);
            if lw <= 0.003 {
                continue;
            }
            for (idx, p) in l.parts.iter().enumerate() {
                let fade = if p.age < 0.0 { 0.0 } else { (p.age / 1.2).min(1.0) };
                let d = p.depth;
                let da = if s.depth3d && !p.bokeh { 0.45 + 0.55 * d } else { 1.0 };
                let a = p.alpha * lw * fade * da;
                if a <= 0.004 {
                    continue;
                }
                let ds = if p.bokeh { 1.0 } else if s.depth3d { 0.55 + 0.6 * d } else { 1.0 };
                let r = p.size * k * sz * ds;
                let col = tint(s.tint, p.col, p.seed, e.time, idx);
                let inst = match l.mode {
                    Mode::Snow => {
                        if p.kind == 1 {
                            Inst { a: [p.x, p.y, r * 1.9, r * 1.9], b: [p.rot, 1.0, 0.0, 0.0], c: [col[0], col[1], col[2], a] }
                        } else {
                            let hard = if p.bokeh { 0.15 } else { 0.35 };
                            Inst { a: [p.x, p.y, r.max(1.0), r.max(1.0)], b: [0.0, 0.0, hard, 0.0], c: [col[0], col[1], col[2], a] }
                        }
                    }
                    Mode::Sakura => {
                        let f = p.flip.cos();
                        Inst { a: [p.x, p.y, r * 0.8 * f, r], b: [p.rot, 2.0, f, 0.0], c: [col[0], col[1], col[2], a] }
                    }
                    Mode::Leaves => {
                        let f = p.flip.cos();
                        Inst { a: [p.x, p.y, r * 0.75 * f, r], b: [p.rot, 3.0, f, 0.0], c: [col[0], col[1], col[2], a] }
                    }
                    Mode::Rain => {
                        let wdt = (0.9 * k * sz.sqrt()).max(0.8);
                        Inst { a: [p.x, p.y, wdt, p.size * k * sz.sqrt() * (0.6 + 0.4 * d)], b: [p.rot, 4.0, 0.0, 0.0], c: [col[0], col[1], col[2], a] }
                    }
                    Mode::Fireflies => {
                        let pulse = 0.15 + 0.85 * ((e.time * p.freq * 2.2 + p.phase).sin() * 0.5 + 0.5).powf(2.0);
                        Inst { a: [p.x, p.y, r * 2.2, r * 2.2], b: [0.0, 5.0, 0.0, 0.0], c: [col[0], col[1], col[2], a * pulse] }
                    }
                    Mode::Confetti => {
                        let f = p.flip.cos();
                        Inst { a: [p.x, p.y, r * 0.65 * f, r], b: [p.rot, 6.0, f, 0.0], c: [col[0], col[1], col[2], a] }
                    }
                    Mode::Bubbles => Inst { a: [p.x, p.y, r, r], b: [0.0, 7.0, 0.0, p.seed], c: [col[0], col[1], col[2], a] },
                    Mode::Butterflies => {
                        // взмах: крылья «складываются» по ширине; наклон по направлению полёта
                        let f = p.flip.sin();
                        let w = 0.45 + 0.55 * f.abs();
                        let tilt = p.head.cos() * 0.35;
                        Inst { a: [p.x, p.y, r * w, r * 0.8], b: [tilt, 12.0, f, p.seed], c: [col[0], col[1], col[2], a] }
                    }
                    Mode::Dandelions => Inst { a: [p.x, p.y, r, r * 1.25], b: [p.rot + (e.time * 0.8 + p.phase).sin() * 0.25, 13.0, 0.0, 0.0], c: [col[0], col[1], col[2], a] },
                };
                out.push(inst);
            }
        }
        for p in &self.fx {
            let t = (p.age / p.life).clamp(0.0, 1.0);
            let (scale, a) = if p.kind == FX_RING { (0.4 + 0.9 * t, (1.0 - t) * p.alpha) } else { (1.0 - 0.5 * t, (1.0 - t).powf(1.5) * p.alpha) };
            let r = p.size * k * scale * if p.kind == FX_RING { 1.0 } else { sz.sqrt() };
            let (hw, hh) = if p.kind == FX_RING && p.size < 8.0 { (r * 1.8, r * 0.55) } else { (r, r) };
            out.push(Inst { a: [p.x, p.y, hw, hh], b: [p.rot, p.kind as f32, 0.0, 0.0], c: [p.col[0], p.col[1], p.col[2], a] });
        }
    }
}

fn trail_color(m: Mode, r: &mut Rng) -> [f32; 3] {
    match m {
        Mode::Snow | Mode::Rain | Mode::Bubbles | Mode::Dandelions => [0.85, 0.93, 1.0],
        Mode::Butterflies => [1.0, 0.85, 0.5],
        Mode::Sakura => [1.0, 0.7, 0.85],
        Mode::Leaves => [1.0, 0.75, 0.35],
        Mode::Fireflies => [0.85, 1.0, 0.45],
        Mode::Confetti => {
            let h = r.f();
            hsv(h, 0.7, 1.0)
        }
    }
}

pub fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

fn tint(t: Tint, c: [f32; 3], seed: f32, time: f32, _idx: usize) -> [f32; 3] {
    match t {
        Tint::Natural => c,
        Tint::Rainbow => {
            let h = hsv((seed + time * 0.04).fract(), 0.55, 1.0);
            let l = (c[0] + c[1] + c[2]) / 3.0;
            [h[0] * (0.4 + 0.7 * l), h[1] * (0.4 + 0.7 * l), h[2] * (0.4 + 0.7 * l)]
        }
        _ => {
            let tc = t.rgb();
            let l = (0.3 * c[0] + 0.55 * c[1] + 0.15 * c[2]).max(0.35) * 1.1;
            [c[0] * 0.2 + tc[0] * l * 0.8, c[1] * 0.2 + tc[1] * l * 0.8, c[2] * 0.2 + tc[2] * l * 0.8]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_in_range() {
        let mut r = Rng::new(42);
        for _ in 0..10_000 {
            let f = r.f();
            assert!((0.0..1.0).contains(&f));
            let x = r.r(-3.0, 5.0);
            assert!((-3.0..5.0).contains(&x));
        }
    }

    #[test]
    fn hsv_primaries() {
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4);
        assert!(near(hsv(0.0, 1.0, 1.0), [1.0, 0.0, 0.0]));
        assert!(near(hsv(1.0 / 3.0, 1.0, 1.0), [0.0, 1.0, 0.0]));
        assert!(near(hsv(2.0 / 3.0, 1.0, 1.0), [0.0, 0.0, 1.0]));
        assert!(near(hsv(0.5, 0.0, 0.7), [0.7, 0.7, 0.7]));
    }

    #[test]
    fn field_fills_and_clears() {
        let s = Settings::default();
        let env = Env { s: &s, dt: 1.0 / 60.0, time: 0.0, wind: (0.0, 0.0), storm: 0.0, flash: 0.0, mouse: None };
        let mut f = Field::new(1920.0, 1080.0, 1040.0, 1.0, 1);
        f.update(&env);
        let mut out = Vec::new();
        f.emit(&env, &mut out);
        let n = f.layers[0].parts.len();
        assert_eq!(n, base_count(Mode::Snow) as usize);
        // свежие частицы проявляются плавно: часть ещё невидима
        assert!(out.len() <= n);
        f.clear();
        assert!(f.layers[0].parts.is_empty());
    }
}
