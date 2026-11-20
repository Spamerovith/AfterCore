use ac_core::config::{CpuMode, GameRule, Priority, Profile, Tri};
use eframe::egui::{self, vec2, Align, Layout, Margin, Stroke};

use crate::ui::theme::{alpha, bold, body};
use crate::ui::widgets::{self, ico};
use crate::ui::App;

impl App {
    pub fn page_profiles(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        widgets::page_title(ui, &pal, "Профили", "Наборы настроек оверлея и правила для отдельных игр");

        // ── Setting profiles ──
        let mut action: Option<(&'static str, usize)> = None;
        widgets::section(ui, &pal, ico::PROFILES, "Профили настроек", "Переключаются в один клик или горячей клавишей", |ui| {
            let n = self.cfg.profiles.len();
            for i in 0..n {
                let p = self.cfg.profiles[i].clone();
                let active = p.name == self.cfg.active_profile;
                egui::Frame::new()
                    .fill(if active { alpha(pal.accent, 18) } else { pal.surface2 })
                    .stroke(Stroke::new(1.0_f32, if active { alpha(pal.accent, 130) } else { pal.border }))
                    .corner_radius(14)
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let renaming = matches!(&self.prof.rename, Some((k, _)) if *k == i);
                            if renaming {
                                let mut done = false;
                                if let Some((_, buf)) = self.prof.rename.as_mut() {
                                    let r = widgets::input(ui, buf, "Название", 220.0);
                                    r.request_focus();
                                    done = r.lost_focus();
                                }
                                if done || widgets::icon_btn(ui, &pal, ico::CHECK, "Готово").clicked() {
                                    action = Some(("rename_commit", i));
                                }
                            } else {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(&p.name).font(bold(16.0)).color(pal.text));
                                        if active {
                                            widgets::pill(ui, &pal, "активный", pal.good);
                                        }
                                    });
                                    let metrics = p.overlay.items.iter().filter(|m| m.enabled).count();
                                    let uses = self.cfg.game_rules.iter().filter(|r| r.profile == p.name).count();
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} · {} метрик · {}{}",
                                            p.overlay.layout.title(),
                                            metrics,
                                            p.overlay.anchor.title().to_lowercase(),
                                            if uses > 0 { format!(" · игр: {uses}") } else { String::new() }
                                        ))
                                        .font(body(12.5))
                                        .color(pal.muted),
                                    );
                                });
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if n > 1 && widgets::icon_btn(ui, &pal, ico::DELETE, "Удалить").clicked() {
                                    action = Some(("delete", i));
                                }
                                if widgets::icon_btn(ui, &pal, ico::EXPORT, "Экспорт в файл").clicked() {
                                    action = Some(("export", i));
                                }
                                if widgets::icon_btn(ui, &pal, ico::COPY, "Дублировать").clicked() {
                                    action = Some(("dup", i));
                                }
                                if widgets::icon_btn(ui, &pal, ico::EDIT, "Переименовать").clicked() {
                                    action = Some(("rename", i));
                                }
                                if widgets::icon_btn(ui, &pal, ico::OVERLAY, "Настроить оверлей").clicked() {
                                    action = Some(("edit", i));
                                }
                                if !active && widgets::ghost(ui, &pal, ico::CHECK, "Активировать").clicked() {
                                    action = Some(("activate", i));
                                }
                            });
                        });
                    });
            }
            ui.horizontal(|ui| {
                if widgets::primary(ui, &pal, ico::ADD, "Новый профиль").clicked() {
                    action = Some(("new", 0));
                }
                ui.menu_button(egui::RichText::new("Импорт профиля…").font(body(14.0)), |ui| {
                    self.prof.imports = list_json(&ac_core::paths::exports_dir());
                    if self.prof.imports.is_empty() {
                        ui.label("В папке data\\exports нет файлов .json");
                    }
                    for f in self.prof.imports.clone() {
                        let name = f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.button(name).clicked() {
                            self.import_profile(&f);
                        }
                    }
                    ui.separator();
                    if ui.button("Открыть папку экспорта").clicked() {
                        ac_core::process::shell_open(&ac_core::paths::exports_dir().to_string_lossy());
                    }
                });
            });
        });

        if let Some((what, i)) = action {
            self.profile_action(what, i);
        }
        ui.add_space(4.0);
        self.game_rules(ui);
    }

    fn import_profile(&mut self, path: &std::path::Path) {
        let pal = self.pal;
        match std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Profile>(&b).ok()) {
            Some(mut p) => {
                p.name = self.cfg.unique_profile_name(&p.name);
                p.overlay.normalize();
                self.toast(format!("Профиль «{}» импортирован", p.name), pal.good);
                self.cfg.profiles.push(p);
            }
            None => self.toast("Файл не похож на профиль AfterCore", pal.bad),
        }
    }

    fn profile_action(&mut self, what: &str, i: usize) {
        let pal = self.pal;
        match what {
            "activate" => self.cfg.active_profile = self.cfg.profiles[i].name.clone(),
            "edit" => {
                self.ov.edit_profile = self.cfg.profiles[i].name.clone();
                self.page = crate::ui::Page::Overlay;
            }
            "rename" => self.prof.rename = Some((i, self.cfg.profiles[i].name.clone())),
            "rename_commit" => {
                if let Some((k, new)) = self.prof.rename.take() {
                    let new = new.trim().to_string();
                    let old = self.cfg.profiles[k].name.clone();
                    if !new.is_empty() && new != old {
                        let new = self.cfg.unique_profile_name(&new);
                        self.cfg.profiles[k].name = new.clone();
                        if self.cfg.active_profile == old {
                            self.cfg.active_profile = new.clone();
                        }
                        if self.ov.edit_profile == old {
                            self.ov.edit_profile = new.clone();
                        }
                        for r in self.cfg.game_rules.iter_mut().filter(|r| r.profile == old) {
                            r.profile = new.clone();
                        }
                    }
                }
            }
            "dup" => {
                let mut p = self.cfg.profiles[i].clone();
                p.name = self.cfg.unique_profile_name(&format!("{} копия", p.name));
                self.cfg.profiles.insert(i + 1, p);
            }
            "delete" => {
                let name = self.cfg.profiles.remove(i).name;
                for r in self.cfg.game_rules.iter_mut().filter(|r| r.profile == name) {
                    r.profile.clear();
                }
                self.cfg.normalize();
                self.toast(format!("Профиль «{name}» удалён"), pal.muted);
            }
            "export" => {
                let p = &self.cfg.profiles[i];
                let (ts, _) = ac_core::win::local_time_strings();
                let path = ac_core::paths::exports_dir().join(format!("profile_{}_{ts}.json", sanitize(&p.name)));
                match serde_json::to_vec_pretty(p).map(|j| std::fs::write(&path, j)) {
                    Ok(Ok(())) => {
                        self.toast("Профиль экспортирован в data\\exports", pal.good);
                        ac_core::process::shell_open(&ac_core::paths::exports_dir().to_string_lossy());
                    }
                    _ => self.toast("Не удалось сохранить файл", pal.bad),
                }
            }
            "new" => {
                let name = self.cfg.unique_profile_name("Новый профиль");
                self.cfg.profiles.push(Profile { name: name.clone(), overlay: Default::default() });
                self.prof.rename = Some((self.cfg.profiles.len() - 1, name));
            }
            _ => {}
        }
    }

    fn game_rules(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let topo = self.core.topo.clone();
        let presets = topo.presets();
        let profile_names: Vec<String> = self.cfg.profiles.iter().map(|p| p.name.clone()).collect();
        let set_names: Vec<String> = self.cfg.core_sets.iter().map(|s| s.name.clone()).collect();
        let mut remove: Option<usize> = None;
        let mut apply_now: Option<usize> = None;

        widgets::section(
            ui,
            &pal,
            ico::GAME,
            "Игровые профили",
            "Правила по имени exe: свой профиль оверлея, ядра и приоритет - применяются автоматически",
            |ui| {
                // Add rule.
                ui.horizontal(|ui| {
                    widgets::input(ui, &mut self.prof.new_exe, "game.exe", 220.0);
                    if widgets::primary(ui, &pal, ico::ADD, "Добавить").clicked() {
                        let exe = self.prof.new_exe.trim().to_lowercase();
                        let exe = if exe.is_empty() || exe.ends_with(".exe") { exe } else { format!("{exe}.exe") };
                        if !exe.is_empty() && !self.cfg.game_rules.iter().any(|r| r.exe == exe) {
                            self.cfg.game_rules.push(GameRule { exe, ..Default::default() });
                            self.prof.new_exe.clear();
                        }
                    }
                    let presenting = self.core.frames.presenting();
                    if !presenting.is_empty() {
                        ui.label(egui::RichText::new("или из запущенных:").font(body(12.5)).color(pal.muted));
                        for (pid, _, _) in presenting.iter().take(4) {
                            let name = self.core.process_name(*pid);
                            let lower = name.to_lowercase();
                            if !self.cfg.game_rules.iter().any(|r| r.exe == lower) && widgets::chip(ui, &pal, &name, false).clicked() {
                                self.cfg.game_rules.push(GameRule { exe: lower, ..Default::default() });
                            }
                        }
                    }
                });
                if self.cfg.game_rules.is_empty() {
                    widgets::muted(ui, &pal, "Правил пока нет. Добавьте игру по имени exe или выберите запущенную");
                }
                for (i, r) in self.cfg.game_rules.iter_mut().enumerate() {
                    egui::Frame::new()
                        .fill(pal.surface2)
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .corner_radius(14)
                        .inner_margin(Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                widgets::toggle(ui, &pal, &mut r.enabled);
                                ui.label(egui::RichText::new(&r.exe).font(bold(15.0)).color(if r.enabled { pal.text } else { pal.muted }));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if widgets::icon_btn(ui, &pal, ico::DELETE, "Удалить правило").clicked() {
                                        remove = Some(i);
                                    }
                                    if widgets::icon_btn(ui, &pal, ico::BOLT, "Применить к запущенной игре сейчас").clicked() {
                                        apply_now = Some(i);
                                    }
                                });
                            });
                            ui.add_space(2.0);
                            egui::Grid::new(("rule", i)).num_columns(5).spacing(vec2(14.0, 6.0)).show(ui, |ui| {
                                for t in ["Профиль", "Оверлей", "Ядра", "Режим ядер", "Приоритет"] {
                                    ui.label(egui::RichText::new(t).font(body(12.0)).color(pal.muted));
                                }
                                ui.end_row();
                                let w = 150.0;
                                egui::ComboBox::from_id_salt(("rp", i))
                                    .width(w)
                                    .selected_text(if r.profile.is_empty() { "Активный".to_string() } else { r.profile.clone() })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut r.profile, String::new(), "Активный");
                                        for n in &profile_names {
                                            ui.selectable_value(&mut r.profile, n.clone(), n);
                                        }
                                    });
                                egui::ComboBox::from_id_salt(("ro", i))
                                    .width(w)
                                    .selected_text(match r.overlay {
                                        Tri::Inherit => "Как в профиле",
                                        Tri::On => "Показывать",
                                        Tri::Off => "Скрывать",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut r.overlay, Tri::Inherit, "Как в профиле");
                                        ui.selectable_value(&mut r.overlay, Tri::On, "Показывать");
                                        ui.selectable_value(&mut r.overlay, Tri::Off, "Скрывать");
                                    });
                                egui::ComboBox::from_id_salt(("rc", i))
                                    .width(w)
                                    .selected_text(topo.set_label(&r.core_set))
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut r.core_set, String::new(), "Не менять");
                                        for p in presets.iter() {
                                            ui.selectable_value(&mut r.core_set, format!("builtin:{}", p.id), &p.title);
                                        }
                                        for s in &set_names {
                                            ui.selectable_value(&mut r.core_set, s.clone(), format!("★ {s}"));
                                        }
                                    });
                                ui.add_enabled_ui(!r.core_set.is_empty(), |ui| {
                                    egui::ComboBox::from_id_salt(("rm", i))
                                        .width(w)
                                        .selected_text(match r.cpu_mode {
                                            CpuMode::Affinity => "Affinity",
                                            CpuMode::CpuSets => "CPU Sets",
                                        })
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut r.cpu_mode, CpuMode::Affinity, CpuMode::Affinity.title());
                                            ui.selectable_value(&mut r.cpu_mode, CpuMode::CpuSets, CpuMode::CpuSets.title());
                                        });
                                });
                                egui::ComboBox::from_id_salt(("rpr", i))
                                    .width(w)
                                    .selected_text(r.priority.map(|p| p.title()).unwrap_or("Не менять"))
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut r.priority, None, "Не менять");
                                        for p in Priority::ALL {
                                            ui.selectable_value(&mut r.priority, Some(p), p.title());
                                        }
                                    });
                                ui.end_row();
                            });
                        });
                }
            },
        );
        if let Some(i) = apply_now {
            let rule = self.cfg.game_rules[i].clone();
            let pids: Vec<u32> =
                ac_core::process::list_processes().into_iter().filter(|p| p.name.to_lowercase() == rule.exe).map(|p| p.pid).collect();
            if pids.is_empty() {
                self.toast(format!("{} не запущен - правило сработает при запуске", rule.exe), pal.muted);
            } else {
                *self.core.cfg.write() = self.cfg.clone();
                for pid in pids {
                    let done = self.core.apply_rule_to(pid, &rule);
                    let t = if done.is_empty() { "нечего применять".to_string() } else { done.join(", ") };
                    self.toast(format!("{}: {t}", rule.exe), pal.good);
                }
            }
        }
        if let Some(i) = remove {
            self.cfg.game_rules.remove(i);
        }
    }
}

fn list_json(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect())
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

pub fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}
