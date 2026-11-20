use ac_core::config::GameRule;
use ac_telemetry::etw::EtwState;
use eframe::egui::{self, vec2, Align, Layout, Margin, Sense, Stroke};

use super::frametime_chart;
use crate::core::{effective_overlay, LogLevel};
use crate::ui::theme::{alpha, bold, body, icon, mix, rgba};
use crate::ui::widgets::{self, ico};
use crate::ui::{App, Page};

impl App {
    pub fn page_dashboard(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        widgets::page_title(ui, &pal, "Обзор", "Живые показатели игры и системы - без внедрения в процессы");

        let game = self.core.last_game.read().clone();
        let ov = effective_overlay(&self.cfg, &self.core.effective_profile()).clone();
        let stats = (game.pid != 0).then(|| self.core.frames.stats(game.pid, ov.low_window_s as f64, 240)).flatten();
        let sensors = self.core.sensors.get();
        let etw = self.core.etw_state.read().clone();

        // ── Hero ──
        egui::Frame::new()
            .fill(pal.surface)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .corner_radius(20)
            .inner_margin(Margin::same(22))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let full = ui.max_rect();
                // Soft radial accent glow in the top-left corner, clipped to the card.
                if pal.glow {
                    let painter = ui.painter().with_clip_rect(full.expand(20.0));
                    let c = full.left_top() + vec2(90.0, 50.0);
                    let mut mesh = egui::Mesh::default();
                    mesh.colored_vertex(c, alpha(pal.accent, if pal.dark { 34 } else { 22 }));
                    let n = 48u32;
                    for k in 0..=n {
                        let a = k as f32 / n as f32 * std::f32::consts::TAU;
                        mesh.colored_vertex(c + vec2(a.cos() * 320.0, a.sin() * 200.0), alpha(pal.accent, 0));
                        if k > 0 {
                            mesh.add_triangle(0, k, k + 1);
                        }
                    }
                    painter.add(egui::Shape::mesh(mesh));
                }
                match (&stats, &etw) {
                    (Some(st), _) => {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(300.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(ico::GAME).font(icon(16.0)).color(pal.accent));
                                    ui.label(
                                        egui::RichText::new(game.exe.trim_end_matches(".exe")).font(bold(18.0)).color(pal.text),
                                    );
                                    if let Some(src) = st.source {
                                        widgets::pill(ui, &pal, src.label(), pal.accent);
                                    }
                                    if let Some(ac) = self.core.anticheat(game.pid) {
                                        widgets::pill(ui, &pal, &format!("{} · безопасный режим", ac.name), pal.good).on_hover_text(
                                            "Игра под защитой античита. AfterCore не читает её память, не прячет оверлей                                              от скриншотов и не меняет ядра/приоритет без вашего разрешения",
                                        );
                                    }
                                });
                                let col = rgba(crate::core::fps_color(&ov, st.fps));
                                let col = if ov.style.fps_colors { col } else { pal.accent };
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let fg = st.frame_generation();
                                    if fg {
                                        if let Some(r) = st.real_fps {
                                            ui.label(egui::RichText::new(format!("{r:.0}")).font(bold(40.0)).color(pal.muted))
                                                .on_hover_text("Настоящие кадры игры (без генератора)");
                                        }
                                    }
                                    let v = if st.fps > 0.0 { format!("{:.0}", st.fps) } else { "-".into() };
                                    ui.label(egui::RichText::new(v).font(bold(68.0)).color(col));
                                    ui.vertical(|ui| {
                                        ui.add_space(30.0);
                                        let unit = if fg { self.core.tech(game.pid).and_then(|t| t.fg).unwrap_or("FG") } else { "FPS" };
                                        ui.label(egui::RichText::new(unit).font(bold(16.0)).color(pal.muted));
                                    });
                                });
                                ui.horizontal(|ui| {
                                    mini_stat(ui, &pal, "1% low", &format!("{:.0}", st.low1));
                                    mini_stat(ui, &pal, "0.1% low", &format!("{:.0}", st.low01));
                                    mini_stat(ui, &pal, "Frametime", &format!("{:.1} ms", st.frametime_ms));
                                    mini_stat(ui, &pal, "Средний", &format!("{:.0}", st.avg_fps));
                                    mini_stat(ui, &pal, "Макс", &format!("{:.0}", st.max_fps));
                                });
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Профиль: {}  ·  pid {}{}",
                                        self.core.effective_profile(),
                                        game.pid,
                                        if st.presenting { "" } else { "  ·  кадров нет" }
                                    ))
                                    .font(body(12.5))
                                    .color(pal.faint),
                                );
                            });
                            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 210.0), Sense::hover());
                            frametime_chart(ui, &pal, rect, &st.graph, pal.accent);
                        });
                    }
                    (None, EtwState::NeedsAdmin) => {
                        empty_state(
                            ui,
                            &pal,
                            ico::SHIELD,
                            "Счётчику FPS нужны права администратора",
                            "Windows отдаёт события Present (ETW) только администраторам. Мониторинг железа уже работает",
                        );
                        ui.vertical_centered(|ui| {
                            if widgets::primary(ui, &pal, ico::SHIELD, "Перезапустить от администратора").clicked() {
                                crate::ui::restart_elevated(&self.core);
                            }
                        });
                    }
                    (None, EtwState::Failed(e)) => {
                        empty_state(ui, &pal, ico::WARN, "Счётчик FPS не запустился", e);
                    }
                    _ => {
                        empty_state(
                            ui,
                            &pal,
                            ico::GAME,
                            "Запустите игру",
                            "AfterCore подхватит её автоматически: FPS, frametime и 1% low появятся здесь и в оверлее",
                        );
                    }
                }
            });

        ui.add_space(6.0);

        // ── Tiles ──
        let g = &sensors.gpu;
        let cpu_hist: Vec<f32> = sensors.cpu_hist.iter().copied().collect();
        let gpu_hist: Vec<f32> = sensors.gpu_hist.iter().copied().collect();
        let ram_hist: Vec<f32> = sensors.ram_hist.iter().copied().collect();
        ui.columns(4, |c| {
            widgets::stat_tile(
                &mut c[0],
                &pal,
                ico::CPU,
                "Процессор",
                &sensors.cpu_load.map(|v| format!("{v:.0}")).unwrap_or("-".into()),
                "%",
                &[
                    sensors.cpu_clock_mhz.map(|v| format!("{:.2} GHz", v / 1000.0)),
                    sensors.cpu_power_w.map(|v| format!("{v:.0} W")),
                    sensors.cpu_temp.map(|v| format!("{v:.0}°C")),
                    sensors.cpu_max_core.map(|v| format!("max core {v:.0}%")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
                Some((&cpu_hist, 100.0)),
                pal.accent,
            );
            widgets::stat_tile(
                &mut c[1],
                &pal,
                ico::OVERLAY,
                "Видеокарта",
                &g.load.map(|v| format!("{v:.0}")).unwrap_or("-".into()),
                "%",
                &[g.clock_mhz.map(|v| format!("{v:.0} MHz")), g.power_w.map(|v| format!("{v:.0} W"))]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · "),
                Some((&gpu_hist, 100.0)),
                pal.accent2,
            );
            let tcol = match g.temp {
                Some(t) if t >= 85.0 => pal.bad,
                Some(t) if t >= 76.0 => pal.warn,
                _ => pal.good,
            };
            widgets::stat_tile(
                &mut c[2],
                &pal,
                ico::TEMP,
                "Температура GPU",
                &g.temp.map(|v| format!("{v:.0}")).unwrap_or("-".into()),
                "°C",
                &g.fan_pct.map(|f| format!("Вентилятор {f:.0}%")).unwrap_or_else(|| "NVML недоступен".into()),
                None,
                tcol,
            );
            widgets::stat_tile(
                &mut c[3],
                &pal,
                ico::MEMORY,
                "Память",
                &format!("{:.1}", sensors.ram_used_gb),
                &format!("/ {:.0} GB", sensors.ram_total_gb),
                &g.vram_used_gb
                    .map(|v| match g.vram_total_gb {
                        Some(t) => format!("VRAM {v:.1} / {t:.0} GB"),
                        None => format!("VRAM {v:.1} GB"),
                    })
                    .unwrap_or_default(),
                Some((&ram_hist, 100.0)),
                pal.good,
            );
        });

        ui.add_space(6.0);

        // ── Presenting apps + log ──
        let mut presenting: Vec<_> = self
            .core
            .frames
            .presenting()
            .into_iter()
            .filter(|(pid, _, _)| !self.cfg.is_blacklisted(&self.core.process_name(*pid).to_lowercase()))
            .collect();
        if self.dash_locked {
            // Keep the previous order; new apps go to the end.
            let pos = |pid: u32| self.dash_order.iter().position(|p| *p == pid).unwrap_or(usize::MAX);
            presenting.sort_by_key(|(pid, _, _)| pos(*pid));
        }
        self.dash_order = presenting.iter().map(|p| p.0).collect();
        ui.columns(2, |c| {
            widgets::section(&mut c[0], &pal, ico::LAYERS, "Активные 3D-приложения", "Всё, что сейчас выводит кадры", |ui| {
                ui.horizontal(|ui| {
                    let (ic, tip, txt) = if self.dash_locked {
                        (ico::LOCK, "Порядок закреплён - нажмите, чтобы сортировать по FPS", "Порядок закреплён")
                    } else {
                        (ico::UNLOCK, "Закрепить порядок, чтобы строки не прыгали", "Сортировка по FPS")
                    };
                    if widgets::icon_btn(ui, &pal, ic, tip).clicked() {
                        self.dash_locked = !self.dash_locked;
                    }
                    widgets::muted(ui, &pal, txt);
                });
                if presenting.is_empty() {
                    widgets::muted(ui, &pal, "Пока никто не рисует кадры");
                }
                for (pid, fps, src) in presenting.iter().take(8) {
                    let name = self.core.process_name(*pid);
                    egui::Frame::new()
                        .fill(pal.surface2)
                        .corner_radius(12)
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label(egui::RichText::new(&name).font(bold(14.0)).color(pal.text));
                                    ui.label(
                                        egui::RichText::new(format!("pid {pid} · {}", src.label())).font(body(12.0)).color(pal.faint),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if widgets::icon_btn(ui, &pal, ico::CPU, "Настроить ядра").clicked() {
                                        self.cores.pid = Some(*pid);
                                        self.page = Page::Cores;
                                    }
                                    let lower = name.to_lowercase();
                                    let has_rule = self.cfg.game_rules.iter().any(|r| r.exe == lower);
                                    if !has_rule && widgets::icon_btn(ui, &pal, ico::ADD, "Создать игровой профиль").clicked() {
                                        self.cfg.game_rules.push(GameRule { exe: lower.clone(), ..Default::default() });
                                        self.core.log(LogLevel::Ok, format!("Добавлено правило для {name}"));
                                        self.page = Page::Profiles;
                                    }
                                    ui.label(egui::RichText::new(format!("{fps:.0} FPS")).font(bold(15.0)).color(pal.accent));
                                });
                            });
                        });
                }
            });
            widgets::section(&mut c[1], &pal, ico::CLOCK, "Журнал", "События AfterCore", |ui| {
                let log = self.core.log.lock().clone();
                if log.is_empty() {
                    widgets::muted(ui, &pal, "Пусто");
                }
                for e in log.iter().take(9) {
                    ui.horizontal(|ui| {
                        let col = match e.level {
                            LogLevel::Ok => pal.good,
                            LogLevel::Warn => pal.warn,
                            LogLevel::Info => pal.accent,
                        };
                        let (r, _) = ui.allocate_exact_size(vec2(8.0, 18.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 3.5, col);
                        ui.label(egui::RichText::new(&e.time).font(body(12.0)).color(pal.faint));
                        ui.add(
                            egui::Label::new(egui::RichText::new(&e.text).font(body(13.0)).color(mix(pal.text, pal.muted, 0.2)))
                                .truncate(),
                        )
                        .on_hover_text(&e.text);
                    });
                }
            });
        });
    }
}

fn mini_stat(ui: &mut egui::Ui, pal: &crate::ui::theme::Pal, title: &str, value: &str) {
    egui::Frame::new().fill(pal.surface2).corner_radius(10).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(title).font(body(11.5)).color(pal.muted));
            ui.label(egui::RichText::new(value).font(bold(16.0)).color(pal.text));
        });
    });
}

fn empty_state(ui: &mut egui::Ui, pal: &crate::ui::theme::Pal, ic: &str, title: &str, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(14.0);
        let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
        ui.painter().rect_filled(r, 18, pal.accent_soft);
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, ic, icon(24.0), pal.accent);
        ui.add_space(4.0);
        ui.label(egui::RichText::new(title).font(bold(20.0)).color(pal.text));
        ui.label(egui::RichText::new(text).font(body(13.5)).color(pal.muted));
        ui.add_space(10.0);
    });
}
