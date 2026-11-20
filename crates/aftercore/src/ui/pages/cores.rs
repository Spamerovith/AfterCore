use std::time::{Duration, Instant};

use ac_core::config::{CoreSet, CpuMode, GameRule, Priority};
use ac_core::cpu::{self, PhysCore, Topology};
use eframe::egui::{self, pos2, vec2, Align, Align2, Layout, Margin, Rect, Sense, Stroke, StrokeKind};

use crate::core::LogLevel;
use crate::ui::theme::{alpha, bold, body, icon, mix, Pal};
use crate::ui::widgets::{self, ico};
use crate::ui::App;

fn same_set(a: &[u32], b: &[u32]) -> bool {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort();
    a.dedup();
    b.sort();
    b.dedup();
    a == b
}

impl App {
    pub fn page_cores(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let topo = self.core.topo.clone();
        widgets::page_title(ui, &pal, "Ядра CPU", "Выбор ядер, пресеты под архитектуру и свои наборы для игр");

        // ── Topology header ──
        widgets::card(ui, &pal, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                ui.painter().rect_filled(r, 14, pal.accent_soft);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, ico::CPU, icon(22.0), pal.accent);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(egui::RichText::new(&topo.name).font(bold(18.0)).color(pal.text));
                    let p = topo.cores.iter().filter(|c| topo.is_pcore(c)).count();
                    let e = topo.cores.len() - p;
                    let mut info = format!("{} ядер · {} потоков", topo.cores.len(), topo.logical_count());
                    if topo.hybrid {
                        info.push_str(&format!(" · гибридная архитектура: {p} P + {e} E"));
                    }
                    if topo.llc_count > 1 {
                        info.push_str(&format!(" · {} группы L3 (CCD)", topo.llc_count));
                    }
                    ui.label(egui::RichText::new(info).font(body(13.0)).color(pal.muted));
                });
            });
        });
        ui.add_space(4.0);

        // ── Selection ──
        let per_cpu = self.core.sensors.get().per_cpu;
        let presets = topo.presets();
        let mut delete_set: Option<String> = None;
        widgets::section(ui, &pal, ico::CHECK, "Выбор ядер", "Нажмите на поток, чтобы выбрать его; на заголовок - чтобы выбрать всё ядро", |ui| {
            ui.horizontal(|ui| {
                if widgets::primary(ui, &pal, ico::CHECK, "Выбрать все").clicked() {
                    self.cores.sel = topo.all();
                }
                if widgets::ghost(ui, &pal, "", "Снять все").clicked() {
                    self.cores.sel.clear();
                }
                if widgets::ghost(ui, &pal, ico::REFRESH, "Инвертировать").clicked() {
                    self.cores.sel = topo.all().into_iter().filter(|c| !self.cores.sel.contains(c)).collect();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("Выбрано {} из {}", self.cores.sel.len(), topo.logical_count()))
                            .font(bold(14.0))
                            .color(pal.accent),
                    );
                });
            });

            ui.label(egui::RichText::new("Пресеты").font(bold(13.0)).color(pal.muted));
            ui.horizontal_wrapped(|ui| {
                for p in presets.iter() {
                    let sel = same_set(&p.cpus, &self.cores.sel);
                    if widgets::chip(ui, &pal, &format!("{}  ·  {}", p.title, p.cpus.len()), sel).on_hover_text(&p.hint).clicked() {
                        self.cores.sel = p.cpus.clone();
                    }
                }
            });
            if !self.cfg.core_sets.is_empty() {
                ui.label(egui::RichText::new("Мои наборы").font(bold(13.0)).color(pal.muted));
                ui.horizontal_wrapped(|ui| {
                    for s in self.cfg.core_sets.iter() {
                        let sel = same_set(&s.cpus, &self.cores.sel);
                        if widgets::chip(ui, &pal, &format!("★ {}  ·  {}", s.name, s.cpus.len()), sel).clicked() {
                            self.cores.sel = s.cpus.clone();
                            self.cores.set_name = s.name.clone();
                        }
                        if widgets::icon_btn(ui, &pal, ico::DELETE, "Удалить набор").clicked() {
                            delete_set = Some(s.name.clone());
                        }
                    }
                });
            }
            ui.add_space(4.0);

            // Core tiles grouped by type.
            let groups: Vec<(&str, Vec<&PhysCore>)> = if topo.hybrid {
                vec![
                    ("P-ядра · производительные", topo.cores.iter().filter(|c| topo.is_pcore(c)).collect()),
                    ("E-ядра · энергоэффективные", topo.cores.iter().filter(|c| !topo.is_pcore(c)).collect()),
                ]
            } else {
                vec![("Ядра", topo.cores.iter().collect())]
            };
            for (title, cores) in groups {
                ui.label(egui::RichText::new(title).font(bold(13.0)).color(pal.muted));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for (k, core) in cores.iter().enumerate() {
                        let prefix = if !topo.hybrid { "C" } else if topo.is_pcore(core) { "P" } else { "E" };
                        core_tile(ui, &pal, &format!("{prefix}{k}"), core, &mut self.cores.sel, &per_cpu);
                    }
                });
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                widgets::input(ui, &mut self.cores.set_name, "Название набора, например «Стрим»", 260.0);
                if widgets::ghost(ui, &pal, ico::SAVE, "Сохранить набор").clicked() {
                    if self.cores.sel.is_empty() {
                        self.toast("Выберите хотя бы одно ядро", pal.warn);
                    } else {
                        let name = if self.cores.set_name.trim().is_empty() {
                            self.cfg.unique_set_name("Набор")
                        } else {
                            self.cores.set_name.trim().to_string()
                        };
                        let mut cpus = self.cores.sel.clone();
                        cpus.sort();
                        match self.cfg.core_sets.iter_mut().find(|s| s.name == name) {
                            Some(s) => s.cpus = cpus,
                            None => self.cfg.core_sets.push(CoreSet { name: name.clone(), cpus }),
                        }
                        self.toast(format!("Набор «{name}» сохранён"), pal.good);
                    }
                }
            });
        });
        if let Some(n) = delete_set {
            self.cfg.core_sets.retain(|s| s.name != n);
        }
        ui.add_space(4.0);

        self.cores_apply(ui, &topo);
    }

    fn cores_apply(&mut self, ui: &mut egui::Ui, topo: &Topology) {
        let pal = self.pal;
        if self.cores.procs_at.map(|t| t.elapsed() > Duration::from_secs(3)).unwrap_or(true) {
            let mut list = ac_core::process::list_processes();
            list.retain(|p| p.pid > 4);
            list.sort_by_key(|p| p.name.to_lowercase());
            self.cores.procs = list;
            self.cores.procs_at = Some(Instant::now());
        }
        let presenting: Vec<(u32, f32)> = self.core.frames.presenting().into_iter().map(|(p, f, _)| (p, f)).collect();

        widgets::section(ui, &pal, ico::BOLT, "Применить к процессу", "Сразу к запущенной игре или как постоянное правило", |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() * 0.5).max(300.0));
                    widgets::search(ui, &pal, &mut self.cores.filter, "Найти процесс…", 300.0);
                    egui::Frame::new().fill(pal.surface2).corner_radius(10).inner_margin(Margin::same(6)).show(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("procs").max_height(230.0).auto_shrink([false, true]).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let f = self.cores.filter.to_lowercase();
                            let mut items: Vec<(u32, String, Option<f32>)> = self
                                .cores
                                .procs
                                .iter()
                                .filter(|p| f.is_empty() || p.name.to_lowercase().contains(&f) || p.pid.to_string() == f)
                                .map(|p| (p.pid, p.name.clone(), presenting.iter().find(|x| x.0 == p.pid).map(|x| x.1)))
                                .collect();
                            items.sort_by_key(|x| x.2.is_none());
                            for (pid, name, fps) in items.into_iter().take(300) {
                                let sel = self.cores.pid == Some(pid);
                                let text = match fps {
                                    Some(f) => format!("{name}   ·   {pid}   ·   {f:.0} FPS"),
                                    None => format!("{name}   ·   {pid}"),
                                };
                                let mut rt = egui::RichText::new(text).font(body(13.5));
                                if fps.is_some() {
                                    rt = rt.color(pal.accent);
                                }
                                if ui.selectable_label(sel, rt).clicked() {
                                    self.cores.pid = Some(pid);
                                    // Reading affinity opens the process: never for protected games.
                                    let protected = self.core.tech_for(pid, &[]).anticheat.is_some();
                                    if !protected {
                                        if let Some(mask) = cpu::get_affinity(pid) {
                                            self.cores.sel = Topology::cpus_of_mask(mask);
                                        }
                                    }
                                }
                            }
                        });
                    });
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    let pid = self.cores.pid;
                    let name = pid.map(|p| self.core.process_name(p)).unwrap_or_default();
                    let ac = pid.and_then(|p| self.core.tech_for(p, &[]).anticheat);
                    // Protected game and the user has not allowed touching it: no handle at all.
                    let locked = ac.is_some() && !self.cfg.general.tweak_anticheat_games;
                    match pid {
                        Some(p) => {
                            ui.label(egui::RichText::new(&name).font(bold(17.0)).color(pal.text));
                            let now = if ac.is_some() {
                                "процесс не открывается (античит)".to_string()
                            } else {
                                cpu::get_affinity(p)
                                    .map(|m| format!("Сейчас разрешено потоков: {} из {}", Topology::cpus_of_mask(m).len(), topo.logical_count()))
                                    .unwrap_or_else(|| "Нет доступа к процессу (нужны права администратора?)".into())
                            };
                            widgets::muted(ui, &pal, &format!("pid {p} · {now}"));
                            if let Some(ac) = ac {
                                let text = if locked {
                                    format!(
                                        "Под защитой {}. Чтобы античит не видел чужого доступа к игре, AfterCore не меняет ей ядра и приоритет. \
                                         Разрешить можно в Настройках → Античиты (на свой риск)",
                                        ac.name
                                    )
                                } else {
                                    format!("Под защитой {}: смена ядер разрешена в Настройках - античит может её заблокировать или записать", ac.name)
                                };
                                ui.label(egui::RichText::new(text).font(body(12.5)).color(pal.warn));
                            }
                        }
                        None => {
                            ui.label(egui::RichText::new("Процесс не выбран").font(bold(17.0)).color(pal.muted));
                            widgets::muted(ui, &pal, "Выберите процесс слева - игры с FPS показаны первыми");
                        }
                    }
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Режим").font(bold(13.0)).color(pal.muted));
                    widgets::segmented(
                        ui,
                        &pal,
                        &mut self.cores.mode,
                        &[(CpuMode::Affinity, "Жёсткий (Affinity)"), (CpuMode::CpuSets, "Мягкий (CPU Sets)")],
                    );
                    widgets::muted(
                        ui,
                        &pal,
                        match self.cores.mode {
                            CpuMode::Affinity => "Потоки игры работают только на выбранных ядрах",
                            CpuMode::CpuSets => "Планировщик Windows предпочитает выбранные ядра, но может использовать и другие",
                        },
                    );
                    ui.label(egui::RichText::new("Приоритет").font(bold(13.0)).color(pal.muted));
                    egui::ComboBox::from_id_salt("prio")
                        .selected_text(self.cores.priority.map(|p| p.title()).unwrap_or("Не менять"))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.cores.priority, None, "Не менять");
                            for p in Priority::ALL {
                                ui.selectable_value(&mut self.cores.priority, Some(p), p.title());
                            }
                        });
                    ui.add_space(8.0);
                    ui.add_enabled_ui(pid.is_some() && !locked, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if widgets::primary(ui, &pal, ico::BOLT, "Применить").clicked() {
                                let p = pid.unwrap();
                                let mut msgs = Vec::new();
                                match cpu::apply_cores(p, &self.cores.sel, self.cores.mode, topo) {
                                    Ok(()) => msgs.push(format!("ядер: {}", self.cores.sel.len())),
                                    Err(e) => {
                                        self.toast(format!("Ошибка: {e}"), pal.bad);
                                    }
                                }
                                if let Some(pr) = self.cores.priority {
                                    match cpu::set_priority(p, pr) {
                                        Ok(()) => msgs.push(format!("приоритет: {}", pr.title())),
                                        Err(e) => self.toast(format!("Приоритет: {e}"), pal.bad),
                                    }
                                }
                                if !msgs.is_empty() {
                                    let t = format!("{name}: {}", msgs.join(", "));
                                    self.core.log(LogLevel::Ok, t.clone());
                                    self.toast(t, pal.good);
                                }
                            }
                            if widgets::ghost(ui, &pal, ico::REFRESH, "Сбросить").on_hover_text("Все ядра, без CPU Sets").clicked() {
                                match cpu::reset_cores(pid.unwrap(), topo) {
                                    Ok(()) => {
                                        self.cores.sel = topo.all();
                                        self.toast(format!("{name}: ограничения ядер сняты"), pal.good);
                                    }
                                    Err(e) => self.toast(format!("Ошибка: {e}"), pal.bad),
                                }
                            }
                            if widgets::ghost(ui, &pal, ico::SAVE, "Запоминать для этой игры").clicked() {
                                self.save_rule_from_selection(&name, topo);
                            }
                        });
                    });
                });
            });
        });
    }

    fn save_rule_from_selection(&mut self, exe: &str, topo: &Topology) {
        let pal = self.pal;
        if self.cores.sel.is_empty() {
            self.toast("Выберите хотя бы одно ядро", pal.warn);
            return;
        }
        let exe_l = exe.to_lowercase();
        // Reuse a preset / saved set if it matches, else save a new set.
        let reference = if let Some(p) = topo.presets().into_iter().find(|p| same_set(&p.cpus, &self.cores.sel)) {
            format!("builtin:{}", p.id)
        } else if let Some(s) = self.cfg.core_sets.iter().find(|s| same_set(&s.cpus, &self.cores.sel)) {
            s.name.clone()
        } else {
            let name = self.cfg.unique_set_name(exe.trim_end_matches(".exe"));
            let mut cpus = self.cores.sel.clone();
            cpus.sort();
            self.cfg.core_sets.push(CoreSet { name: name.clone(), cpus });
            name
        };
        let mode = self.cores.mode;
        let prio = self.cores.priority;
        match self.cfg.game_rules.iter_mut().find(|r| r.exe == exe_l) {
            Some(r) => {
                r.core_set = reference.clone();
                r.cpu_mode = mode;
                if prio.is_some() {
                    r.priority = prio;
                }
            }
            None => self.cfg.game_rules.push(GameRule {
                exe: exe_l,
                core_set: reference.clone(),
                cpu_mode: mode,
                priority: prio,
                ..Default::default()
            }),
        }
        self.toast(format!("{exe}: при каждом запуске - «{}»", topo.set_label(&reference)), pal.good);
    }
}

