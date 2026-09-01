//! Screen shown after login (or right away from the role-picker binary) so the
//! teacher can pick which class this lesson is for — every class has its own
//! roster and its own lesson history, never mixed with another's. Structurally
//! mirrors `auth::AuthScreen`: `update` renders one frame and returns
//! `Some((class_id, class_name))` once the teacher confirms one.

use eframe::egui;

use crate::theme;

use super::db;

pub struct ClassPickerScreen {
    classes: Vec<db::ClassRow>,
    selected_class_id: Option<i64>,
    new_class_name: String,
    error: Option<String>,
}

impl Default for ClassPickerScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassPickerScreen {
    pub fn new() -> Self {
        let classes = db::open()
            .ok()
            .and_then(|conn| db::list_classes(&conn).ok())
            .unwrap_or_default();
        let selected_class_id = classes.first().map(|c| c.id);
        Self {
            classes,
            selected_class_id,
            new_class_name: String::new(),
            error: None,
        }
    }

    pub fn update(&mut self, ctx: &egui::Context) -> Option<(i64, String)> {
        let mut result = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.heading(egui::RichText::new("Vocalis").size(36.0).color(theme::ACCENT));
                ui.label("Для какого класса этот урок?");
                ui.add_space(24.0);

                ui.group(|ui| {
                    ui.set_width(360.0);

                    if self.classes.is_empty() {
                        ui.colored_label(theme::MUTED, "Классов пока нет — создайте первый ниже.");
                    } else {
                        for class in &self.classes {
                            if ui.selectable_label(self.selected_class_id == Some(class.id), &class.name).clicked() {
                                self.selected_class_id = Some(class.id);
                            }
                        }
                        ui.add_space(10.0);
                        let selected_name = self
                            .selected_class_id
                            .and_then(|id| self.classes.iter().find(|c| c.id == id))
                            .map(|c| c.name.clone());
                        if let Some(name) = selected_name {
                            if ui.add_sized([320.0, 40.0], egui::Button::new("Начать урок")).clicked() {
                                result = Some((self.selected_class_id.unwrap(), name));
                            }
                        }
                    }

                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(10.0);
                    ui.label("Новый класс:");
                    ui.horizontal(|ui| {
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.new_class_name)
                                .desired_width(240.0)
                                .hint_text("например, 9А английский"),
                        );
                        let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.button("➕ Создать").clicked() || enter_pressed {
                            self.try_create_class();
                        }
                    });
                    if let Some(err) = &self.error {
                        ui.add_space(6.0);
                        ui.colored_label(theme::DANGER, err);
                    }
                });
            });
        });
        result
    }

    fn try_create_class(&mut self) {
        let name = self.new_class_name.trim().to_string();
        if name.is_empty() {
            self.error = Some("Введите название класса".to_string());
            return;
        }
        let conn = match db::open() {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("Не удалось открыть базу данных: {e}"));
                return;
            }
        };
        match db::insert_class(&conn, &name) {
            Ok(id) => {
                self.classes.push(db::ClassRow { id, name: name.clone() });
                self.selected_class_id = Some(id);
                self.new_class_name.clear();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Не удалось создать класс: {e}")),
        }
    }
}
