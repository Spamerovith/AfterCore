//! Software renderer for the overlay (and the live preview in the launcher).
//!
//! Layouts:
//! * **Card**     - panel with sections (header + two value columns); `compact` puts each
//!                  section on one line.
//! * **Classic**  - no panel, coloured section labels, value columns aligned across rows,
//!                  outlined text readable on any scene.
//! * **Bar**      - one horizontal line.
//! * **Minimal**  - big FPS number.
//!
//! Graphs are always drawn at the bottom.

use std::collections::HashMap;
use std::sync::Arc;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, VariableFont};
use ac_core::config::{OverlayLayout, OverlayStyle, Rgba};
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Path, PathBuilder, Pixmap, Point,
    SpreadMode, Stroke, Transform,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub value: String,
    pub unit: String,
    pub color: Option<Rgba>,
}

impl Part {
    pub fn new(value: impl Into<String>, unit: impl Into<String>) -> Self {
        Self { value: value.into(), unit: unit.into(), color: None }
    }
    pub fn colored(value: impl Into<String>, unit: impl Into<String>, c: Rgba) -> Self {
        Self { value: value.into(), unit: unit.into(), color: Some(c) }
    }
}

/// One resource in the bottleneck strip.
#[derive(Clone, Debug, PartialEq)]
pub struct Pill {
    /// "GPU 94%", "CAP 144".
    pub text: String,
    /// Small suffix shown on the limiter: "bound · power limit".
    pub sub: String,
    pub color: Rgba,
    /// This resource is the current limiter (solid pill).
    pub bound: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    /// Game name and graphics API.
    Title { name: String, tag: String },
    /// Presented FPS; with frame generation `real` holds the game's own FPS and `unit` the
    /// technology ("DLSS", "FSR", …), otherwise `unit` is "FPS".
    Fps { value: String, color: Rgba, real: Option<String>, unit: String },
    /// A group of values. An empty title means "frame statistics" (shown under the FPS).
    Section { title: String, color: Rgba, parts: Vec<Part> },
    /// `max <= 0` = frametime mode (auto scale with a sane floor), otherwise fixed 0..max.
    Graph { title: String, value: String, values: Vec<f32>, max: f32, color: Rgba },
    Badge { text: String, color: Rgba },
    /// Small coloured strip at the very bottom: GPU / CPU / VRAM / RAM.
    Bottleneck { pills: Vec<Pill> },
}

pub struct Scene<'a> {
    pub cells: &'a [Cell],
    pub style: &'a OverlayStyle,
    pub layout: OverlayLayout,
    pub scale: f32,
    pub graph_height: f32,
    pub compact: bool,
}

struct FontSet {
    regular: FontVec,
    bold: FontVec,
}

fn load_font(path: &str, weight: f32) -> Option<FontVec> {
    let data = std::fs::read(path).ok()?;
    let mut f = FontVec::try_from_vec_and_index(data, 0).ok()?;
    // Variable fonts (Segoe UI Variable, Bahnschrift, Inter…): pick weight; condensed width if available.
    f.set_variation(b"wght", weight);
    f.set_variation(b"wdth", 85.0);
    Some(f)
}

impl FontSet {
    fn load(path: &str) -> Option<Self> {
        Some(Self { regular: load_font(path, 480.0)?, bold: load_font(path, 700.0)? })
    }
}

pub struct Renderer {
    fonts: HashMap<String, Arc<FontSet>>,
    fallback: Arc<FontSet>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum Weight {
    Regular,
    Bold,
}

#[derive(Clone, Copy, PartialEq)]
enum Fx {
    None,
    Shadow,
    Outline,
}

struct Ctx<'a> {
    main: &'a FontSet,
    fallback: &'a FontSet,
    fx: Fx,
}

impl Ctx<'_> {
    fn font(&self, w: Weight) -> (&FontVec, &FontVec) {
        match w {
            Weight::Regular => (&self.main.regular, &self.fallback.regular),
            Weight::Bold => (&self.main.bold, &self.fallback.bold),
        }
    }

    fn ascent(&self, size: f32) -> f32 {
        self.main.regular.as_scaled(PxScale::from(size)).ascent()
    }

    /// Measure (pm = None) or draw text; returns advance width.
    #[allow(clippy::too_many_arguments)]
    fn text(&self, pm: Option<&mut Pixmap>, w: Weight, size: f32, x: f32, baseline: f32, s: &str, color: Rgba) -> f32 {
        let (main, fb) = self.font(w);
        let scale = PxScale::from(size);
        let mut caret = x;
        let mut prev: Option<(bool, ab_glyph::GlyphId)> = None;
        let mut glyphs = Vec::new();
        for ch in s.chars() {
            let mut id = main.glyph_id(ch);
            let mut use_fb = false;
            if id.0 == 0 && !ch.is_whitespace() {
                let fid = fb.glyph_id(ch);
                if fid.0 != 0 {
                    id = fid;
                    use_fb = true;
                }
            }
            let font: &FontVec = if use_fb { fb } else { main };
            let sf = font.as_scaled(scale);
            if let Some((pfb, pid)) = prev {
                if pfb == use_fb {
                    caret += sf.kern(pid, id);
                }
            }
            glyphs.push((use_fb, id.with_scale_and_position(scale, ab_glyph::point(caret, baseline))));
            caret += sf.h_advance(id);
            prev = Some((use_fb, id));
        }
        if let Some(pm) = pm {
            let r = (size / 14.0).clamp(1.0, 2.2);
            let a = ((color[3] as u32 * 235) / 255) as u8;
            let outline: [(f32, f32); 8] =
                [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (0.7, 0.7), (-0.7, 0.7), (0.7, -0.7), (-0.7, -0.7)];
            let passes: Vec<(f32, f32, Rgba)> = match self.fx {
                Fx::None => vec![],
                Fx::Shadow => vec![(r * 0.8, r * 0.8, [0, 0, 0, ((color[3] as u32 * 170) / 255) as u8])],
                Fx::Outline => {
                    let mut v: Vec<(f32, f32, Rgba)> = vec![(r * 1.2, r * 1.2, [0, 0, 0, a / 2])];
                    v.extend(outline.iter().map(|(dx, dy)| (dx * r, dy * r, [0, 0, 0, a])));
                    v
                }
            };
            for (dx, dy, c) in passes {
                for (use_fb, g) in &glyphs {
                    let font: &FontVec = if *use_fb { fb } else { main };
                    draw_glyph(pm, font, g.clone(), dx, dy, c);
                }
            }
            for (use_fb, g) in &glyphs {
                let font: &FontVec = if *use_fb { fb } else { main };
                draw_glyph(pm, font, g.clone(), 0.0, 0.0, color);
            }
        }
        caret - x
    }

    fn measure(&self, w: Weight, size: f32, s: &str) -> f32 {
        self.text(None, w, size, 0.0, 0.0, s, [0; 4])
    }
}

