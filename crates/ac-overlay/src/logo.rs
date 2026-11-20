//! The AfterCore logo, drawn in code (used for the exe icon, the window and the tray).
//!
//! A rounded "squircle" with a violet → cyan gradient and a white frametime pulse.
//! This file is also `#[path]`-included by the app's build script, so it only depends on tiny-skia.

use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, PathBuilder, Pixmap, Point, SpreadMode,
    Stroke, Transform,
};

pub fn logo_pixmap(size: u32) -> Pixmap {
    let mut pm = Pixmap::new(size, size).expect("logo size");
    let s = size as f32;
    let pad = s * 0.04;
    let r = s * 0.26;
    let (x, y, w, h) = (pad, pad, s - pad * 2.0, s - pad * 2.0);

    let k = 0.5523 * r;
    let mut pb = PathBuilder::new();
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
    let body = pb.finish().expect("logo path");

    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.shader = LinearGradient::new(
        Point::from_xy(x, y),
        Point::from_xy(x + w, y + h),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(139, 92, 246, 255)),
            GradientStop::new(0.55, Color::from_rgba8(99, 102, 241, 255)),
            GradientStop::new(1.0, Color::from_rgba8(34, 211, 238, 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("gradient");
    pm.fill_path(&body, &paint, FillRule::Winding, Transform::identity(), None);

    // Soft top highlight.
    let mut hl = Paint::default();
    hl.anti_alias = true;
    hl.shader = LinearGradient::new(
        Point::from_xy(0.0, y),
        Point::from_xy(0.0, y + h * 0.6),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(255, 255, 255, 60)),
            GradientStop::new(1.0, Color::from_rgba8(255, 255, 255, 0)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("gradient");
    pm.fill_path(&body, &hl, FillRule::Winding, Transform::identity(), None);

    // Pulse: flat → spike up → deep down → flat.
    let mid = y + h * 0.56;
    let pts = [
        (0.16, 0.0),
        (0.36, 0.0),
        (0.46, -0.30),
        (0.58, 0.22),
        (0.66, -0.08),
        (0.84, -0.08),
    ];
    let mut pl = PathBuilder::new();
    for (i, (px, py)) in pts.iter().enumerate() {
        let (qx, qy) = (x + w * px, mid + h * py);
        if i == 0 {
            pl.move_to(qx, qy);
        } else {
            pl.line_to(qx, qy);
        }
    }
    let pulse = pl.finish().expect("pulse");
    let mut white = Paint::default();
    white.anti_alias = true;
    white.set_color_rgba8(255, 255, 255, 255);
    let stroke = Stroke { width: (s * 0.085).max(1.4), line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    pm.stroke_path(&pulse, &white, &stroke, Transform::identity(), None);

    // Dot at the end of the line.
    if let Some(dot) = PathBuilder::from_circle(x + w * 0.84, mid - h * 0.08, (s * 0.075).max(1.2)) {
        pm.fill_path(&dot, &white, FillRule::Winding, Transform::identity(), None);
    }
    pm
}

/// Straight (non-premultiplied) RGBA bytes, as expected by icon APIs.
pub fn logo_rgba(size: u32) -> Vec<u8> {
    let pm = logo_pixmap(size);
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for p in pm.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}
