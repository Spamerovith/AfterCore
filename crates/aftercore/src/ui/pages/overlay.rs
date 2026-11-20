use std::sync::atomic::Ordering;

use ac_core::config::{
    builtin_styles, unique_name, Anchor, AnchorTarget, Metric, NamedStyle, OverlayLayout, ShowMode,
};
use ac_telemetry::{FrameStats, Source};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, Layout, Margin, Rect, Sense, Stroke, StrokeKind};

use crate::core::{build_cells, LiveData};
use crate::ui::theme::{alpha, bold, body, icon, mix, Pal};
use crate::ui::widgets::{self, ico};
use crate::ui::App;

fn demo_stats() -> FrameStats {
    let t = ac_core::win::qpc_seconds();
    let graph: Vec<f32> = (0..180)
        .map(|i| {
            let x = i as f64 * 0.21 + t * 3.0;
            (6.9 + (x.sin() * 0.45 + (x * 2.7).cos() * 0.25) + if i == 130 { 9.0 } else { 0.0 }) as f32
        })
        .collect();
    FrameStats {
        pid: 0,
        source: Some(Source::Dxgi),
        fps: 144.0 + (t.sin() * 3.0) as f32,
        frametime_ms: 6.9,
        low1: 118.0,
        low01: 96.0,
        min_fps: 90.0,
        max_fps: 151.0,
        avg_fps: 143.0,
        real_fps: None,
        fps_hist: (0..120).map(|i| 140.0 + ((i as f64 * 0.3 + t).sin() * 6.0) as f32).collect(),
        graph,
        presenting: true,
    }
}