fn draw_glyph(pm: &mut Pixmap, font: &FontVec, mut g: ab_glyph::Glyph, dx: f32, dy: f32, c: Rgba) {
    g.position.x += dx;
    g.position.y += dy;
    let Some(outlined) = font.outline_glyph(g) else { return };
    let b = outlined.px_bounds();
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    let ca = c[3] as f32 / 255.0;
    outlined.draw(|gx, gy, cov| {
        let px = b.min.x as i32 + gx as i32;
        let py = b.min.y as i32 + gy as i32;
        if px < 0 || py < 0 || px >= w || py >= h {
            return;
        }
        let a = (cov.min(1.0) * ca).clamp(0.0, 1.0);
        if a <= 0.0 {
            return;
        }
        let i = ((py * w + px) * 4) as usize;
        let inv = 1.0 - a;
        data[i] = (c[0] as f32 * a + data[i] as f32 * inv) as u8;
        data[i + 1] = (c[1] as f32 * a + data[i + 1] as f32 * inv) as u8;
        data[i + 2] = (c[2] as f32 * a + data[i + 2] as f32 * inv) as u8;
        data[i + 3] = (255.0 * a + data[i + 3] as f32 * inv) as u8;
    });
}

pub fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    if r < 0.5 {
        pb.push_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?);
        return pb.finish();
    }
    let k = 0.5523 * r;
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.anti_alias = true;
    p.set_color_rgba8(c[0], c[1], c[2], c[3]);
    p
}

fn with_alpha(c: Rgba, a: u8) -> Rgba {
    [c[0], c[1], c[2], ((c[3] as u32 * a as u32) / 255) as u8]
}

