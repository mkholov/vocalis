//! Local, offline password gate for the teacher console — not a real
//! authentication system (no server, no accounts), just "don't let someone who
//! walks up to this school PC open the class roster and grades". First launch
//! shows a "create profile" screen; later launches ask for name + password with
//! no attempt-limiting (this isn't worth locking a teacher out over a typo), plus
//! a reset that deletes only the profile — lesson/student/grade data lives in its
//! own tables untouched by this module.

use eframe::egui;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::theme;

use super::db;

/// Cheap key stretching with zero extra dependencies: re-hash the running digest
/// together with the salt and password this many times. Not a peer-reviewed KDF
/// like Argon2/PBKDF2, but a single SHA-256 pass is fast enough that a stolen
/// database would be trivially brute-forceable — a few hundred thousand rounds
/// costs a login check nothing noticeable while raising that cost substantially.
const HASH_ROUNDS: u32 = 200_000;

fn generate_salt() -> [u8; 16] {
    *Uuid::new_v4().as_bytes()
}

fn hash_password(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut digest: [u8; 32] = Sha256::new().chain_update(salt).chain_update(password.as_bytes()).finalize().into();
    for _ in 1..HASH_ROUNDS {
        digest = Sha256::new().chain_update(digest).chain_update(salt).chain_update(password.as_bytes()).finalize().into();
    }
    digest
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

enum AuthMode {
    /// No profile exists yet.
    Setup,
    /// A profile exists; `profile_name` is shown as a hint but the entered name
    /// still has to match it (see the doc comment on `try_login`).
    Login { profile_name: String },
}

/// The screen shown before the teacher console proper — see the module doc
/// comment. `update` renders one frame and returns `Some(name)` once the teacher
/// is authenticated (freshly created profile, or a successful login), at which
/// point the caller swaps this out for the real `TeacherApp`.
pub struct AuthScreen {
    mode: AuthMode,
    name_input: String,
    password_input: String,
    password_confirm: String,
    error: Option<String>,
    show_reset_confirm: bool,
}

impl AuthScreen {
    pub fn new() -> Self {
        let mode = db::open()
            .ok()
            .and_then(|conn| db::load_teacher_profile(&conn).ok().flatten())
            .map(|p| AuthMode::Login { profile_name: p.name })
            .unwrap_or(AuthMode::Setup);
        Self {
            mode,
            name_input: String::new(),
            password_input: String::new(),
            password_confirm: String::new(),
            error: None,
            show_reset_confirm: false,
        }
    }

    pub fn update(&mut self, ctx: &egui::Context) -> Option<String> {
        let mut result = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.heading(egui::RichText::new("Vocalis").size(36.0).color(theme::ACCENT));
                ui.label("Лингафонный кабинет — консоль преподавателя");
                ui.add_space(24.0);

                ui.group(|ui| {
                    ui.set_width(340.0);
                    match &self.mode {
                        AuthMode::Setup => self.setup_ui(ui, &mut result),
                        AuthMode::Login { .. } => self.login_ui(ui, &mut result),
                    }
                });
            });
        });
        result
    }

    fn setup_ui(&mut self, ui: &mut egui::Ui, result: &mut Option<String>) {
        ui.strong("Создание профиля преподавателя");
        ui.add_space(10.0);
        ui.label("Имя:");
        ui.text_edit_singleline(&mut self.name_input);
        ui.add_space(6.0);
        ui.label("Пароль:");
        ui.add(egui::TextEdit::singleline(&mut self.password_input).password(true));
        ui.add_space(6.0);
        ui.label("Повторите пароль:");
        let resp = ui.add(egui::TextEdit::singleline(&mut self.password_confirm).password(true));
        let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(10.0);
        if let Some(err) = &self.error {
            ui.colored_label(theme::DANGER, err);
            ui.add_space(6.0);
        }
        if ui.add_sized([300.0, 40.0], egui::Button::new("Создать профиль")).clicked() || enter_pressed {
            self.try_create_profile(result);
        }
    }

    fn login_ui(&mut self, ui: &mut egui::Ui, result: &mut Option<String>) {
        ui.strong("Вход в профиль преподавателя");
        ui.add_space(10.0);
        ui.label("Имя:");
        ui.text_edit_singleline(&mut self.name_input);
        ui.add_space(6.0);
        ui.label("Пароль:");
        let resp = ui.add(egui::TextEdit::singleline(&mut self.password_input).password(true));
        let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(10.0);
        if let Some(err) = &self.error {
            ui.colored_label(theme::DANGER, err);
            ui.add_space(6.0);
        }
        if ui.add_sized([300.0, 40.0], egui::Button::new("Войти")).clicked() || enter_pressed {
            self.try_login(result);
        }

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(6.0);
        if !self.show_reset_confirm {
            if ui.small_button("Забыли пароль? Сбросить профиль").clicked() {
                self.show_reset_confirm = true;
                self.error = None;
            }
        } else {
            ui.colored_label(
                theme::WARN,
                "Это удалит текущий профиль преподавателя (имя и пароль) — придётся создать новый. \
                 Уроки, ученики и оценки не удаляются, это данные урока, а не профиля.",
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Да, сбросить профиль").clicked() {
                    self.reset_profile();
                }
                if ui.button("Отмена").clicked() {
                    self.show_reset_confirm = false;
                }
            });
        }
    }

    fn try_create_profile(&mut self, result: &mut Option<String>) {
        let name = self.name_input.trim().to_string();
        if name.is_empty() {
            self.error = Some("Введите имя".to_string());
            return;
        }
        if self.password_input.is_empty() {
            self.error = Some("Введите пароль".to_string());
            return;
        }
        if self.password_input != self.password_confirm {
            self.error = Some("Пароли не совпадают".to_string());
            return;
        }
        let conn = match db::open() {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("Не удалось открыть базу данных: {e}"));
                return;
            }
        };
        let salt = generate_salt();
        let hash = hash_password(&self.password_input, &salt);
        if let Err(e) = db::save_teacher_profile(&conn, &name, &hex_encode(&salt), &hex_encode(&hash)) {
            self.error = Some(format!("Не удалось сохранить профиль: {e}"));
            return;
        }
        *result = Some(name);
    }

    /// The entered name has to match the stored profile's name too, not just the
    /// password — there's only one profile, so this isn't picking a user, it's
    /// just an extra field the login screen was asked to have. Both checks fail
    /// into the same generic message, since there's no multi-user reason to
    /// distinguish "wrong name" from "wrong password" here.
    fn try_login(&mut self, result: &mut Option<String>) {
        let AuthMode::Login { profile_name } = &self.mode else { return };
        if db::normalize_name(&self.name_input) != db::normalize_name(profile_name) {
            self.error = Some("Неверное имя или пароль".to_string());
            return;
        }
        let conn = match db::open() {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("Не удалось открыть базу данных: {e}"));
                return;
            }
        };
        let profile = match db::load_teacher_profile(&conn) {
            Ok(Some(p)) => p,
            _ => {
                self.error = Some("Профиль не найден — потребуется создать новый".to_string());
                return;
            }
        };
        let Some(salt) = hex_decode(&profile.salt) else {
            self.error = Some("Профиль повреждён — потребуется сбросить его".to_string());
            return;
        };
        let hash = hash_password(&self.password_input, &salt);
        if hex_encode(&hash) == profile.password_hash {
            *result = Some(profile.name);
        } else {
            self.error = Some("Неверное имя или пароль".to_string());
        }
    }

    fn reset_profile(&mut self) {
        if let Ok(conn) = db::open() {
            let _ = db::delete_teacher_profile(&conn);
        }
        self.mode = AuthMode::Setup;
        self.name_input.clear();
        self.password_input.clear();
        self.password_confirm.clear();
        self.error = None;
        self.show_reset_confirm = false;
    }
}