impl App {
    pub fn page_overlay(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        widgets::page_title(ui, &pal, "Оверлей", "Как выглядит и что показывает счётчик поверх игры");

        if !self.cfg.profiles.iter().any(|p| p.name == self.ov.edit_profile) {
            self.ov.edit_profile = self.cfg.active_profile.clone();
        }
        let _ = self.fonts();

        let total = ui.available_width();
        let right_w = (total * 0.40).clamp(340.0, 520.0);
        let left_w = total - right_w - 16.0;

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(left_w);
                self.overlay_settings(ui);
            });
            ui.vertical(|ui| {
                ui.set_width(right_w);
                self.overlay_preview(ui);
            });
        });
        let _ = pal;
    }

    fn overlay_preview(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let idx = self.cfg.profiles.iter().position(|p| p.name == self.ov.edit_profile).unwrap_or(0);
        let ov = self.cfg.profiles[idx].overlay.clone();
        let game = self.core.last_game.read().clone();
        let live = (game.pid != 0)
            .then(|| self.core.frames.stats(game.pid, ov.low_window_s as f64, ov.graph_frames as usize))
            .flatten()
            .filter(|s| s.presenting);
        let stats = live.clone().unwrap_or_else(demo_stats);
        let exe = if live.is_some() { game.exe.clone() } else { "Cyberpunk2077.exe".into() };
        let sensors = self.core.sensors.get();
        let tech = if live.is_some() { self.core.tech(game.pid) } else { None };
        let verdict = live.as_ref().map(|l| self.core.verdict(game.pid, &sensors, Some(l)));
        let cells = build_cells(
            &ov,
            &LiveData {
                exe: &exe,
                stats: Some(&stats),
                sensors: &sensors,
                tech: tech.as_ref(),
                recording: self.core.frames.is_recording().map(|r| r.1),
                notice: None,
                verdict: verdict.as_ref(),
            },
        );
        let ppp = ui.ctx().pixels_per_point();
        let pm = self.ov.renderer.render(&ac_overlay::Scene {
            cells: &cells,
            style: &ov.style,
            layout: ov.layout,
            scale: ov.scale * ppp * 0.85,
            graph_height: ov.graph_height,
            compact: ov.compact,
        });
        let img = egui::ColorImage::from_rgba_premultiplied([pm.width() as usize, pm.height() as usize], pm.data());
        match self.ov.tex.as_mut() {
            Some(t) => t.set(img, egui::TextureOptions::LINEAR),
            None => self.ov.tex = Some(ui.ctx().load_texture("overlay_preview", img, egui::TextureOptions::LINEAR)),
        }
        let tex = self.ov.tex.clone().unwrap();
        let img_size = vec2(pm.width() as f32, pm.height() as f32) / ppp;

        widgets::section(ui, &pal, ico::EYE, "Предпросмотр", if live.is_some() { "Живые данные игры" } else { "Демо-данные" }, |ui| {
            let w = ui.available_width();
            let h = (w * 9.0 / 16.0).max(img_size.y + 24.0);
            let (scene, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
            paint_fake_game(ui, &pal, scene);
            let (fx, fy) = ov.anchor.factors();
            let off = vec2(ov.offset[0] as f32, ov.offset[1] as f32) * 0.5;
            let ox = if fx < 0.25 { off.x } else if fx > 0.75 { -off.x } else { off.x };
            let oy = if fy < 0.25 { off.y } else if fy > 0.75 { -off.y } else { off.y };
            let min = pos2(
                scene.left() + (scene.width() - img_size.x) * fx + ox,
                scene.top() + (scene.height() - img_size.y) * fy + oy,
            );
            let r = Rect::from_min_size(min, img_size);
            let tint = Color32::from_white_alpha((ov.style.opacity.clamp(0.1, 1.0) * 255.0) as u8);
            ui.painter_at(scene).image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
            ui.add_space(6.0);
            let mut desk = self.core.preview_on_desktop.load(Ordering::Relaxed);
            if widgets::toggle_row(ui, &pal, "Показать на рабочем столе", "Оверлей поверх всех окон, пока включено", &mut desk) {
                self.core.preview_on_desktop.store(desk, Ordering::Relaxed);
            }
        });
        ui.add_space(4.0);
        widgets::card(ui, &pal, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(ico::INFO).font(icon(14.0)).color(pal.accent));
                ui.label(egui::RichText::new("Как это работает").font(bold(14.0)).color(pal.text));
            });
            widgets::muted(
                ui,
                &pal,
                "Оверлей - отдельное прозрачное окно поверх игры: клики проходят сквозь него, в процесс игры ничего не \
                 внедряется. Работает в оконном и безрамочном режимах, а также в «полноэкранном» режиме Windows 10/11 \
                 (с оптимизацией полноэкранного режима)",
            );
        });
    }

    fn overlay_settings(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let hwinfo = self.core.sensors.get().hwinfo;

        // Profile being edited.
        widgets::card(ui, &pal, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Профиль").font(bold(14.0)).color(pal.text));
                let names: Vec<String> = self.cfg.profiles.iter().map(|p| p.name.clone()).collect();
                egui::ComboBox::from_id_salt("ov_profile").selected_text(self.ov.edit_profile.clone()).show_ui(ui, |ui| {
                    for n in names {
                        ui.selectable_value(&mut self.ov.edit_profile, n.clone(), n);
                    }
                });
                if self.ov.edit_profile == self.cfg.active_profile {
                    widgets::pill(ui, &pal, "активный", pal.good);
                } else if widgets::ghost(ui, &pal, ico::CHECK, "Сделать активным").clicked() {
                    self.cfg.active_profile = self.ov.edit_profile.clone();
                }
            });
        });
        ui.add_space(4.0);

        let idx = self.cfg.profiles.iter().position(|p| p.name == self.ov.edit_profile).unwrap_or(0);
        let fonts = self.fonts.clone().unwrap_or_default();
        let custom_styles = self.cfg.custom_styles.clone();
        let mut save_style: Option<NamedStyle> = None;
        let mut delete_style: Option<String> = None;
        let ov = &mut self.cfg.profiles[idx].overlay;

        widgets::section(ui, &pal, ico::OVERLAY, "Вид", "Раскладка и когда показывать", |ui| {
            widgets::toggle_row(ui, &pal, "Оверлей включён", "Для этого профиля", &mut ov.enabled);
            ui.add_space(4.0);
            layout_cards(ui, &pal, &mut ov.layout);
            widgets::toggle_row(
                ui,
                &pal,
                "Компактный режим",
                "Плотнее строки: подпись и значения в одну линию, меньше отступов",
                &mut ov.compact,
            );
            widgets::toggle_row(
                ui,
                &pal,
                "FPS без генератора кадров",
                "С DLSS / FSR Frame Generation слева серым - настоящие кадры игры (нужен Reflex в игре)",
                &mut ov.show_real_fps,
            );
            widgets::toggle_row(ui, &pal, "Подписи на русском", "ВРЕМЯ, ИГРА, сред, макс, упор и т.д. вместо английских", &mut ov.russian);
            ui.add_space(4.0);
            widgets::row(ui, &pal, "Когда показывать", "«Только в играх» - полноэкранные приложения и игры с правилом", |ui| {
                widgets::segmented(
                    ui,
                    &pal,
                    &mut ov.show_mode,
                    &[(ShowMode::GamesOnly, "В играх"), (ShowMode::AnyPresenting, "Любое 3D"), (ShowMode::Always, "Всегда")],
                );
            });
        });
        ui.add_space(4.0);

        widgets::section(ui, &pal, ico::LAYERS, "Положение", "Угол экрана, отступы и масштаб", |ui| {
            ui.horizontal_top(|ui| {
                anchor_picker(ui, &pal, &mut ov.anchor);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(ov.anchor.title()).font(bold(14.0)).color(pal.text));
                    widgets::segmented(
                        ui,
                        &pal,
                        &mut ov.anchor_target,
                        &[(AnchorTarget::Window, "К окну игры"), (AnchorTarget::Monitor, "К монитору")],
                    );
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("X").color(pal.muted));
                        ui.add(egui::DragValue::new(&mut ov.offset[0]).range(0..=2000).suffix(" px"));
                        ui.label(egui::RichText::new("Y").color(pal.muted));
                        ui.add(egui::DragValue::new(&mut ov.offset[1]).range(0..=2000).suffix(" px"));
                    });
                });
            });
            widgets::slider(ui, &pal, "Масштаб", &mut ov.scale, 0.5..=3.0, "×");
        });
        ui.add_space(4.0);

        widgets::section(ui, &pal, ico::SPEED, "Метрики", "Нажмите на метрику, чтобы показать или скрыть её", |ui| {
            // 1. Compact chips grouped by category.
            for group in Metric::GROUPS {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(118.0, 32.0), Sense::hover());
                    ui.painter().text(
                        pos2(r.left(), r.center().y),
                        Align2::LEFT_CENTER,
                        group,
                        bold(12.5),
                        pal.muted,
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                        for i in 0..ov.items.len() {
                            let it = ov.items[i];
                            if it.metric.group() != group {
                                continue;
                            }
                            let mut resp = widgets::chip(ui, &pal, it.metric.short(), it.enabled);
                            resp = match it.metric.hint() {
                                Some(h) => resp.on_hover_text(format!("{}
{}", it.metric.title(), h)),
                                None => resp.on_hover_text(it.metric.title()),
                            };
                            if resp.clicked() {
                                ov.items[i].enabled = !it.enabled;
                            }
                        }
                    });
                });
                ui.add_space(2.0);
            }
            if !hwinfo
                && ov.items.iter().any(|i| i.enabled && matches!(i.metric, Metric::CpuTemp | Metric::CpuVoltage | Metric::CpuFan))
            {
                ui.add_space(2.0);
                widgets::muted(
                    ui,
                    &pal,
                    "Температура, напряжение и вентилятор CPU берутся из запущенного MSI Afterburner или HWiNFO (Shared Memory Support). Без них эти значения не показываются",
                );
            }
            ui.add_space(6.0);

            // 2. Order of the enabled metrics, folded by default.
            let open_id = ui.id().with("metric_order_open");
            let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
            let enabled: Vec<usize> = (0..ov.items.len()).filter(|&i| ov.items[i].enabled).collect();
            ui.horizontal(|ui| {
                let label = if open { "Скрыть порядок" } else { "Изменить порядок" };
                if widgets::ghost(ui, &pal, if open { ico::UP } else { ico::DOWN }, label).clicked() {
                    open = !open;
                }
                widgets::muted(ui, &pal, &format!("Включено: {}", enabled.len()));
            });
            ui.data_mut(|d| d.insert_temp(open_id, open));
            if open {
                widgets::muted(ui, &pal, "Секции идут в порядке первой метрики. Графики и узкое место - всегда внизу");
                let mut swap: Option<(usize, usize)> = None;
                for (k, &i) in enabled.iter().enumerate() {
                    let it = ov.items[i];
                    egui::Frame::new()
                        .fill(pal.surface2)
                        .corner_radius(8)
                        .inner_margin(Margin::symmetric(10, 2))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("{:>2}", k + 1)).font(body(12.0)).color(pal.faint));
                                ui.label(egui::RichText::new(it.metric.title()).font(body(13.5)).color(pal.text));
                                ui.label(egui::RichText::new(it.metric.group()).font(body(11.0)).color(pal.faint));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if widgets::icon_btn(ui, &pal, ico::DELETE, "Скрыть").clicked() {
                                        ov.items[i].enabled = false;
                                    }
                                    if k + 1 < enabled.len() && widgets::icon_btn(ui, &pal, ico::DOWN, "Ниже").clicked() {
                                        swap = Some((i, enabled[k + 1]));
                                    }
                                    if k > 0 && widgets::icon_btn(ui, &pal, ico::UP, "Выше").clicked() {
                                        swap = Some((i, enabled[k - 1]));
                                    }
                                });
                            });
                        });
                    ui.add_space(2.0);
                }
                if let Some((a, b)) = swap {
                    ov.items.swap(a, b);
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if widgets::ghost(ui, &pal, "", "Только FPS").clicked() {
                    for it in ov.items.iter_mut() {
                        it.enabled = matches!(it.metric, Metric::Fps | Metric::FrameTime);
                    }
                }
                if widgets::ghost(ui, &pal, "", "Геймер").clicked() {
                    for it in ov.items.iter_mut() {
                        it.enabled = matches!(
                            it.metric,
                            Metric::Process
                                | Metric::Fps
                                | Metric::FrameTime
                                | Metric::Low1
                                | Metric::Graph
                                | Metric::GpuLoad
                                | Metric::GpuTemp
                                | Metric::CpuLoad
                                | Metric::CpuMaxCore
                                | Metric::CpuTemp
                                | Metric::Bottleneck
                        );
                    }
                }
                if widgets::ghost(ui, &pal, "", "Всё").clicked() {
                    for it in ov.items.iter_mut() {
                        it.enabled = true;
                    }
                }
            });
        });
        ui.add_space(4.0);

        widgets::section(ui, &pal, ico::COLOR, "Стиль", "Темы, шрифт и цвета оверлея", |ui| {
            ui.label(egui::RichText::new("Темы").font(bold(13.0)).color(pal.muted));
            ui.horizontal_wrapped(|ui| {
                for s in builtin_styles() {
                    let selected = ov.style == s.style;
                    if widgets::chip(ui, &pal, &s.name, selected).clicked() {
                        ov.style = s.style.clone();
                    }
                }
                for s in custom_styles.iter() {
                    let selected = ov.style == s.style;
                    if widgets::chip(ui, &pal, &format!("★ {}", s.name), selected).clicked() {
                        ov.style = s.style.clone();
                    }
                    if widgets::icon_btn(ui, &pal, ico::DELETE, "Удалить тему").clicked() {
                        delete_style = Some(s.name.clone());
                    }
                }
            });
            ui.horizontal(|ui| {
                widgets::input(ui, &mut self.ov.new_style_name, "Название своей темы", 220.0);
                if widgets::ghost(ui, &pal, ico::SAVE, "Сохранить как тему").clicked() {
                    let base = if self.ov.new_style_name.trim().is_empty() { "Моя тема" } else { self.ov.new_style_name.trim() };
                    let name = unique_name(base, |n| custom_styles.iter().any(|s| s.name == n));
                    save_style = Some(NamedStyle { name, style: ov.style.clone() });
                    self.ov.new_style_name.clear();
                }
            });
            widgets::divider(ui, &pal);

            ui.label(egui::RichText::new("Шрифт").font(bold(13.0)).color(pal.muted));
            font_picker(ui, &pal, "ov_font", &fonts, &mut self.ov.font_filter, &mut ov.style.font_name, &mut ov.style.font_path);
            widgets::slider(ui, &pal, "Размер текста", &mut ov.style.font_size, 10.0..=34.0, " px");
            widgets::divider(ui, &pal);

            ui.label(egui::RichText::new("Цвета").font(bold(13.0)).color(pal.muted));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 18.0;
                let st = &mut ov.style;
                widgets::color_edit(ui, &pal, "Фон", &mut st.bg);
                widgets::color_edit(ui, &pal, "Рамка", &mut st.border);
                widgets::color_edit(ui, &pal, "Значения", &mut st.text);
                widgets::color_edit(ui, &pal, "Подписи", &mut st.label);
                widgets::color_edit(ui, &pal, "Акцент", &mut st.accent);
                widgets::color_edit(ui, &pal, "График", &mut st.graph);
                widgets::color_edit(ui, &pal, "Хорошо", &mut st.good);
                widgets::color_edit(ui, &pal, "Средне", &mut st.warn);
                widgets::color_edit(ui, &pal, "Плохо", &mut st.bad);
            });
            widgets::toggle_row(ui, &pal, "Цвет FPS по порогам", "Зелёный / жёлтый / красный", &mut ov.style.fps_colors);
            if ov.style.fps_colors {
                widgets::slider(ui, &pal, "Жёлтый ниже", &mut ov.style.fps_warn, 20.0..=240.0, " FPS");
                widgets::slider(ui, &pal, "Красный ниже", &mut ov.style.fps_bad, 10.0..=120.0, " FPS");
            }
            widgets::divider(ui, &pal);
            widgets::slider(ui, &pal, "Скругление", &mut ov.style.radius, 0.0..=28.0, " px");
            widgets::slider(ui, &pal, "Отступы", &mut ov.style.padding, 2.0..=28.0, " px");
            widgets::slider(ui, &pal, "Толщина рамки", &mut ov.style.border_width, 0.0..=4.0, " px");
            widgets::slider(ui, &pal, "Непрозрачность", &mut ov.style.opacity, 0.1..=1.0, "");
            widgets::toggle_row(ui, &pal, "Тень текста", "Читаемость на светлых сценах", &mut ov.style.shadow);
        });
        ui.add_space(4.0);

        widgets::section(ui, &pal, ico::SETTINGS, "Дополнительно", "Частота обновления, окно расчёта и приватность", |ui| {
            widgets::slider(ui, &pal, "Обновление", &mut ov.refresh_ms, 16..=1000, " мс");
            widgets::slider(ui, &pal, "Окно статистики (avg, max, lows)", &mut ov.low_window_s, 2..=60, " с");
            widgets::slider(ui, &pal, "Кадров на графике", &mut ov.graph_frames, 30..=600, "");
            widgets::slider(ui, &pal, "Высота графика", &mut ov.graph_height, 20.0..=120.0, " px");
            widgets::toggle_row(
                ui,
                &pal,
                "Скрывать из записи и стрима",
                "Оверлей видите только вы - OBS и Discord его не захватят. В играх с античитом выключается сам:                  античит должен видеть, что на экране только FPS",
                &mut ov.hide_from_capture,
            );
        });

        if let Some(s) = save_style {
            self.toast(format!("Тема «{}» сохранена", s.name), pal.good);
            self.cfg.custom_styles.push(s);
        }
        if let Some(n) = delete_style {
            self.cfg.custom_styles.retain(|s| s.name != n);
        }
    }
}