fn fill_rrect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
    if c[3] == 0 {
        return;
    }
    if let Some(p) = rrect(x, y, w, h, r) {
        pm.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

/// Frametime scale: a floor of ~33 ms (30 FPS) so normal jitter stays a calm line and
/// only real hitches stand out - like classic OSD graphs.
pub fn frametime_scale(values: &[f32]) -> (f32, f32) {
    let mut sorted: Vec<f32> = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted.get(sorted.len() / 2).copied().unwrap_or(16.7).max(0.1);
    ((median * 3.0).max(1000.0 / 30.0), median)
}

#[allow(clippy::too_many_arguments)]
fn draw_graph(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, values: &[f32], max: f32, color: Rgba, st: &OverlayStyle, sc: f32, panel: bool) {
    if panel {
        fill_rrect(pm, x, y, w, h, (4.0 * sc).min(h / 2.0), with_alpha(st.label, 22));
    } else {
        // Baseline for panel-less graphs.
        fill_rrect(pm, x, y + h - 1.0 * sc, w, 1.0 * sc, 0.0, [0, 0, 0, 120]);
    }
    if values.len() < 2 || w < 4.0 {
        return;
    }
    let (max, median, ft) = if max <= 0.0 {
        let (m, med) = frametime_scale(values);
        (m, med, true)
    } else {
        (max, 0.0, false)
    };
    let n = values.len();
    let step = w / (n - 1) as f32;
    let inner_top = y + 2.0 * sc;
    let inner_h = h - 4.0 * sc;
    let py = |v: f32| inner_top + inner_h - (v / max).clamp(0.0, 1.0) * inner_h;

    let mut line = PathBuilder::new();
    let mut area = PathBuilder::new();
    area.move_to(x, y + h);
    for (i, &v) in values.iter().enumerate() {
        let (qx, qy) = (x + i as f32 * step, py(v));
        if i == 0 {
            line.move_to(qx, qy);
        } else {
            line.line_to(qx, qy);
        }
        area.line_to(qx, qy);
    }
    area.line_to(x + w, y + h);
    area.close();

    if panel {
        if let Some(a) = area.finish() {
            let mut p = Paint::default();
            p.anti_alias = true;
            if let Some(sh) = LinearGradient::new(
                Point::from_xy(0.0, y),
                Point::from_xy(0.0, y + h),
                vec![
                    GradientStop::new(0.0, Color::from_rgba8(color[0], color[1], color[2], 110)),
                    GradientStop::new(1.0, Color::from_rgba8(color[0], color[1], color[2], 0)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            ) {
                p.shader = sh;
                pm.fill_path(&a, &p, FillRule::Winding, Transform::identity(), None);
            }
        }
    }
    if let Some(l) = line.finish() {
        if !panel {
            // Dark halo so the line reads on bright scenes.
            let halo = Stroke { width: 3.4 * sc, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
            pm.stroke_path(&l, &paint([0, 0, 0, 150]), &halo, Transform::identity(), None);
        }
        let stroke = Stroke { width: 1.4 * sc, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
        pm.stroke_path(&l, &paint(color), &stroke, Transform::identity(), None);
    }
    if ft {
        for (i, &v) in values.iter().enumerate() {
            if v > (median * 2.5).max(median + 6.0) {
                if let Some(d) = PathBuilder::from_circle(x + i as f32 * step, py(v), 1.8 * sc) {
                    pm.fill_path(&d, &paint(st.bad), FillRule::Winding, Transform::identity(), None);
                }
            }
        }
    }
}

impl Renderer {
    pub fn new() -> Self {
        let def = crate::fonts::default_font_path();
        let fallback = FontSet::load(&def.to_string_lossy())
            .or_else(|| FontSet::load("C:\\Windows\\Fonts\\arial.ttf"))
            .expect("no usable system font");
        Self { fonts: HashMap::new(), fallback: Arc::new(fallback) }
    }

    fn fontset(&mut self, path: &str) -> Arc<FontSet> {
        if path.is_empty() {
            return self.fallback.clone();
        }
        if let Some(f) = self.fonts.get(path) {
            return f.clone();
        }
        let f = FontSet::load(path).map(Arc::new).unwrap_or_else(|| self.fallback.clone());
        self.fonts.insert(path.to_string(), f.clone());
        f
    }

    pub fn render(&mut self, scene: &Scene) -> Pixmap {
        let main = self.fontset(&scene.style.font_path);
        let fallback = self.fallback.clone();
        let st = scene.style;
        let panel_less = st.bg[3] < 60 || scene.layout == OverlayLayout::Classic;
        let fx = if !st.shadow && !panel_less {
            Fx::None
        } else if panel_less {
            Fx::Outline
        } else {
            Fx::Shadow
        };
        let ctx = Ctx { main: &main, fallback: &fallback, fx };
        match scene.layout {
            OverlayLayout::Card => card(&ctx, scene),
            OverlayLayout::Classic => classic(&ctx, scene),
            OverlayLayout::Bar => bar(&ctx, scene),
            OverlayLayout::Minimal => minimal(&ctx, scene),
        }
    }
}

fn new_pixmap(w: f32, h: f32) -> Pixmap {
    Pixmap::new(w.ceil().max(1.0) as u32, h.ceil().max(1.0) as u32).unwrap_or_else(|| Pixmap::new(1, 1).unwrap())
}

fn background(pm: &mut Pixmap, st: &OverlayStyle, sc: f32) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    let bw = st.border_width * sc;
    fill_rrect(pm, 0.0, 0.0, w, h, st.radius * sc, st.bg);
    if bw > 0.0 && st.border[3] > 0 {
        if let Some(p) = rrect(bw / 2.0, bw / 2.0, w - bw, h - bw, (st.radius * sc - bw / 2.0).max(0.0)) {
            let stroke = Stroke { width: bw, ..Default::default() };
            pm.stroke_path(&p, &paint(st.border), &stroke, Transform::identity(), None);
        }
    }
}

fn graphs<'a>(cells: &'a [Cell]) -> impl Iterator<Item = (&'a str, &'a str, &'a [f32], f32, Rgba)> {
    cells.iter().filter_map(|c| match c {
        Cell::Graph { title, value, values, max, color } => Some((title.as_str(), value.as_str(), values.as_slice(), *max, *color)),
        _ => None,
    })
}

// ─────────────────────────── shared pieces ───────────────────────────

fn part_width(ctx: &Ctx, fs: f32, p: &Part) -> f32 {
    let mut w = ctx.measure(Weight::Bold, fs, &p.value);
    if !p.unit.is_empty() {
        w += fs * 0.2 + ctx.measure(Weight::Regular, fs * 0.74, &p.unit);
    }
    w
}

fn draw_part(ctx: &Ctx, pm: &mut Pixmap, fs: f32, x: f32, base: f32, p: &Part, st: &OverlayStyle) -> f32 {
    let mut w = ctx.text(Some(pm), Weight::Bold, fs, x, base, &p.value, p.color.unwrap_or(st.text));
    if !p.unit.is_empty() {
        w += fs * 0.2;
        w += ctx.text(Some(pm), Weight::Regular, fs * 0.74, x + w, base, &p.unit, st.label);
    }
    w
}

fn parts_inline_width(ctx: &Ctx, fs: f32, parts: &[Part]) -> f32 {
    let gap = fs * 0.75;
    parts.iter().map(|p| part_width(ctx, fs, p)).sum::<f32>() + gap * parts.len().saturating_sub(1) as f32
}

fn draw_parts_inline(ctx: &Ctx, pm: &mut Pixmap, fs: f32, mut x: f32, base: f32, parts: &[Part], st: &OverlayStyle) {
    let gap = fs * 0.75;
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            x += gap;
        }
        x += draw_part(ctx, pm, fs, x, base, p, st);
    }
}

/// FPS block: optional grey real FPS, then the big value and its unit. Returns width.
#[allow(clippy::too_many_arguments)]
fn fps_block(ctx: &Ctx, pm: Option<&mut Pixmap>, big: f32, fs: f32, x: f32, base: f32, value: &str, color: Rgba, real: &Option<String>, unit: &str, st: &OverlayStyle) -> f32 {
    let real_size = big * 0.58;
    let unit_size = fs * 0.8;
    match pm {
        None => {
            let mut w = 0.0;
            if let Some(r) = real {
                w += ctx.measure(Weight::Bold, real_size, r) + fs * 0.45;
            }
            w + ctx.measure(Weight::Bold, big, value) + fs * 0.32 + ctx.measure(Weight::Bold, unit_size, unit)
        }
        Some(pm) => {
            let mut cx = x;
            if let Some(r) = real {
                let grey = [150, 150, 158, st.text[3]];
                cx += ctx.text(Some(pm), Weight::Bold, real_size, cx, base, r, grey) + fs * 0.45;
            }
            cx += ctx.text(Some(pm), Weight::Bold, big, cx, base, value, color) + fs * 0.32;
            cx += ctx.text(Some(pm), Weight::Bold, unit_size, cx, base, unit, st.label);
            cx - x
        }
    }
}

fn badge(ctx: &Ctx, pm: &mut Pixmap, fs: f32, x: f32, y: f32, text: &str, color: Rgba, pill: bool) {
    let bh = fs * 1.4;
    let tw = ctx.measure(Weight::Bold, fs * 0.8, text);
    if pill {
        fill_rrect(pm, x, y, tw + fs * 2.0, bh, bh / 2.0, with_alpha(color, 50));
    }
    if let Some(d) = PathBuilder::from_circle(x + fs * 0.7, y + bh / 2.0, fs * 0.24) {
        pm.fill_path(&d, &paint(color), FillRule::Winding, Transform::identity(), None);
    }
    ctx.text(Some(pm), Weight::Bold, fs * 0.8, x + fs * 1.2, y + bh / 2.0 + ctx.ascent(fs * 0.8) * 0.36, text, color);
}

// ─────────────────────────── Bottleneck strip ───────────────────────────

fn bottleneck<'a>(cells: &'a [Cell]) -> Option<&'a [Pill]> {
    cells.iter().find_map(|c| match c {
        Cell::Bottleneck { pills } if !pills.is_empty() => Some(pills.as_slice()),
        _ => None,
    })
}

