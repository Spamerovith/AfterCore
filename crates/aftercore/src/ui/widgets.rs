//! AfterCore's own widgets on top of egui.

use ac_core::config::Rgba;
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, Layout, Margin, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui,
};

use super::theme::{alpha, bold, body, icon, mix, rgba, to_rgba, Pal};

pub mod ico {
    pub const HOME: &str = "\u{E80F}";
    pub const OVERLAY: &str = "\u{E7F4}";
    pub const CPU: &str = "\u{E950}";
    pub const PROFILES: &str = "\u{E77B}";
    pub const BENCH: &str = "\u{E9D9}";
    pub const KEYBOARD: &str = "\u{E765}";
    pub const SETTINGS: &str = "\u{E713}";
    pub const MIN: &str = "\u{E921}";
    pub const MAX: &str = "\u{E922}";
    pub const RESTORE: &str = "\u{E923}";
    pub const CLOSE: &str = "\u{E8BB}";
    pub const ADD: &str = "\u{E710}";
    pub const DELETE: &str = "\u{E74D}";
    pub const EDIT: &str = "\u{E70F}";
    pub const COPY: &str = "\u{E8C8}";
    pub const SAVE: &str = "\u{E74E}";
    pub const CHECK: &str = "\u{E73E}";
    pub const UP: &str = "\u{E70E}";
    pub const DOWN: &str = "\u{E70D}";
    pub const FOLDER: &str = "\u{E838}";
    pub const STOP: &str = "\u{E71A}";
    pub const SHIELD: &str = "\u{EA18}";
    pub const INFO: &str = "\u{E946}";
    pub const WARN: &str = "\u{E7BA}";
    pub const GAME: &str = "\u{E7FC}";
    pub const BOLT: &str = "\u{E945}";
    pub const SPEED: &str = "\u{EC4A}";
    pub const COLOR: &str = "\u{E790}";
    pub const EYE: &str = "\u{E7B3}";
    pub const REFRESH: &str = "\u{E72C}";
    pub const EXPORT: &str = "\u{EDE1}";
    pub const SEARCH: &str = "\u{E721}";
    pub const MEMORY: &str = "\u{EEA0}";
    pub const TEMP: &str = "\u{E9CA}";
    pub const LAYERS: &str = "\u{E81E}";
    pub const CLOCK: &str = "\u{E823}";
    pub const RECORD: &str = "\u{E7C8}";
    pub const LOCK: &str = "\u{E72E}";
    pub const UNLOCK: &str = "\u{E785}";
}

/// Rounded surface card.
pub fn card<R>(ui: &mut Ui, pal: &Pal, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(pal.surface)
        .stroke(Stroke::new(1.0_f32, pal.border))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Card with a header (icon, title, optional subtitle).
pub fn section<R>(ui: &mut Ui, pal: &Pal, ic: &str, title: &str, subtitle: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    card(ui, pal, |ui| {
        header(ui, pal, ic, title, subtitle);
        ui.add_space(6.0);
        add(ui)
    })
}

pub fn header(ui: &mut Ui, pal: &Pal, ic: &str, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
        ui.painter().rect_filled(r, 10, pal.accent_soft);
        ui.painter().text(r.center(), Align2::CENTER_CENTER, ic, icon(16.0), pal.accent);
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(egui::RichText::new(title).font(bold(16.0)).color(pal.text));
            if !subtitle.is_empty() {
                ui.label(egui::RichText::new(subtitle).font(body(12.5)).color(pal.muted));
            }
        });
    });
}

pub fn muted(ui: &mut Ui, pal: &Pal, text: &str) -> Response {
    ui.label(egui::RichText::new(text).font(body(12.5)).color(pal.muted))
}