fn layout_cards(ui: &mut egui::Ui, pal: &Pal, layout: &mut OverlayLayout) {
    ui.columns(4, |c| {
        for (i, l) in OverlayLayout::ALL.iter().enumerate() {
            let ui = &mut c[i];
            let selected = *layout == *l;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 92.0), Sense::click());
            let t = ui.ctx().animate_bool_with_time(resp.id, selected, 0.15);
            let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.1);
            let p = ui.painter();
            p.rect_filled(rect, 14, mix(pal.surface2, pal.surface3, h));
            p.rect_stroke(rect, 14, Stroke::new(1.0 + t, mix(pal.border, pal.accent, t)), StrokeKind::Inside);
            // Mini illustration.
            let ill = Rect::from_min_size(rect.min + vec2(14.0, 14.0), vec2(rect.width() - 28.0, 44.0));
            let a = mix(pal.muted, pal.accent, t);
            match l {
                OverlayLayout::Card => {
                    let r = Rect::from_min_size(ill.min, vec2(46.0, 44.0));
                    p.rect_filled(r, 6, alpha(a, 50));
                    for k in 0..4 {
                        let y = r.top() + 8.0 + k as f32 * 9.0;
                        p.line_segment([pos2(r.left() + 6.0, y), pos2(r.right() - 6.0 - (k % 2) as f32 * 10.0, y)], Stroke::new(2.0_f32, a));
                    }
                }
                OverlayLayout::Bar => {
                    let r = Rect::from_min_size(ill.min + vec2(0.0, 14.0), vec2(ill.width().min(130.0), 14.0));
                    p.rect_filled(r, 7, alpha(a, 50));
                    for k in 0..4 {
                        let x = r.left() + 8.0 + k as f32 * (r.width() - 16.0) / 4.0;
                        p.line_segment([pos2(x, r.center().y), pos2(x + 16.0, r.center().y)], Stroke::new(2.0_f32, a));
                    }
                }
                OverlayLayout::Classic => {
                    let cols = [pal.good, pal.accent2, pal.warn];
                    for k in 0..3 {
                        let y = ill.top() + 8.0 + k as f32 * 13.0;
                        p.line_segment([pos2(ill.left(), y), pos2(ill.left() + 22.0, y)], Stroke::new(3.0_f32, cols[k]));
                        p.line_segment([pos2(ill.left() + 30.0, y), pos2(ill.left() + 52.0, y)], Stroke::new(3.0_f32, alpha(egui::Color32::from_rgb(255, 160, 30), if t > 0.5 { 255 } else { 160 })));
                    }
                }
                OverlayLayout::Minimal => {
                    p.text(ill.left_center(), Align2::LEFT_CENTER, "144", bold(28.0), a);
                }
            }
            p.text(pos2(rect.left() + 14.0, rect.bottom() - 18.0), Align2::LEFT_CENTER, l.title(), bold(13.5), mix(pal.text, pal.accent, t * 0.4));
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *layout = *l;
            }
        }
    });
}