fn pill_text(p: &Pill) -> (String, String) {
    (p.text.clone(), if p.bound { p.sub.clone() } else { String::new() })
}

/// Height of the strip for base font size `fs`.
fn strip_h(fs: f32) -> f32 {
    fs * 1.2
}

fn strip_width(ctx: &Ctx, fs: f32, pills: &[Pill]) -> f32 {
    let ts = fs * 0.68;
    let gap = fs * 0.3;
    pills
        .iter()
        .map(|p| {
            let (a, b) = pill_text(p);
            let mut w = ctx.measure(Weight::Bold, ts, &a) + fs * 0.7;
            if !b.is_empty() {
                w += ctx.measure(Weight::Regular, ts * 0.9, &b) + fs * 0.25;
            }
            w + gap
        })
        .sum::<f32>()
        - gap
}

fn draw_strip(ctx: &Ctx, pm: &mut Pixmap, fs: f32, mut x: f32, y: f32, pills: &[Pill]) {
    let ts = fs * 0.68;
    let gap = fs * 0.3;
    let h = strip_h(fs);
    let base = y + h / 2.0 + ctx.ascent(ts) * 0.36;
    for p in pills {
        let (a, b) = pill_text(p);
        let aw = ctx.measure(Weight::Bold, ts, &a);
        let bw = if b.is_empty() { 0.0 } else { ctx.measure(Weight::Regular, ts * 0.9, &b) + fs * 0.25 };
        let w = aw + bw + fs * 0.7;
        if p.bound {
            fill_rrect(pm, x, y, w, h, h * 0.3, with_alpha(p.color, 190));
            let dark = [255, 255, 255, 255];
            ctx.text(Some(pm), Weight::Bold, ts, x + fs * 0.35, base, &a, dark);
            ctx.text(Some(pm), Weight::Regular, ts * 0.9, x + fs * 0.35 + aw + fs * 0.25, base, &b, dark);
        } else {
            fill_rrect(pm, x, y, w, h, h * 0.3, with_alpha(p.color, 48));
            ctx.text(Some(pm), Weight::Bold, ts, x + fs * 0.35, base, &a, p.color);
        }
        x += w + gap;
    }
}

// ─────────────────────────── Card ───────────────────────────