/// iOS-style switch.
pub fn toggle(ui: &mut Ui, pal: &Pal, on: &mut bool) -> Response {
    let size = vec2(42.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.16);
    let p = ui.painter();
    let track = mix(pal.surface3, pal.accent, t);
    p.rect_filled(rect, 12, track);
    if !*on {
        p.rect_stroke(rect, 12, Stroke::new(1.0_f32, pal.border), StrokeKind::Inside);
    }
    if resp.hovered() {
        p.rect_filled(rect.expand(3.0), 15, alpha(pal.accent, 24));
    }
    let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
    let knob = if *on { pal.on_accent } else { pal.muted };
    p.circle_filled(pos2(x, rect.center().y + 1.0), 9.0, Color32::from_black_alpha(40));
    p.circle_filled(pos2(x, rect.center().y), 9.0, mix(knob, Color32::WHITE, if *on { 0.0 } else { 0.4 }));
    resp
}

/// Title + description on the left, switch on the right.
pub fn toggle_row(ui: &mut Ui, pal: &Pal, title: &str, desc: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.set_max_width(ui.available_width() - 64.0);
            ui.label(egui::RichText::new(title).font(body(14.5)).color(pal.text));
            if !desc.is_empty() {
                ui.label(egui::RichText::new(desc).font(body(12.5)).color(pal.muted));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            changed = toggle(ui, pal, on).changed();
        });
    });
    changed
}

/// Label column + control on the right (generic row).
pub fn row(ui: &mut Ui, pal: &Pal, title: &str, desc: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.set_width((ui.available_width() * 0.42).max(180.0));
            ui.label(egui::RichText::new(title).font(body(14.5)).color(pal.text));
            if !desc.is_empty() {
                ui.label(egui::RichText::new(desc).font(body(12.5)).color(pal.muted));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), add);
    });
}

pub fn divider(ui: &mut Ui, pal: &Pal) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0, pal.border);
}