fn anchor_picker(ui: &mut egui::Ui, pal: &Pal, anchor: &mut Anchor) {
    let size = vec2(132.0, 84.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 12, pal.surface2);
    p.rect_stroke(rect, 12, Stroke::new(1.0_f32, pal.border), StrokeKind::Inside);
    for (i, a) in Anchor::GRID.iter().enumerate() {
        let (fx, fy) = a.factors();
        let c = pos2(rect.left() + 18.0 + fx * (rect.width() - 36.0), rect.top() + 16.0 + fy * (rect.height() - 32.0));
        let r = Rect::from_center_size(c, vec2(26.0, 18.0));
        let resp = ui.interact(r, ui.id().with(("anchor", i)), Sense::click()).on_hover_text(a.title());
        let sel = *a == *anchor;
        let col = if sel { pal.accent } else if resp.hovered() { pal.muted } else { alpha(pal.muted, 90) };
        ui.painter().rect_filled(r.shrink(if sel { 0.0 } else { 3.0 }), 5, col);
        if resp.clicked() {
            *anchor = *a;
        }
    }
}

pub fn font_picker(
    ui: &mut egui::Ui,
    pal: &Pal,
    id: &str,
    fonts: &[ac_overlay::fonts::FontEntry],
    filter: &mut String,
    name: &mut String,
    path: &mut String,
) {
    ui.horizontal(|ui| {
        let shown = if name.is_empty() { "Segoe UI Variable (по умолчанию)".to_string() } else { name.clone() };
        ui.label(egui::RichText::new(shown).font(bold(14.0)).color(pal.accent));
        if !name.is_empty() && widgets::icon_btn(ui, pal, ico::REFRESH, "Шрифт по умолчанию").clicked() {
            name.clear();
            path.clear();
        }
    });
    widgets::search(ui, pal, filter, "Поиск среди установленных шрифтов…", 320.0);
    egui::Frame::new().fill(pal.surface2).corner_radius(10).inner_margin(Margin::same(6)).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt(id).max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let f = filter.to_lowercase();
            for e in fonts.iter().filter(|e| f.is_empty() || e.name.to_lowercase().contains(&f)).take(400) {
                let sel = *path == e.path;
                if ui.selectable_label(sel, egui::RichText::new(&e.name).font(body(13.5))).clicked() {
                    *name = e.name.clone();
                    *path = e.path.clone();
                }
            }
        });
    });
    widgets::muted(ui, pal, "Свои .ttf/.otf можно положить в папку data\\fonts рядом с AfterCore.exe");
}