fn card(ctx: &Ctx, scene: &Scene) -> Pixmap {
    let st = scene.style;
    let sc = scene.scale;
    let compact = scene.compact;
    let fs = st.font_size * sc;
    let pad = st.padding * sc * if compact { 0.7 } else { 1.0 };
    let big = fs * if compact { 1.8 } else { 2.35 };
    let row_h = fs * if compact { 1.3 } else { 1.45 };
    let head_fs = fs * 0.72;
    let col_gap = fs * 1.2;
    let graph_h = scene.graph_height * sc * if compact { 0.75 } else { 1.0 };

    // Column widths shared by all sections (two value columns).
    let mut col = [0f32; 2];
    let mut label_w: f32 = 0.0;
    for c in scene.cells {
        if let Cell::Section { title, parts, .. } = c {
            for (i, p) in parts.iter().enumerate() {
                col[i % 2] = col[i % 2].max(part_width(ctx, fs, p));
            }
            label_w = label_w.max(ctx.measure(Weight::Bold, head_fs, title));
        }
    }
    let grid_w = if col[1] > 0.0 { col[0] + col_gap + col[1] } else { col[0] };

    // Pass 1: size.
    let mut content_w: f32 = 0.0;
    let mut h = 0.0;
    for c in scene.cells {
        match c {
            Cell::Title { name, tag } => {
                let mut w = ctx.measure(Weight::Bold, fs * 0.92, name);
                if !tag.is_empty() {
                    w += fs * 0.8 + ctx.measure(Weight::Bold, fs * 0.66, tag) + fs * 0.8;
                }
                content_w = content_w.max(w);
                h += fs * if compact { 1.4 } else { 1.65 };
            }
            Cell::Fps { value, color, real, unit } => {
                content_w = content_w.max(fps_block(ctx, None, big, fs, 0.0, 0.0, value, *color, real, unit, st));
                h += big * 1.12;
            }
            Cell::Section { title, parts, .. } => {
                if compact {
                    let lw = if label_w > 0.0 { label_w + fs * 0.8 } else { 0.0 };
                    content_w = content_w.max(lw + parts_inline_width(ctx, fs, parts));
                    h += row_h;
                } else {
                    content_w = content_w.max(grid_w);
                    if !title.is_empty() {
                        h += fs * 1.2;
                    }
                    h += row_h * parts.len().div_ceil(2) as f32 + fs * 0.3;
                }
            }
            Cell::Badge { text, .. } => {
                content_w = content_w.max(ctx.measure(Weight::Bold, fs * 0.8, text) + fs * 2.0);
                h += fs * 1.75;
            }
            Cell::Graph { .. } | Cell::Bottleneck { .. } => {}
        }
    }
    let strip = bottleneck(scene.cells);
    if let Some(p) = strip {
        content_w = content_w.max(strip_width(ctx, fs, p));
        h += strip_h(fs) + fs * 0.35;
    }
    let gs: Vec<_> = graphs(scene.cells).collect();
    if !gs.is_empty() {
        content_w = content_w.max(190.0 * sc);
        for (title, value, ..) in &gs {
            let cw = ctx.measure(Weight::Bold, head_fs, title) + fs + ctx.measure(Weight::Bold, fs * 0.8, value);
            content_w = content_w.max(cw);
            h += fs * 1.2 + graph_h + fs * 0.45;
        }
    }
    content_w = content_w.max(110.0 * sc);
    let w = content_w + pad * 2.0;
    let total_h = h + pad * 2.0;

    let mut pm = new_pixmap(w, total_h);
    background(&mut pm, st, sc);

    // Pass 2: draw.
    let mut y = pad;
    let mut badges = Vec::new();
    for c in scene.cells {
        match c {
            Cell::Title { name, tag } => {
                let size = fs * 0.92;
                let base = y + ctx.ascent(size);
                ctx.text(Some(&mut pm), Weight::Bold, size, pad, base, name, st.text);
                if !tag.is_empty() {
                    let ts = fs * 0.66;
                    let tw = ctx.measure(Weight::Bold, ts, tag) + fs * 0.8;
                    let th = fs * 1.15;
                    let tx = w - pad - tw;
                    let ty = y + (size * 1.25 - th) / 2.0;
                    fill_rrect(&mut pm, tx, ty, tw, th, th / 2.0, with_alpha(st.accent, 46));
                    ctx.text(Some(&mut pm), Weight::Bold, ts, tx + fs * 0.4, ty + th / 2.0 + ctx.ascent(ts) * 0.36, tag, st.accent);
                }
                y += fs * if compact { 1.4 } else { 1.65 };
            }
            Cell::Fps { value, color, real, unit } => {
                let base = y + ctx.ascent(big) * 0.92;
                fps_block(ctx, Some(&mut pm), big, fs, pad - big * 0.03, base, value, *color, real, unit, st);
                y += big * 1.12;
            }
            Cell::Section { title, parts, .. } => {
                if compact {
                    let base = y + row_h * 0.5 + ctx.ascent(fs) * 0.36;
                    let mut x = pad;
                    if label_w > 0.0 {
                        ctx.text(Some(&mut pm), Weight::Bold, head_fs, pad, base, title, st.label);
                        x += label_w + fs * 0.8;
                    }
                    draw_parts_inline(ctx, &mut pm, fs, x, base, parts, st);
                    y += row_h;
                } else {
                    if !title.is_empty() {
                        let base = y + fs * 0.85;
                        ctx.text(Some(&mut pm), Weight::Bold, head_fs, pad, base, title, st.accent);
                        // Hairline under the header.
                        let tw = ctx.measure(Weight::Bold, head_fs, title);
                        fill_rrect(&mut pm, pad + tw + fs * 0.5, base - head_fs * 0.3, content_w - tw - fs * 0.5, 1.0 * sc, 0.0, with_alpha(st.label, 50));
                        y += fs * 1.2;
                    }
                    for (i, p) in parts.iter().enumerate() {
                        let r = i / 2;
                        let x = pad + if i % 2 == 0 { 0.0 } else { col[0] + col_gap };
                        let base = y + r as f32 * row_h + row_h * 0.5 + ctx.ascent(fs) * 0.36;
                        draw_part(ctx, &mut pm, fs, x, base, p, st);
                    }
                    y += row_h * parts.len().div_ceil(2) as f32 + fs * 0.3;
                }
            }
            Cell::Badge { text, color } => badges.push((text.clone(), *color)),
            Cell::Graph { .. } | Cell::Bottleneck { .. } => {}
        }
    }
    for (title, value, values, max, color) in gs {
        let base = y + fs * 0.85;
        ctx.text(Some(&mut pm), Weight::Bold, head_fs, pad, base, title, st.label);
        let vw = ctx.measure(Weight::Bold, fs * 0.8, value);
        ctx.text(Some(&mut pm), Weight::Bold, fs * 0.8, pad + content_w - vw, base, value, st.text);
        y += fs * 1.2;
        draw_graph(&mut pm, pad, y, content_w, graph_h, values, max, color, st, sc, st.bg[3] >= 60);
        y += graph_h + fs * 0.45;
    }
    if let Some(p) = strip {
        y += fs * 0.1;
        draw_strip(ctx, &mut pm, fs, pad, y, p);
        y += strip_h(fs) + fs * 0.25;
    }
    for (text, color) in badges {
        badge(ctx, &mut pm, fs, pad, y + fs * 0.12, &text, color, true);
        y += fs * 1.75;
    }
    pm
}

// ─────────────────────────── Classic ───────────────────────────