/// Selectable pill.
pub fn chip(ui: &mut Ui, pal: &Pal, text: &str, selected: bool) -> Response {
    let font = body(13.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), pal.text);
    let size = vec2(galley.size().x + 26.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let t = ui.ctx().animate_bool_with_time(resp.id, selected, 0.14);
    let hover = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.1);
    let p = ui.painter();
    let fill = mix(mix(pal.surface2, pal.surface3, hover), pal.accent, t);
    p.rect_filled(rect, 16, fill);
    if t < 0.5 {
        p.rect_stroke(rect, 16, Stroke::new(1.0_f32, mix(pal.border, pal.accent, hover * 0.6)), StrokeKind::Inside);
    }
    let col = mix(pal.text, pal.on_accent, t);
    p.text(rect.center(), Align2::CENTER_CENTER, text, font, col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Segmented control. Returns true when changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, pal: &Pal, value: &mut T, items: &[(T, &str)]) -> bool {
    let font = body(13.5);
    let widths: Vec<f32> =
        items.iter().map(|(_, t)| ui.painter().layout_no_wrap(t.to_string(), font.clone(), pal.text).size().x + 28.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 8.0;
    let (rect, _) = ui.allocate_exact_size(vec2(total, 36.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 11, pal.surface2);
    p.rect_stroke(rect, 11, Stroke::new(1.0_f32, pal.border), StrokeKind::Inside);

    let id = ui.id().with("seg").with(rect.min.x as i32).with(rect.min.y as i32);
    let sel = items.iter().position(|(v, _)| v == value).unwrap_or(0);
    let mut x = rect.left() + 4.0;
    let mut sel_x = x;
    for (i, w) in widths.iter().enumerate() {
        if i == sel {
            sel_x = x;
        }
        x += w;
    }
    let ax = ui.ctx().animate_value_with_time(id, sel_x, 0.16);
    let aw = ui.ctx().animate_value_with_time(id.with("w"), widths[sel], 0.16);
    p.rect_filled(Rect::from_min_size(pos2(ax, rect.top() + 4.0), vec2(aw, 28.0)), 8, pal.accent);

    let mut changed = false;
    let mut x = rect.left() + 4.0;
    for (i, ((v, t), w)) in items.iter().zip(&widths).enumerate() {
        let r = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(*w, 28.0));
        let resp = ui.interact(r, id.with(i), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.clicked() && v != value {
            *value = *v;
            changed = true;
        }
        let col = if i == sel { pal.on_accent } else if resp.hovered() { pal.text } else { pal.muted };
        p.text(r.center(), Align2::CENTER_CENTER, *t, font.clone(), col);
        x += w;
    }
    changed
}

fn button_impl(ui: &mut Ui, pal: &Pal, ic: &str, text: &str, primary: bool, danger: bool) -> Response {
    let font = body(14.0);
    let tw = if text.is_empty() { 0.0 } else { ui.painter().layout_no_wrap(text.to_string(), font.clone(), pal.text).size().x };
    let iw = if ic.is_empty() { 0.0 } else { 16.0 };
    let gap = if ic.is_empty() || text.is_empty() { 0.0 } else { 8.0 };
    let w = tw + iw + gap + if text.is_empty() { 18.0 } else { 30.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(36.0), 36.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let pressed = resp.is_pointer_button_down_on();
    let r = if pressed { rect.shrink(1.0) } else { rect };
    let p = ui.painter();
    let (fill, fg) = if primary {
        (mix(pal.accent, Color32::WHITE, h * 0.12), pal.on_accent)
    } else if danger {
        (mix(pal.surface2, pal.bad, 0.12 + h * 0.18), pal.bad)
    } else {
        (mix(pal.surface2, pal.surface3, h), pal.text)
    };
    if primary && pal.glow {
        p.rect_filled(r.expand(2.0 + h * 2.0), 13, alpha(pal.accent, (30.0 + 40.0 * h) as u8));
    }
    p.rect_filled(r, 10, fill);
    if !primary {
        p.rect_stroke(r, 10, Stroke::new(1.0_f32, mix(pal.border, pal.accent, h * 0.5)), StrokeKind::Inside);
    }
    let mut x = r.center().x - (tw + iw + gap) / 2.0;
    if !ic.is_empty() {
        p.text(pos2(x + iw / 2.0, r.center().y), Align2::CENTER_CENTER, ic, icon(14.0), fg);
        x += iw + gap;
    }
    if !text.is_empty() {
        p.text(pos2(x, r.center().y), Align2::LEFT_CENTER, text, font, fg);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn primary(ui: &mut Ui, pal: &Pal, ic: &str, text: &str) -> Response {
    button_impl(ui, pal, ic, text, true, false)
}

pub fn ghost(ui: &mut Ui, pal: &Pal, ic: &str, text: &str) -> Response {
    button_impl(ui, pal, ic, text, false, false)
}

pub fn danger(ui: &mut Ui, pal: &Pal, ic: &str, text: &str) -> Response {
    button_impl(ui, pal, ic, text, false, true)
}

/// Small square icon button.
pub fn icon_btn(ui: &mut Ui, pal: &Pal, ic: &str, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let p = ui.painter();
    p.rect_filled(rect, 8, alpha(pal.surface3, (h * 255.0) as u8));
    p.text(rect.center(), Align2::CENTER_CENTER, ic, icon(13.0), mix(pal.muted, pal.text, h));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// Status pill: coloured dot + text.
pub fn pill(ui: &mut Ui, pal: &Pal, text: &str, color: Color32) -> Response {
    let font = body(12.5);
    let tw = ui.painter().layout_no_wrap(text.to_string(), font.clone(), pal.text).size().x;
    let (rect, resp) = ui.allocate_exact_size(vec2(tw + 30.0, 26.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 13, alpha(color, 30));
    p.circle_filled(pos2(rect.left() + 12.0, rect.center().y), 3.5, color);
    p.text(pos2(rect.left() + 21.0, rect.center().y), Align2::LEFT_CENTER, text, font, mix(color, pal.text, 0.35));
    resp
}

pub fn sparkline(p: &egui::Painter, rect: Rect, values: &[f32], max: f32, color: Color32, fill: bool) {
    if values.len() < 2 {
        return;
    }
    let max = max.max(1e-3);
    let n = values.len();
    let pts: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            pos2(
                rect.left() + rect.width() * i as f32 / (n - 1) as f32,
                rect.bottom() - (v / max).clamp(0.0, 1.0) * rect.height(),
            )
        })
        .collect();
    if fill {
        // Vertical gradient via a triangle mesh.
        let mut mesh = egui::Mesh::default();
        let top = alpha(color, 70);
        let bottom = alpha(color, 0);
        for (i, pt) in pts.iter().enumerate() {
            mesh.colored_vertex(*pt, top);
            mesh.colored_vertex(pos2(pt.x, rect.bottom()), bottom);
            if i > 0 {
                let b = (i as u32) * 2;
                mesh.add_triangle(b - 2, b - 1, b);
                mesh.add_triangle(b - 1, b + 1, b);
            }
        }
        p.add(Shape::mesh(mesh));
    }
    p.add(Shape::line(pts, Stroke::new(1.6_f32, color)));
}

/// Metric tile with a big value and an optional sparkline.
#[allow(clippy::too_many_arguments)]
pub fn stat_tile(
    ui: &mut Ui,
    pal: &Pal,
    ic: &str,
    title: &str,
    value: &str,
    unit: &str,
    sub: &str,
    hist: Option<(&[f32], f32)>,
    color: Color32,
) {
    egui::Frame::new()
        .fill(pal.surface)
        .stroke(Stroke::new(1.0_f32, pal.border))
        .corner_radius(16)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(136.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                ui.painter().rect_filled(r, 8, alpha(color, 36));
                ui.painter().text(r.center(), Align2::CENTER_CENTER, ic, icon(13.0), color);
                ui.label(egui::RichText::new(title).font(body(13.0)).color(pal.muted));
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(egui::RichText::new(value).font(bold(28.0)).color(pal.text));
                ui.label(egui::RichText::new(unit).font(body(13.0)).color(pal.muted));
            });
            ui.label(egui::RichText::new(if sub.is_empty() { " " } else { sub }).font(body(12.0)).color(pal.faint));
            let rest = ui.available_rect_before_wrap();
            if let Some((h, max)) = hist {
                // Auto-scale so low loads are still readable.
                let peak = h.iter().cloned().fold(0.0, f32::max).max(max * 0.15) * 1.25;
                let r = Rect::from_min_max(pos2(rest.left(), rest.top() + 2.0), pos2(rest.right(), rest.bottom()));
                if r.height() > 6.0 {
                    sparkline(ui.painter(), r, h, peak.min(max * 1.25), color, true);
                }
            }
        });
}

/// Colour swatch editor with label.
pub fn color_edit(ui: &mut Ui, pal: &Pal, label: &str, c: &mut Rgba) -> bool {
    let mut col = rgba(*c);
    let mut changed = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(egui::RichText::new(label).font(body(12.0)).color(pal.muted));
        if egui::color_picker::color_edit_button_srgba(ui, &mut col, egui::color_picker::Alpha::OnlyBlend).changed() {
            *c = to_rgba(col);
            changed = true;
        }
    });
    changed
}

