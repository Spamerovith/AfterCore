use ac_core::hotkey::HotkeyAction;
use eframe::egui::{self, pos2, vec2, Align, Align2, Layout, Margin, Rect, Sense, Stroke};

use crate::ui::theme::{alpha, bold, body};
use crate::ui::widgets::{self, ico};
use crate::ui::App;

impl App {
    pub fn page_bench(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        widgets::page_title(ui, &pal, "Бенчмарк", "Запись frametime игры: средний FPS, 1% и 0.1% low, статтеры, CSV");

        let rec = self.core.frames.is_recording();
        let game = self.core.last_game.read().clone();
        let hk = self.cfg.hotkeys.get(&HotkeyAction::BenchToggle).map(|h| h.label());

        widgets::card(ui, &pal, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - 260.0);
                    match rec {
                        Some((pid, secs)) => {
                            let s = secs as u64;
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                                let pulse = ((ui.input(|i| i.time) * 3.0).sin() * 0.5 + 0.5) as f32;
                                ui.painter().circle_filled(r.center(), 5.0 + pulse * 2.0, pal.bad);
                                ui.label(egui::RichText::new(format!("Идёт запись · {:02}:{:02}", s / 60, s % 60)).font(bold(22.0)).color(pal.text));
                            });
                            let name = self.core.process_name(pid);
                            let fps = self.core.frames.stats(pid, 1.0, 1).map(|s| s.fps).unwrap_or(0.0);
                            widgets::muted(ui, &pal, &format!("{name} · сейчас {fps:.0} FPS"));
                            if self.cfg.bench.duration_s > 0 {
                                let t = (secs as f32 / self.cfg.bench.duration_s as f32).clamp(0.0, 1.0);
                                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
                                ui.painter().rect_filled(r, 4, pal.surface3);
                                ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width() * t, 8.0)), 4, pal.accent);
                            }
                        }
                        None => {
                            ui.label(egui::RichText::new("Готов к записи").font(bold(22.0)).color(pal.text));
                            let t = if game.pid != 0 {
                                format!("Цель: {} - последняя активная игра", game.exe)
                            } else {
                                "Запустите игру и переключитесь на неё - AfterCore выберет её целью".into()
                            };
                            widgets::muted(ui, &pal, &t);
                            if let Some(h) = &hk {
                                widgets::muted(ui, &pal, &format!("Горячая клавиша прямо в игре: {h}"));
                            }
                        }
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if rec.is_some() {
                        if widgets::danger(ui, &pal, ico::STOP, "Остановить").clicked() {
                            self.core.bench_stop();
                        }
                    } else if widgets::primary(ui, &pal, ico::RECORD, "Начать запись").clicked() {
                        self.core.bench_start();
                    }
                });
            });
            ui.add_space(6.0);
            widgets::divider(ui, &pal);
            ui.add_space(4.0);
            widgets::row(ui, &pal, "Длительность", "Запись остановится сама", |ui| {
                widgets::segmented(
                    ui,
                    &pal,
                    &mut self.cfg.bench.duration_s,
                    &[(0, "Вручную"), (30, "30 с"), (60, "60 с"), (120, "2 мин"), (300, "5 мин")],
                );
            });
            widgets::toggle_row(ui, &pal, "Сохранять CSV", "Покадровый лог для анализа в Excel / CapFrameX", &mut self.cfg.bench.save_csv);
        });

        ui.add_space(4.0);
        let records = self.core.bench.read().clone();
        let mut delete: Option<usize> = None;
        widgets::section(ui, &pal, ico::BENCH, "Результаты", "Сохраняются в data\\benchmarks", |ui| {
            ui.horizontal(|ui| {
                if widgets::ghost(ui, &pal, ico::FOLDER, "Открыть папку").clicked() {
                    ac_core::process::shell_open(&ac_core::paths::bench_dir().to_string_lossy());
                }
            });
            if records.is_empty() {
                widgets::muted(ui, &pal, "Записей пока нет");
            }
            let max_fps = records.iter().map(|r| r.avg_fps).fold(1.0, f32::max);
            for (i, r) in records.iter().enumerate() {
                egui::Frame::new()
                    .fill(pal.surface2)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .corner_radius(14)
                    .inner_margin(Margin::symmetric(16, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(egui::RichText::new(r.exe.trim_end_matches(".exe")).font(bold(16.0)).color(pal.text));
                                ui.label(
                                    egui::RichText::new(format!("{} · {} · {:.0} с · {} кадров", r.when, r.api, r.duration_s, r.frames))
                                        .font(body(12.0))
                                        .color(pal.faint),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if widgets::icon_btn(ui, &pal, ico::DELETE, "Удалить").clicked() {
                                    delete = Some(i);
                                }
                                if !r.csv.is_empty() && widgets::icon_btn(ui, &pal, ico::FOLDER, "Открыть CSV").clicked() {
                                    ac_core::process::shell_open(&r.csv);
                                }
                            });
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            for (title, v, col) in [
                                ("AVG", format!("{:.0}", r.avg_fps), pal.accent),
                                ("1% LOW", format!("{:.0}", r.low1), pal.accent2),
                                ("0.1% LOW", format!("{:.0}", r.low01), pal.warn),
                                ("MIN", format!("{:.0}", r.min_fps), pal.muted),
                                ("MAX", format!("{:.0}", r.max_fps), pal.muted),
                                ("СТАТТЕРЫ", format!("{}", r.stutters), if r.stutters > 0 { pal.bad } else { pal.good }),
                            ] {
                                ui.vertical(|ui| {
                                    ui.set_width(84.0);
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label(egui::RichText::new(title).font(bold(10.5)).color(pal.faint));
                                    ui.label(egui::RichText::new(v).font(bold(22.0)).color(col));
                                });
                            }
                        });
                        // Bars: avg / 1% / 0.1% relative to best avg.
                        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
                        let p = ui.painter();
                        for (k, (v, col)) in [(r.avg_fps, pal.accent), (r.low1, pal.accent2), (r.low01, pal.warn)].iter().enumerate() {
                            let y = rect.top() + 2.0 + k as f32 * 11.0;
                            let bg = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 7.0));
                            p.rect_filled(bg, 4, alpha(*col, 26));
                            p.rect_filled(Rect::from_min_size(bg.min, vec2(bg.width() * (v / max_fps).clamp(0.0, 1.0), 7.0)), 4, *col);
                        }
                        let _ = Align2::LEFT_TOP;
                    });
            }
        });
        if let Some(i) = delete {
            self.core.delete_bench(i);
        }
    }
}