fn core_tile(ui: &mut egui::Ui, pal: &Pal, title: &str, core: &PhysCore, sel: &mut Vec<u32>, load: &[f32]) {
    let cell = vec2(54.0, 50.0);
    let pad = 8.0;
    let n = core.threads.len() as f32;
    let size = vec2(pad * 2.0 + cell.x * n + 6.0 * (n - 1.0), 26.0 + cell.y + pad);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let all_on = core.threads.iter().all(|t| sel.contains(t));
    let any_on = core.threads.iter().any(|t| sel.contains(t));
    let p = ui.painter();
    p.rect_filled(rect, 14, if any_on { mix(pal.surface2, pal.accent, 0.08) } else { pal.surface2 });
    p.rect_stroke(rect, 14, Stroke::new(1.0_f32, if all_on { alpha(pal.accent, 160) } else { pal.border }), StrokeKind::Inside);

    // Header toggles the whole core.
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), 24.0));
    let hresp = ui.interact(head, ui.id().with(("core", core.threads[0])), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let hcol = if hresp.hovered() { pal.accent } else { pal.muted };
    ui.painter().text(pos2(rect.left() + pad + 2.0, rect.top() + 13.0), Align2::LEFT_CENTER, title, bold(12.5), hcol);
    if core.threads.len() > 1 {
        ui.painter().text(pos2(rect.right() - pad - 2.0, rect.top() + 13.0), Align2::RIGHT_CENTER, "HT", body(10.5), pal.faint);
    }
    if hresp.clicked() {
        if all_on {
            sel.retain(|c| !core.threads.contains(c));
        } else {
            for t in &core.threads {
                if !sel.contains(t) {
                    sel.push(*t);
                }
            }
        }
    }

    for (i, t) in core.threads.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + pad + i as f32 * (cell.x + 6.0), rect.top() + 24.0), cell);
        let resp = ui.interact(r, ui.id().with(("thr", *t)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let on = sel.contains(t);
        let a = ui.ctx().animate_bool_with_time(resp.id, on, 0.12);
        let pnt = ui.painter();
        let fill = mix(pal.surface3, pal.accent, a);
        pnt.rect_filled(r, 10, fill);
        if resp.hovered() {
            pnt.rect_stroke(r, 10, Stroke::new(1.5_f32, alpha(pal.accent, 200)), StrokeKind::Inside);
        }
        // Live load bar.
        let l = load.get(*t as usize).copied().unwrap_or(0.0) / 100.0;
        let bar = Rect::from_min_max(pos2(r.left() + 6.0, r.bottom() - 7.0), pos2(r.right() - 6.0, r.bottom() - 4.0));
        pnt.rect_filled(bar, 2, alpha(if on { pal.on_accent } else { pal.muted }, 50));
        let fillw = bar.width() * l.clamp(0.0, 1.0);
        pnt.rect_filled(Rect::from_min_size(bar.min, vec2(fillw, bar.height())), 2, if on { pal.on_accent } else { pal.accent });
        let fg = mix(pal.text, pal.on_accent, a);
        pnt.text(pos2(r.center().x, r.top() + 15.0), Align2::CENTER_CENTER, format!("#{t}"), bold(13.0), fg);
        pnt.text(pos2(r.center().x, r.top() + 31.0), Align2::CENTER_CENTER, format!("{:.0}%", l * 100.0), body(10.5), alpha(fg, 190));
        if resp.clicked() {
            if on {
                sel.retain(|c| c != t);
            } else {
                sel.push(*t);
            }
        }
    }
}
