//! Short first-run walkthrough for the teacher console: shown once, right after
//! profile creation/login (see `TeacherEntry` in `lib.rs`), before the class
//! picker. Re-openable any time via "Показать введение ещё раз" in Settings —
//! same struct, just constructed fresh and shown as an overlay over the console
//! instead of as its own `TeacherEntry` state (see `TeacherApp::onboarding`).
//!
//! Deliberately short (five steps): only what a new teacher needs in the first
//! five minutes, not a full feature tour. Structurally mirrors
//! `auth::AuthScreen`/`class_picker::ClassPickerScreen` — `update` renders one
//! frame and returns `true` once the teacher has stepped through (or skipped) it.

use eframe::egui;

use crate::theme;

struct Step {
    title: &'static str,
    body: &'static str,
}

const STEPS: &[Step] = &[
    Step {
        title: "Добро пожаловать в Vocalis",
        body: "Vocalis — лингафонный кабинет: вы ведёте урок с одного экрана, слышите и \
               говорите с учениками, показываете материалы и задания, видите, кто активен. \
               Этот короткий обзор — 5 шагов — покажет, с чего начать.",
    },
    Step {
        title: "Начните урок и подключите учеников",
        body: "После выбора класса открывается консоль урока — в шапке сразу виден PIN-код \
               урока. Сообщите его ученикам: в клиенте они вводят имя и этот PIN, чтобы \
               подключиться. Список подключённых появится в основной сетке.",
    },
    Step {
        title: "Класс и список учеников",
        body: "Вкладка «Список класса» — это ростер: заранее внесённые имена. Когда ученик \
               подключается, его имя мягко сопоставляется со списком (даже если он опечатался \
               или ввёл имя чуть иначе), а не создаёт нового ученика на каждый урок.",
    },
    Step {
        title: "Вкладки консоли",
        body: "«Материалы» — аудио/видео для проигрывания классу. «Задания» — тесты, \
               аудирование, чтение с автопроверкой. «Список класса» — ростер и его редактирование. \
               «Журнал» — история подключений и событий урока. «Настройки» — устройства, \
               тема оформления, язык — и это самое введение, если понадобится снова.",
    },
    Step {
        title: "Демонстрация экрана и интерком",
        body: "Кнопка демонстрации экрана транслирует классу либо ваш экран, либо экран \
               выбранного ученика. Интерком — приватный разговор с одним учеником, не мешая \
               остальному классу: выберите ученика и нажмите «Поговорить».",
    },
];

/// The onboarding screen/overlay itself — see the module doc comment.
pub struct Onboarding {
    step: usize,
}

impl Default for Onboarding {
    fn default() -> Self {
        Self::new()
    }
}

impl Onboarding {
    pub fn new() -> Self {
        Self { step: 0 }
    }

    /// Renders one frame; returns `true` once the teacher has clicked through
    /// to the end ("Готово") or hit "Пропустить" — either way, onboarding is
    /// considered done for this showing.
    pub fn update(&mut self, ctx: &egui::Context) -> bool {
        let mut done = false;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                let current = &STEPS[self.step];
                ui.heading(egui::RichText::new(current.title).size(28.0).color(theme::accent()));
                ui.add_space(18.0);

                ui.group(|ui| {
                    ui.set_width(480.0);
                    ui.label(current.body);
                });

                ui.add_space(14.0);
                ui.colored_label(theme::muted(), format!("Шаг {} из {}", self.step + 1, STEPS.len()));
                ui.add_space(18.0);

                // Plain `ui.horizontal` expands to the panel's full width, which defeats
                // `vertical_centered` (it only centers a child by its *reported* width) —
                // pin this row to its actual content width first, same trick the text
                // group above uses via its own `set_width`.
                egui::Frame::none().show(ui, |ui| {
                    ui.set_width(340.0);
                    ui.horizontal(|ui| {
                        if self.step > 0 && ui.add_sized([90.0, 36.0], egui::Button::new("⬅ Назад")).clicked() {
                            self.step -= 1;
                        }
                        if ui.add_sized([110.0, 36.0], egui::Button::new("Пропустить")).clicked() {
                            done = true;
                        }
                        if self.step + 1 < STEPS.len() {
                            if ui.add_sized([120.0, 36.0], egui::Button::new("Далее ➡")).clicked() {
                                self.step += 1;
                            }
                        } else if ui.add_sized([120.0, 36.0], egui::Button::new("Готово ✔")).clicked() {
                            done = true;
                        }
                    });
                });
            });
        });
        done
    }
}