/// Stylised "game scene" background for the preview.
fn paint_fake_game(ui: &mut egui::Ui, pal: &Pal, r: Rect) {
    let p = ui.painter_at(r);
    let mut mesh = egui::Mesh::default();
    let top = Color32::from_rgb(28, 36, 70);
    let mid = Color32::from_rgb(86, 58, 120);
    let bottom = Color32::from_rgb(214, 122, 98);
    let rows = [(0.0, top), (0.55, mid), (1.0, bottom)];
    for (k, (t, c)) in rows.iter().enumerate() {
        let y = r.top() + r.height() * t;
        mesh.colored_vertex(pos2(r.left(), y), *c);
        mesh.colored_vertex(pos2(r.right(), y), *c);
        if k > 0 {
            let b = (k as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b + 1, b);
        }
    }
    p.add(egui::Shape::mesh(mesh));
    // Sun + mountains.
    p.circle_filled(pos2(r.left() + r.width() * 0.68, r.top() + r.height() * 0.62), r.height() * 0.16, Color32::from_rgb(255, 196, 120));
    let base = r.bottom();
    let ridge = [
        (0.0, 0.70),
        (0.12, 0.58),
        (0.25, 0.68),
        (0.4, 0.52),
        (0.55, 0.66),
        (0.7, 0.56),
        (0.85, 0.70),
        (1.0, 0.60),
    ];
    // One mesh (no anti-aliased seams): a strip from the ridge line down to the bottom edge.
    let mountain = Color32::from_rgb(40, 26, 56);
    let mut m = egui::Mesh::default();
    for (i, (x, y)) in ridge.iter().enumerate() {
        let px = r.left() + r.width() * x;
        m.colored_vertex(pos2(px, r.top() + r.height() * y), mountain);
        m.colored_vertex(pos2(px, base), mountain);
        if i > 0 {
            let b = (i as u32) * 2;
            m.add_triangle(b - 2, b - 1, b);
            m.add_triangle(b - 1, b + 1, b);
        }
    }
    p.add(egui::Shape::mesh(m));
    let ground: Vec<egui::Pos2> = vec![
        pos2(r.left(), base),
        pos2(r.left(), r.top() + r.height() * 0.82),
        pos2(r.right(), r.top() + r.height() * 0.78),
        pos2(r.right(), base),
    ];
    p.add(egui::Shape::convex_polygon(ground, Color32::from_rgb(20, 14, 30), Stroke::NONE));
    // Rounded mask edge.
    p.rect_stroke(r, 12, Stroke::new(6.0_f32, pal.surface), StrokeKind::Outside);
    p.rect_stroke(r, 12, Stroke::new(1.0_f32, pal.border), StrokeKind::Inside);
}