/// Slider with title on the left.
pub fn slider<N: egui::emath::Numeric>(
    ui: &mut Ui,
    pal: &Pal,
    title: &str,
    v: &mut N,
    range: std::ops::RangeInclusive<N>,
    suffix: &str,
) -> bool {
    let mut changed = false;
    row(ui, pal, title, "", |ui| {
        changed = ui.add(egui::Slider::new(v, range).suffix(suffix)).changed();
    });
    changed
}

/// Page title with subtitle.
pub fn page_title(ui: &mut Ui, pal: &Pal, title: &str, subtitle: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(title).font(bold(28.0)).color(pal.text));
    ui.label(egui::RichText::new(subtitle).font(body(14.0)).color(pal.muted));
    ui.add_space(10.0);
}

/// Text field with a leading search icon.
pub fn search(ui: &mut Ui, pal: &Pal, text: &mut String, hint: &str, width: f32) -> Response {
    egui::Frame::new()
        .fill(pal.surface2)
        .stroke(Stroke::new(1.0_f32, pal.border))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(ico::SEARCH).font(icon(13.0)).color(pal.muted));
                ui.add(egui::TextEdit::singleline(text).hint_text(hint).frame(false).desired_width(width - 40.0))
            })
            .inner
        })
        .inner
}

pub fn input(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(width).margin(Margin::symmetric(10, 7)))
}