fn classic(ctx: &Ctx, scene: &Scene) -> Pixmap {
    let st = scene.style;
    let sc = scene.scale;
    let fs = st.font_size * sc;
    let pad = (st.padding * sc).max(3.0 * sc);
    let label_fs = fs * 1.05;
    let val_fs = fs * 1.15;
    let unit_fs = fs * 0.6;
    let row_h = fs * if scene.compact { 1.18 } else { 1.34 };
    let unit_gap = fs * 0.12;
    let col_gap = fs * 0.7;

    // Rows: (label, label colour, parts). The FPS row absorbs frame statistics.
    let tag = scene.cells.iter().find_map(|c| match c {
        Cell::Title { tag, .. } if !tag.is_empty() => Some(tag.clone()),
        _ => None,
    });
    let title = scene.cells.iter().find_map(|c| match c {
        Cell::Title { name, .. } => Some(name.clone()),
        _ => None,
    });
    let frames: Vec<Part> = scene
        .cells
        .iter()
        .filter_map(|c| match c {
            Cell::Section { title, parts, .. } if title.is_empty() => Some(parts.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    let mut rows: Vec<(String, Rgba, Vec<Part>)> = Vec::new();
    let mut fps_row: Option<usize> = None;
    for c in scene.cells {
        match c {
            Cell::Fps { value, color, real, unit } => {
                let mut parts = Vec::new();
                if let Some(r) = real {
                    parts.push(Part::colored(r.clone(), "real", [170, 170, 178, 255]));
                }
                parts.push(Part::colored(value.clone(), unit.clone(), *color));
                parts.extend(frames.iter().cloned());
                fps_row = Some(rows.len());
                rows.push((tag.clone().unwrap_or_else(|| "FPS".into()), [255, 120, 190, 255], parts));
            }
            Cell::Section { title, color, parts } if !title.is_empty() => rows.push((title.clone(), *color, parts.clone())),
            _ => {}
        }
    }
    if fps_row.is_none() && !frames.is_empty() {
        rows.insert(0, (tag.clone().unwrap_or_else(|| "FPS".into()), [255, 120, 190, 255], frames.clone()));
    }

    // Column metrics across rows.
    // The FPS row is laid out inline; the other rows share aligned columns.
    let aligned = |i: usize| Some(i) != fps_row;
    let ncol = rows.iter().enumerate().filter(|(i, _)| aligned(*i)).map(|(_, r)| r.2.len()).max().unwrap_or(0);
    let mut vw = vec![0f32; ncol];
    let mut uw = vec![0f32; ncol];
    for (_, _, parts) in rows.iter().enumerate().filter(|(i, _)| aligned(*i)).map(|(_, r)| r) {
        for (j, p) in parts.iter().enumerate() {
            vw[j] = vw[j].max(ctx.measure(Weight::Bold, val_fs, &p.value));
            uw[j] = uw[j].max(ctx.measure(Weight::Bold, unit_fs, &p.unit));
        }
    }
    let label_w = rows.iter().map(|r| ctx.measure(Weight::Bold, label_fs, &r.0)).fold(0.0, f32::max);
    let cols_w: f32 = (0..ncol).map(|j| vw[j] + unit_gap + uw[j]).sum::<f32>() + col_gap * ncol as f32;
    let inline_w = |parts: &[Part]| -> f32 {
        parts
            .iter()
            .map(|p| ctx.measure(Weight::Bold, val_fs, &p.value) + unit_gap + ctx.measure(Weight::Bold, unit_fs, &p.unit) + col_gap)
            .sum()
    };
    let mut content_w = label_w + cols_w;
    if let Some(i) = fps_row {
        content_w = content_w.max(label_w + col_gap + inline_w(&rows[i].2));
    }
    let mut h = rows.len() as f32 * row_h;
    if let Some(t) = &title {
        content_w = content_w.max(ctx.measure(Weight::Bold, label_fs, t));
        h += row_h;
    }
    let gs: Vec<_> = graphs(scene.cells).collect();
    let graph_h = scene.graph_height * sc * 0.8;
    if !gs.is_empty() {
        content_w = content_w.max(220.0 * sc);
        h += gs.len() as f32 * (fs * 0.95 + graph_h + fs * 0.35);
    }
    let badges: Vec<(String, Rgba)> = scene
        .cells
        .iter()
        .filter_map(|c| match c {
            Cell::Badge { text, color } => Some((text.clone(), *color)),
            _ => None,
        })
        .collect();
    h += badges.len() as f32 * fs * 1.6;
    let strip = bottleneck(scene.cells);
    if let Some(p) = strip {
        content_w = content_w.max(strip_width(ctx, fs, p));
        h += strip_h(fs) + fs * 0.4;
    }

    let mut pm = new_pixmap(content_w + pad * 2.0, h + pad * 2.0);
    background(&mut pm, st, sc);

    let mut y = pad;
    if let Some(t) = &title {
        ctx.text(Some(&mut pm), Weight::Bold, label_fs, pad, y + row_h * 0.5 + ctx.ascent(label_fs) * 0.36, t, st.label);
        y += row_h;
    }
    for (ri, (label, lc, parts)) in rows.iter().enumerate() {
        let base = y + row_h * 0.5 + ctx.ascent(val_fs) * 0.36;
        ctx.text(Some(&mut pm), Weight::Bold, label_fs, pad, base, label, *lc);
        let mut x = pad + label_w + col_gap;
        if !aligned(ri) {
            for p in parts {
                x += ctx.text(Some(&mut pm), Weight::Bold, val_fs, x, base, &p.value, p.color.unwrap_or(st.text)) + unit_gap;
                x += ctx.text(Some(&mut pm), Weight::Bold, unit_fs, x, base - val_fs * 0.38, &p.unit, st.label) + col_gap;
            }
            y += row_h;
            continue;
        }
        for (j, p) in parts.iter().enumerate() {
            let w = ctx.measure(Weight::Bold, val_fs, &p.value);
            ctx.text(Some(&mut pm), Weight::Bold, val_fs, x + vw[j] - w, base, &p.value, p.color.unwrap_or(st.text));
            // Superscript unit.
            ctx.text(Some(&mut pm), Weight::Bold, unit_fs, x + vw[j] + unit_gap, base - val_fs * 0.38, &p.unit, st.label);
            x += vw[j] + unit_gap + uw[j] + col_gap;
        }
        y += row_h;
    }
    for (title, value, values, max, color) in gs {
        let base = y + fs * 0.8;
        ctx.text(Some(&mut pm), Weight::Bold, fs * 0.66, pad, base, title, color);
        let w = ctx.measure(Weight::Bold, fs * 0.66, value);
        ctx.text(Some(&mut pm), Weight::Bold, fs * 0.66, pad + content_w - w, base, value, color);
        y += fs * 0.95;
        draw_graph(&mut pm, pad, y, content_w, graph_h, values, max, color, st, sc, false);
        y += graph_h + fs * 0.35;
    }
    if let Some(p) = strip {
        y += fs * 0.15;
        draw_strip(ctx, &mut pm, fs, pad, y, p);
        y += strip_h(fs) + fs * 0.25;
    }
    for (text, color) in badges {
        badge(ctx, &mut pm, fs, pad - fs * 0.3, y, &text, color, false);
        y += fs * 1.6;
    }
    pm
}

// ─────────────────────────── Bar ───────────────────────────

fn bar(ctx: &Ctx, scene: &Scene) -> Pixmap {
    let st = scene.style;
    let sc = scene.scale;
    let fs = st.font_size * sc;
    let pad_x = st.padding * sc;
    let h = fs * 2.0;
    let sep = fs * 0.65;
    let label_fs = fs * 0.72;
    let fps_fs = fs * 1.25;
    let spark_w = 90.0 * sc;

    let cell_w = |c: &Cell| -> f32 {
        match c {
            Cell::Title { name, .. } => ctx.measure(Weight::Bold, fs * 0.9, name),
            Cell::Fps { value, color, real, unit } => fps_block(ctx, None, fps_fs, label_fs, 0.0, 0.0, value, *color, real, unit, st),
            Cell::Section { title, parts, .. } => {
                let lw = if title.is_empty() { 0.0 } else { ctx.measure(Weight::Bold, label_fs, title) + fs * 0.4 };
                lw + parts_inline_width(ctx, fs, parts)
            }
            Cell::Graph { .. } => spark_w,
            Cell::Badge { text, .. } => ctx.measure(Weight::Bold, fs * 0.8, text) + fs * 1.1,
            Cell::Bottleneck { pills } => strip_width(ctx, fs, pills),
        }
    };
    let widths: Vec<f32> = scene.cells.iter().map(cell_w).collect();
    let total = widths.iter().sum::<f32>() + sep * 2.0 * widths.len().saturating_sub(1) as f32 + pad_x * 2.0;
    let mut pm = new_pixmap(total.max(40.0 * sc), h);
    background(&mut pm, st, sc);

    let base = h / 2.0 + ctx.ascent(fs) * 0.36;
    let mut x = pad_x;
    for (i, c) in scene.cells.iter().enumerate() {
        if i > 0 {
            x += sep;
            fill_rrect(&mut pm, x - 0.5 * sc, h * 0.28, 1.0 * sc, h * 0.44, 0.0, with_alpha(st.label, 70));
            x += sep;
        }
        match c {
            Cell::Title { name, .. } => {
                ctx.text(Some(&mut pm), Weight::Bold, fs * 0.9, x, base, name, st.text);
            }
            Cell::Fps { value, color, real, unit } => {
                let b = h / 2.0 + ctx.ascent(fps_fs) * 0.36;
                fps_block(ctx, Some(&mut pm), fps_fs, label_fs, x, b, value, *color, real, unit, st);
            }
            Cell::Section { title, parts, .. } => {
                let mut px = x;
                if !title.is_empty() {
                    px += ctx.text(Some(&mut pm), Weight::Bold, label_fs, x, base, title, st.label) + fs * 0.4;
                }
                draw_parts_inline(ctx, &mut pm, fs, px, base, parts, st);
            }
            Cell::Graph { values, max, color, .. } => {
                draw_graph(&mut pm, x, h * 0.22, spark_w, h * 0.56, values, *max, *color, st, sc, st.bg[3] >= 60);
            }
            Cell::Badge { text, color } => {
                if let Some(d) = PathBuilder::from_circle(x + fs * 0.3, h / 2.0, fs * 0.24) {
                    pm.fill_path(&d, &paint(*color), FillRule::Winding, Transform::identity(), None);
                }
                ctx.text(Some(&mut pm), Weight::Bold, fs * 0.8, x + fs * 0.75, base, text, *color);
            }
            Cell::Bottleneck { pills } => draw_strip(ctx, &mut pm, fs, x, (h - strip_h(fs)) / 2.0, pills),
        }
        x += widths[i];
    }
    pm
}

// ─────────────────────────── Minimal ───────────────────────────

fn minimal(ctx: &Ctx, scene: &Scene) -> Pixmap {
    let st = scene.style;
    let sc = scene.scale;
    let fs = st.font_size * sc;
    let pad = st.padding * sc;
    let big = fs * 2.6;
    let small = fs * 0.85;

    let fps = scene.cells.iter().find_map(|c| match c {
        Cell::Fps { value, color, real, unit } => Some((value.clone(), *color, real.clone(), unit.clone())),
        _ => None,
    });
    let sub: Vec<Part> = scene
        .cells
        .iter()
        .filter_map(|c| match c {
            Cell::Section { title, parts, .. } if title.is_empty() => Some(parts.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    let badge_c = scene.cells.iter().find_map(|c| match c {
        Cell::Badge { text, color } => Some((text.clone(), *color)),
        _ => None,
    });

    let (fv, fc, real, unit) = fps.unwrap_or_else(|| ("-".into(), st.text, None, "FPS".into()));
    let fw = fps_block(ctx, None, big, fs, 0.0, 0.0, &fv, fc, &real, &unit, st);
    let sw = if sub.is_empty() { 0.0 } else { parts_inline_width(ctx, small, &sub) };
    let bw = badge_c.as_ref().map(|(t, _)| ctx.measure(Weight::Bold, small, t) + fs * 1.4).unwrap_or(0.0);
    let strip = bottleneck(scene.cells);
    let stw = strip.map(|p| strip_width(ctx, small, p)).unwrap_or(0.0);
    let w = fw.max(sw).max(bw).max(stw) + pad * 2.0;
    let mut h = pad * 2.0 + big * 1.05;
    if strip.is_some() {
        h += strip_h(small) + small * 0.4;
    }
    if !sub.is_empty() {
        h += small * 1.4;
    }
    if badge_c.is_some() {
        h += small * 1.6;
    }
    let mut pm = new_pixmap(w, h);
    background(&mut pm, st, sc);

    let mut y = pad;
    let base = y + ctx.ascent(big) * 0.92;
    fps_block(ctx, Some(&mut pm), big, fs, pad, base, &fv, fc, &real, &unit, st);
    y += big * 1.05;
    if !sub.is_empty() {
        draw_parts_inline(ctx, &mut pm, small, pad, y + ctx.ascent(small), &sub, st);
        y += small * 1.4;
    }
    if let Some(p) = strip {
        y += small * 0.15;
        draw_strip(ctx, &mut pm, small, pad, y, p);
        y += strip_h(small) + small * 0.25;
    }
    if let Some((t, c)) = badge_c {
        badge(ctx, &mut pm, small, pad - small * 0.3, y, &t, c, false);
    }
    pm
}

#[cfg(test)]
mod tests {
    use super::*;
    use ac_core::config::builtin_styles;

    pub fn sample() -> Vec<Cell> {
        let ft: Vec<f32> = (0..120).map(|i| 3.5 + ((i * 7) % 11) as f32 * 0.05 + if i == 80 { 14.0 } else { 0.0 }).collect();
        vec![
            Cell::Title { name: "GTA5_Enhanced".into(), tag: "D3D12".into() },
            Cell::Fps { value: "232".into(), color: [74, 222, 128, 255], real: Some("115".into()), unit: "DLSS".into() },
            Cell::Section {
                title: String::new(),
                color: [255; 4],
                parts: vec![
                    Part::new("3.6", "ms"),
                    Part::new("228", "avg"),
                    Part::new("57", "1% low"),
                    Part::new("47", "0.1% low"),
                    Part::new("251", "max"),
                ],
            },
            Cell::Section {
                title: "GPU".into(),
                color: [118, 214, 106, 255],
                parts: vec![Part::new("97", "%"), Part::new("61", "°C"), Part::new("2797", "MHz"), Part::new("135", "W")],
            },
            Cell::Section { title: "VRAM".into(), color: [118, 214, 106, 255], parts: vec![Part::new("9.2", "/ 16 GB")] },
            Cell::Section { title: "CPU".into(), color: [64, 160, 255, 255], parts: vec![Part::new("68", "%"), Part::new("4.14", "GHz")] },
            Cell::Section { title: "RAM".into(), color: [64, 160, 255, 255], parts: vec![Part::new("25.3", "/ 32 GB")] },
            Cell::Section { title: "ВРЕМЯ".into(), color: [200, 200, 200, 255], parts: vec![Part::new("13:41:07", "")] },
            Cell::Graph { title: "Frametime".into(), value: "3.6 ms".into(), values: ft, max: 0.0, color: [167, 139, 250, 255] },
            Cell::Graph { title: "GPU".into(), value: "97 %".into(), values: (0..120).map(|i| 60.0 + (i % 30) as f32).collect(), max: 100.0, color: [118, 214, 106, 255] },
            Cell::Bottleneck { pills: vec![
                Pill { text: "GPU 97%".into(), sub: "bound · power limit".into(), color: [248, 113, 113, 255], bound: true },
                Pill { text: "CPU 64%".into(), sub: "".into(), color: [74, 222, 128, 255], bound: false },
                Pill { text: "VRAM 58%".into(), sub: "".into(), color: [74, 222, 128, 255], bound: false },
                Pill { text: "RAM 81%".into(), sub: "".into(), color: [250, 204, 21, 255], bound: false },
            ] },
            Cell::Badge { text: "REC 00:12".into(), color: [248, 113, 113, 255] },
        ]
    }

    #[test]
    fn renders_all_layouts() {
        let mut r = Renderer::new();
        let cells = sample();
        let out = std::env::var("AC_RENDER_OUT").ok();
        for (si, style) in builtin_styles().iter().enumerate() {
            for layout in OverlayLayout::ALL {
                for compact in [false, true] {
                    let pm = r.render(&Scene { cells: &cells, style: &style.style, layout, scale: 1.25, graph_height: 42.0, compact });
                    assert!(pm.width() > 50 && pm.height() > 20, "{layout:?} {}x{}", pm.width(), pm.height());
                    assert!(pm.data().chunks(4).any(|p| p[3] > 200), "nothing drawn");
                    if let Some(dir) = out.as_ref() {
                        // Composite on a bright "game" background to judge readability.
                        let mut bg = Pixmap::new(pm.width() + 40, pm.height() + 40).unwrap();
                        bg.fill(Color::from_rgba8(176, 140, 100, 255));
                        let mut sky = Paint::default();
                        sky.set_color_rgba8(235, 235, 240, 255);
                        bg.fill_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, bg.width() as f32, bg.height() as f32 * 0.35).unwrap(), &sky, Transform::identity(), None);
                        bg.draw_pixmap(20, 20, pm.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
                        let _ = bg.save_png(format!("{dir}/overlay_{si}_{layout:?}_{compact}.png"));
                    }
                }
            }
        }
    }

    #[test]
    fn frametime_scale_has_floor() {
        let (max, med) = frametime_scale(&[3.5; 50]);
        assert!((med - 3.5).abs() < 1e-3);
        assert!(max >= 33.0);
    }
}
