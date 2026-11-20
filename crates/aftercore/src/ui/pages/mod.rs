mod bench;
mod cores;
mod dashboard;
mod hotkeys;
mod overlay;
mod profiles;
mod settings;

use eframe::egui::{self, pos2, Align2, Color32, Rect, Shape, Stroke};

use super::theme::{alpha, body, Pal};

/// Large frametime chart with grid and spike markers.
pub fn frametime_chart(ui: &mut egui::Ui, pal: &Pal, rect: Rect, values: &[f32], color: Color32) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 12, pal.surface2);
    if values.len() < 2 {
        p.text(rect.center(), Align2::CENTER_CENTER, "Нет данных о кадрах", body(13.0), pal.faint);
        return;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[sorted.len() / 2].max(0.1);
    // Same calm scale as the overlay: floor at 33 ms so jitter does not look like stutter.
    let (max, _) = ac_overlay::render::frametime_scale(values);
    let inner = rect.shrink2(egui::vec2(46.0, 14.0)).translate(egui::vec2(18.0, 0.0));

    for i in 0..=3 {
        let v = max * i as f32 / 3.0;
        let y = inner.bottom() - inner.height() * i as f32 / 3.0;
        p.line_segment([pos2(inner.left(), y), pos2(inner.right(), y)], Stroke::new(1.0_f32, alpha(pal.border, 200)));
        p.text(pos2(inner.left() - 8.0, y), Align2::RIGHT_CENTER, format!("{v:.0} ms"), body(11.0), pal.faint);
    }

    let n = values.len();
    let pts: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            pos2(
                inner.left() + inner.width() * i as f32 / (n - 1) as f32,
                inner.bottom() - (v / max).clamp(0.0, 1.0) * inner.height(),
            )
        })
        .collect();
    let mut mesh = egui::Mesh::default();
    for (i, pt) in pts.iter().enumerate() {
        mesh.colored_vertex(*pt, alpha(color, 80));
        mesh.colored_vertex(pos2(pt.x, inner.bottom()), alpha(color, 0));
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b + 1, b);
        }
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::line(pts.clone(), Stroke::new(1.8_f32, color)));
    for (pt, v) in pts.iter().zip(values) {
        if *v > (median * 2.5).max(median + 6.0) {
            p.circle_filled(*pt, 3.0, pal.bad);
        }
    }
}
