use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use eframe::egui;
use lingua_common::ClientToServer;
use tokio::sync::mpsc;

use crate::theme;

use super::state::{self, AppState, SharedState};
use super::{audio, mic, net, recording, screen};

pub struct StudentApp {
    state: AppState,
    _rt: tokio::runtime::Runtime,
    _mic_capture: mic::MicCapture,
    /// The mic's native capture rate — recordings are saved at this rate rather
    /// than downsampled to the Opus voice rate, since it's a local file, not a
    /// network stream.
    mic_native_rate: u32,
    student_name: String,
    pin_input: String,
    connect_task: Option<tokio::task::JoinHandle<()>>,
    was_locked: bool,
    /// OS focus state as of the last frame, tracked only while locked in test
    /// mode — used to detect the moment focus is *lost* (edge, not level) so
    /// `ClientToServer::FocusLost` is sent once per switch-away, not every frame.
    was_focused: bool,
    chat_input: String,
    /// Version + texture for the incoming screen-demo stream (teacher's own
    /// screen, or a relayed classmate's) — same "poll and diff by version" pattern
    /// the teacher's grid uses for student thumbnails.
    demo_texture: Option<(u64, egui::TextureHandle)>,
    /// In-progress answers for a `Test` assignment currently being filled in —
    /// `None` per slot until the student picks an option for that question.
    /// Immediate-mode UI needs this to live across frames; cleared once submitted.
    test_answers: HashMap<lingua_common::AssignmentId, Vec<Option<usize>>>,
    /// Local, per-machine preferences — see `settings::Settings`'s doc comment.
    /// Loaded once at launch; device/language choices only take effect the
    /// next time capture/playback actually starts (mic and output device are
    /// both opened once for the process's whole lifetime), but the video
    /// quality ceiling applies to the very next screen demo this student
    /// presents, since that's started fresh each time.
    settings: crate::settings::Settings,
    /// Whether the "⚙ Настройки" window is currently open.
    settings_open: bool,
}

impl StudentApp {
    fn connect(&mut self, addr: SocketAddr) {
        if let Some(task) = self.connect_task.take() {
            task.abort();
        }
        self.state.lock().unwrap().connecting = true;
        let state = self.state.clone();
        let name = self.student_name.clone();
        let pin = self.pin_input.trim().to_string();
        let handle = self._rt.spawn(async move {
            if let Err(e) = net::connect_to_teacher(state.clone(), addr, name, pin).await {
                tracing::warn!("connection to {addr} failed: {e:#}");
                let mut guard = state.lock().unwrap();
                guard.connecting = false;
                guard.connected_teacher = None;
                guard.last_error = Some(format!("Не удалось подключиться: {e}"));
            }
        });
        self.connect_task = Some(handle);
    }

    fn disconnect(&mut self) {
        if let Some(task) = self.connect_task.take() {
            task.abort();
        }
        let mut guard = self.state.lock().unwrap();
        guard.connected_teacher = None;
        guard.teacher_addr = None;
        guard.connecting = false;
        guard.to_server = None;
        guard.session_key = None;
        guard.pin.clear();
        guard.peer_addrs.clear();
        guard.peer_names.clear();
        guard.peer_keys.clear();
        guard.uploading_to_teacher = false;
        guard.locked_message = None;
        guard.test_mode_active = false;
        guard.mic_locked = false;
        guard.needs_help = false;
        guard.assignments.clear();
        guard.intercom_active = false;
        guard.material_title = None;
        guard.material_playing = false;
        guard.reference_capture = None;
        guard.reference = None;
        guard.screen_boosted = false;
        guard.demo_presenter = None;
        guard.demo_frame = None;
    }

    fn toggle_help(&mut self) {
        let mut guard = self.state.lock().unwrap();
        guard.needs_help = !guard.needs_help;
        let needed = guard.needs_help;
        if let Some(tx) = &guard.to_server {
            let _ = tx.send(ClientToServer::RequestHelp { needed });
        }
    }

    fn mark_assignment_done(&mut self, id: lingua_common::AssignmentId) {
        let mut guard = self.state.lock().unwrap();
        if let Some(a) = guard.assignments.iter_mut().find(|a| a.id == id) {
            a.done = true;
        }
        if let Some(tx) = &guard.to_server {
            let _ = tx.send(ClientToServer::AssignmentDone { id });
        }
    }

    /// Grades a finished test locally (client-side, like everything else in this
    /// app — no encryption or server-side answer-hiding anywhere else either) and
    /// reports the result, which also counts as completing the assignment — no
    /// separate "mark done" for a test.
    fn submit_test(&mut self, id: lingua_common::AssignmentId, questions: &[lingua_common::TestQuestion], answers: &[usize]) {
        let correct = questions
            .iter()
            .zip(answers.iter())
            .filter(|(q, &a)| q.correct_index == a)
            .count() as u32;
        let total = questions.len() as u32;

        let mut guard = self.state.lock().unwrap();
        if let Some(a) = guard.assignments.iter_mut().find(|a| a.id == id) {
            a.done = true;
            a.last_score = Some((correct, total));
        }
        if let Some(tx) = &guard.to_server {
            let _ = tx.send(ClientToServer::TestResult { id, correct, total });
        }
        drop(guard);
        self.test_answers.remove(&id);
    }

    fn send_chat(&mut self) {
        let text = self.chat_input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let sender = self.state.lock().unwrap().to_server.clone();
        if let Some(tx) = sender {
            let _ = tx.send(ClientToServer::ChatMessage { text: text.clone() });
            self.state.lock().unwrap().chat_log.push(state::ChatEntry {
                from: "Я".to_string(),
                text,
            });
        }
        self.chat_input.clear();
    }

    /// Starts recording the mic to a local WAV file — no new capture stream, just
    /// taps the same PCM already flowing through `audio::run_outbound_and_group_audio`
    /// (see its recording tap) at the mic's native rate.
    fn start_recording(&mut self) {
        let mut guard = self.state.lock().unwrap();
        guard.recording = Some(state::ActiveRecording {
            samples: Vec::new(),
            sample_rate: self.mic_native_rate,
        });
    }

    /// Ends the current recording (if any) and saves it to disk.
    fn stop_recording(&mut self) {
        let active = self.state.lock().unwrap().recording.take();
        let Some(active) = active else { return };
        match recording::save(&active.samples, active.sample_rate) {
            Ok(entry) => self.state.lock().unwrap().saved_recordings.insert(0, entry),
            Err(e) => tracing::warn!("failed to save recording: {e:#}"),
        }
    }

    fn delete_recording(&mut self, path: &Path) {
        if let Err(e) = recording::delete(path) {
            tracing::warn!("failed to delete recording {path:?}: {e:#}");
            return;
        }
        self.state.lock().unwrap().saved_recordings.retain(|r| r.path != path);
    }

    /// Pushes a saved recording to the teacher over the control channel — the
    /// reverse direction of the existing "teacher sends a file" flow, same
    /// `ClientToServer`/`ServerToClient` FileOffer mechanism, just the other way.
    fn send_recording_to_teacher(&mut self, path: &Path) {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("failed to read recording {path:?}: {e:#}");
                return;
            }
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "recording.wav".to_string());
        let sender = self.state.lock().unwrap().to_server.clone();
        if let Some(tx) = sender {
            let _ = tx.send(ClientToServer::FileOffer { name, data });
        }
    }

    /// Local, per-machine preferences: audio devices, and (since this student
    /// might themselves end up presenting a screen demo — see
    /// `screen::run_video_upload`) the same video quality ceiling the teacher
    /// settings tab offers. Saved to disk immediately on any change, and (for
    /// the video quality, which a live connection can pick up without a
    /// restart) written straight through into `SharedState` too.
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        let mut changed = false;
        let mut new_video_quality = None;
        egui::Window::new("⚙ Настройки").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.colored_label(
                theme::muted(),
                "Сохраняются сразу. Выбор устройств ввода/вывода звука применяется при \
                 следующем запуске приложения.",
            );
            ui.add_space(10.0);

            crate::ui_helpers::section_header(ui, "Аудиоустройства");
            ui.add_space(6.0);
            ui.label("Микрофон:");
            let input_names = crate::audio_devices::list_input_device_names();
            let current_input = self.settings.input_device.clone().unwrap_or_else(|| "Системное по умолчанию".to_string());
            egui::ComboBox::from_id_salt("student_settings_input_device").selected_text(current_input).show_ui(ui, |ui| {
                if ui.selectable_label(self.settings.input_device.is_none(), "Системное по умолчанию").clicked() {
                    self.settings.input_device = None;
                    changed = true;
                }
                for name in &input_names {
                    if ui.selectable_label(self.settings.input_device.as_deref() == Some(name.as_str()), name).clicked() {
                        self.settings.input_device = Some(name.clone());
                        changed = true;
                    }
                }
            });
            ui.add_space(6.0);
            ui.label("Устройство вывода:");
            let output_names = crate::audio_devices::list_output_device_names();
            let current_output = self.settings.output_device.clone().unwrap_or_else(|| "Системное по умолчанию".to_string());
            egui::ComboBox::from_id_salt("student_settings_output_device").selected_text(current_output).show_ui(ui, |ui| {
                if ui.selectable_label(self.settings.output_device.is_none(), "Системное по умолчанию").clicked() {
                    self.settings.output_device = None;
                    changed = true;
                }
                for name in &output_names {
                    if ui.selectable_label(self.settings.output_device.as_deref() == Some(name.as_str()), name).clicked() {
                        self.settings.output_device = Some(name.clone());
                        changed = true;
                    }
                }
            });

            ui.add_space(14.0);
            crate::ui_helpers::section_header(ui, "Тема оформления");
            ui.add_space(6.0);
            for theme_choice in [crate::settings::Theme::Dark, crate::settings::Theme::Light] {
                if ui.radio_value(&mut self.settings.theme, theme_choice, theme_choice.label()).clicked() {
                    theme::apply(ctx, self.settings.theme.is_light());
                    changed = true;
                }
            }

            ui.add_space(14.0);
            crate::ui_helpers::section_header(ui, "Качество видео (если вы демонстрируете экран)");
            ui.add_space(6.0);
            ui.colored_label(
                theme::muted(),
                "Потолок — при нехватке производительности во время демонстрации автоматическая \
                 деградация всё равно может снижать качество дальше.",
            );
            ui.add_space(6.0);
            for quality in crate::settings::VideoQuality::ALL {
                if ui.radio_value(&mut self.settings.video_quality, quality, quality.label()).clicked() {
                    changed = true;
                    new_video_quality = Some(quality);
                }
            }

            ui.add_space(14.0);
            crate::ui_helpers::section_header(ui, "Язык интерфейса");
            ui.add_space(6.0);
            egui::ComboBox::from_id_salt("student_settings_language").selected_text(self.settings.language.label()).show_ui(ui, |ui| {
                if ui
                    .selectable_value(
                        &mut self.settings.language,
                        crate::settings::Language::Russian,
                        crate::settings::Language::Russian.label(),
                    )
                    .clicked()
                {
                    changed = true;
                }
            });
        });
        self.settings_open = open;

        if let Some(quality) = new_video_quality {
            self.state.lock().unwrap().video_quality = quality;
        }
        if changed {
            if let Err(e) = self.settings.save() {
                tracing::warn!("failed to save settings: {e:#}");
            }
        }
    }
}

impl eframe::App for StudentApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(150));

        let (locked_message, test_mode_active) = {
            let guard = self.state.lock().unwrap();
            (guard.locked_message.clone(), guard.test_mode_active)
        };

        if let Some(message) = &locked_message {
            if !self.was_locked {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
                self.was_locked = true;
                self.was_focused = ctx.input(|i| i.focused);
            }

            // Test mode: a regular desktop app can't actually block Alt+Tab or
            // other task-switching without an admin-level global hook (which is
            // exactly the kind of invasive, stability-risking trick we're
            // avoiding) — so instead of trying to prevent switching away, this
            // notices it (via OS focus) and does two honest things: fights to
            // reclaim focus/fullscreen/topmost so casually switching away is
            // awkward rather than seamless, and tells the teacher it happened.
            if test_mode_active {
                let focused = ctx.input(|i| i.focused);
                if !focused {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
                }
                if self.was_focused && !focused {
                    let sender = self.state.lock().unwrap().to_server.clone();
                    if let Some(tx) = sender {
                        let _ = tx.send(ClientToServer::FocusLost);
                    }
                }
                self.was_focused = focused;
            }

            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(theme::DANGER))
                .show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new(format!("🔒 {message}"))
                                .size(28.0)
                                .color(egui::Color32::WHITE),
                        );
                    });
                });
            return;
        } else if self.was_locked {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            self.was_locked = false;
        }

        // Screen demo takes over the whole window (not OS-level fullscreen like
        // the lock screen above — this is just informational viewing, not an
        // enforcement mechanism, so an in-window takeover is enough and simpler).
        let demo_presenter = self.state.lock().unwrap().demo_presenter.clone();
        if let Some(presenter) = demo_presenter {
            // Decoding already happened off the UI thread in
            // `screen::run_screen_demo_receiver`; the version check below means
            // this only clones the (Arc-wrapped, so cheap) RGBA buffer on an
            // actual new frame rather than every single UI redraw.
            let demo_version = self.state.lock().unwrap().demo_frame_version;
            let needs_update = self.demo_texture.as_ref().map(|(v, _)| *v != demo_version).unwrap_or(true);
            if needs_update {
                let frame = self.state.lock().unwrap().demo_frame.clone();
                if let Some(frame) = frame {
                    let color_image =
                        egui::ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba);
                    let handle = ctx.load_texture("screen_demo", color_image, egui::TextureOptions::LINEAR);
                    self.demo_texture = Some((demo_version, handle));
                }
            }
            egui::TopBottomPanel::top("demo_top").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.colored_label(theme::accent(), format!("🖥 Демонстрация экрана: {presenter}"));
                ui.add_space(4.0);
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some((_, tex)) = &self.demo_texture {
                    let avail = ui.available_size();
                    let size = tex.size_vec2();
                    let scale = (avail.x / size.x).min(avail.y / size.y);
                    ui.centered_and_justified(|ui| {
                        ui.image((tex.id(), size * scale));
                    });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.colored_label(theme::muted(), "Ожидание изображения…");
                    });
                }
            });
            return;
        } else {
            self.demo_texture = None;
        }

        let already_connected = self.state.lock().unwrap().connected_teacher.is_some();

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("Vocalis — клиент ученика").color(theme::accent()));
                ui.separator();
                if already_connected {
                    ui.label(format!("Имя: {}", self.student_name));
                }
                ui.separator();
                ui.label("🎙");
                theme::wave_meter(ui, audio::MIC_LEVEL_MILLIS.load(Ordering::Relaxed));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙ Настройки").clicked() {
                        self.settings_open = !self.settings_open;
                    }
                });
            });
        });

        self.settings_window(ctx);

        if let Some(err) = self.state.lock().unwrap().last_error.clone() {
            egui::TopBottomPanel::top("error_banner").show(ctx, |ui| {
                ui.colored_label(theme::DANGER, format!("⚠ {err}"));
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let (connected, connecting, peer_names, uploading, files, mic_locked, needs_help, assignments, intercom_active, recording_active, saved_recordings, can_send_to_teacher, material_title, material_playing, reference, screen_boosted) = {
                let guard = self.state.lock().unwrap();
                (
                    guard.connected_teacher.clone(),
                    guard.connecting,
                    guard.peer_names.clone(),
                    guard.uploading_to_teacher,
                    guard
                        .received_files
                        .iter()
                        .map(|f| (f.name.clone(), f.path.display().to_string()))
                        .collect::<Vec<_>>(),
                    guard.mic_locked,
                    guard.needs_help,
                    guard
                        .assignments
                        .iter()
                        .map(|a| (a.id, a.title.clone(), a.kind, a.done, a.content.clone(), a.last_score))
                        .collect::<Vec<_>>(),
                    guard.intercom_active,
                    guard.recording.is_some(),
                    guard
                        .saved_recordings
                        .iter()
                        .map(|r| (r.path.clone(), r.duration_secs))
                        .collect::<Vec<_>>(),
                    guard.to_server.is_some(),
                    guard.material_title.clone(),
                    guard.material_playing,
                    guard.reference.as_ref().map(|r| r.path.clone()),
                    guard.screen_boosted,
                )
            };

            if screen_boosted {
                ui.colored_label(theme::accent(), "🖥 Ваш экран сейчас транслируется классу");
            }

            // "Модельное произношение": the teacher played (or is playing) a
            // material — surface the existing record button prominently right
            // here, since this is exactly when a student would want it.
            if let Some(title) = &material_title {
                egui::Frame::none()
                    .fill(theme::accent().linear_multiply(0.3))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let verb = if material_playing { "звучит" } else { "прозвучал" };
                            ui.colored_label(
                                theme::accent_300(),
                                format!("🎧 Сейчас {verb} материал «{title}». Повторите за диктором!"),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let rec_label = if recording_active { "⏹ Остановить" } else { "🔴 Записать" };
                                if ui.button(rec_label).clicked() {
                                    if recording_active {
                                        self.stop_recording();
                                    } else {
                                        self.start_recording();
                                    }
                                }
                            });
                        });
                    });
                ui.add_space(6.0);
            }
            if let (Some(reference_path), Some((latest_path, _))) = (&reference, saved_recordings.first()) {
                ui.label("Сравните с эталоном:");
                if ui.button("▶ Прослушать: Эталон").clicked() {
                    if let Err(e) = open::that(reference_path) {
                        tracing::warn!("failed to open reference recording: {e:#}");
                    }
                }
                if ui.button("▶ Прослушать: Моя попытка").clicked() {
                    if let Err(e) = open::that(latest_path) {
                        tracing::warn!("failed to open recording: {e:#}");
                    }
                }
                ui.add_space(6.0);
            }

            ui.label("Мои записи (для самопроверки произношения или как домашнее задание):");
            ui.horizontal(|ui| {
                let rec_label = if recording_active { "⏹ Остановить запись" } else { "🔴 Записать" };
                let rec_button = egui::Button::new(rec_label).fill(if recording_active {
                    theme::DANGER.linear_multiply(0.35)
                } else {
                    ui.style().visuals.widgets.inactive.bg_fill
                });
                if ui.add(rec_button).clicked() {
                    if recording_active {
                        self.stop_recording();
                    } else {
                        self.start_recording();
                    }
                }
                if recording_active {
                    ui.colored_label(theme::DANGER, "🔴 Идёт запись...");
                }
            });
            if !saved_recordings.is_empty() {
                let mut to_delete = None;
                let mut to_send = None;
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    for (path, duration_secs) in &saved_recordings {
                        ui.horizontal(|ui| {
                            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            if ui
                                .add(egui::Button::new(format!("▶ {name} ({duration_secs:.0} сек)")).frame(false))
                                .clicked()
                            {
                                if let Err(e) = open::that(path) {
                                    tracing::warn!("failed to open recording {path:?}: {e:#}");
                                }
                            }
                            if can_send_to_teacher && ui.button("📤 Отправить учителю").clicked() {
                                to_send = Some(path.clone());
                            }
                            if ui.button("🗑").on_hover_text("Удалить запись").clicked() {
                                to_delete = Some(path.clone());
                            }
                        });
                    }
                });
                if let Some(path) = to_delete {
                    self.delete_recording(&path);
                }
                if let Some(path) = to_send {
                    self.send_recording_to_teacher(&path);
                }
            }
            ui.separator();

            if let Some(teacher_name) = connected {
                ui.colored_label(theme::OK, format!("✅ Подключено к: {teacher_name}"));
                if intercom_active {
                    egui::Frame::none()
                        .fill(theme::accent().linear_multiply(0.3))
                        .rounding(egui::Rounding::same(8.0))
                        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                        .show(ui, |ui| {
                            ui.colored_label(
                                theme::accent_300(),
                                "🎧 Преподаватель говорит с вами лично — это приватный разговор, не общий урок",
                            );
                        });
                    ui.add_space(6.0);
                }
                if !peer_names.is_empty() {
                    ui.label(format!("🔗 В группе с: {}", peer_names.join(", ")));
                }
                if uploading {
                    ui.colored_label(theme::WARN, "🔴 Учитель слушает ваш микрофон в реальном времени");
                }
                if mic_locked {
                    ui.colored_label(theme::DANGER, "🔒 Микрофон заблокирован преподавателем");
                }
                ui.horizontal(|ui| {
                    if ui.button("Отключиться").clicked() {
                        self.disconnect();
                    }
                    let help_label = if needs_help { "✋ Отменить запрос помощи" } else { "✋ Попросить помощь" };
                    let help_button = egui::Button::new(help_label).fill(if needs_help {
                        theme::WARN.linear_multiply(0.35)
                    } else {
                        ui.style().visuals.widgets.inactive.bg_fill
                    });
                    if ui.add(help_button).clicked() {
                        self.toggle_help();
                    }
                });

                if !assignments.is_empty() {
                    ui.separator();
                    ui.label("Задания:");
                    let mut to_complete = None;
                    let mut to_submit: Option<(lingua_common::AssignmentId, Vec<lingua_common::TestQuestion>, Vec<usize>)> = None;
                    for (id, title, kind, done, content, last_score) in &assignments {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(theme::accent(), format!("[{}]", kind.label()));
                                ui.strong(title);
                            });
                            match content {
                                Some(lingua_common::AssignmentContent::Test { questions }) => {
                                    if *done {
                                        let score = last_score
                                            .map(|(c, t)| format!("✔ Тест завершён: {c}/{t}"))
                                            .unwrap_or_else(|| "✔ Тест завершён".to_string());
                                        ui.colored_label(theme::OK, score);
                                    } else {
                                        let answers = self
                                            .test_answers
                                            .entry(*id)
                                            .or_insert_with(|| vec![None; questions.len()]);
                                        for (qi, q) in questions.iter().enumerate() {
                                            ui.add_space(4.0);
                                            ui.label(format!("{}. {}", qi + 1, q.text));
                                            for (oi, opt) in q.options.iter().enumerate() {
                                                ui.radio_value(&mut answers[qi], Some(oi), opt);
                                            }
                                        }
                                        let all_answered = answers.iter().all(|a| a.is_some());
                                        ui.add_space(6.0);
                                        if ui.add_enabled(all_answered, egui::Button::new("Завершить тест")).clicked() {
                                            let picked: Vec<usize> = answers.iter().map(|a| a.unwrap()).collect();
                                            to_submit = Some((*id, questions.clone(), picked));
                                        }
                                    }
                                }
                                Some(lingua_common::AssignmentContent::Listening { material_title, questions }) => {
                                    ui.label(format!("🎧 Материал: {material_title}"));
                                    for (qi, q) in questions.iter().enumerate() {
                                        ui.label(format!("{}. {q}", qi + 1));
                                    }
                                    if *done {
                                        ui.colored_label(theme::OK, "✔ Готово");
                                    } else if ui.button("Отметить готовым").clicked() {
                                        to_complete = Some(*id);
                                    }
                                }
                                Some(lingua_common::AssignmentContent::Reading { text }) => {
                                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                                        ui.label(text);
                                    });
                                    if *done {
                                        ui.colored_label(theme::OK, "✔ Готово");
                                    } else if ui.button("Отметить готовым").clicked() {
                                        to_complete = Some(*id);
                                    }
                                }
                                None => {
                                    if *done {
                                        ui.colored_label(theme::OK, "✔ Готово");
                                    } else if ui.button("Отметить готовым").clicked() {
                                        to_complete = Some(*id);
                                    }
                                }
                            }
                        });
                    }
                    if let Some(id) = to_complete {
                        self.mark_assignment_done(id);
                    }
                    if let Some((id, questions, picked)) = to_submit {
                        self.submit_test(id, &questions, &picked);
                    }
                }

                ui.separator();
                ui.label("Чат:");
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    let guard = self.state.lock().unwrap();
                    for entry in &guard.chat_log {
                        ui.label(format!("{}: {}", entry.from, entry.text));
                    }
                });
                ui.horizontal(|ui| {
                    let resp = ui.text_edit_singleline(&mut self.chat_input);
                    let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("Отправить").clicked() || enter_pressed {
                        self.send_chat();
                    }
                });

                if !files.is_empty() {
                    ui.separator();
                    ui.label("Полученные файлы (нажмите, чтобы открыть):");
                    for (name, path) in files {
                        let resp = ui
                            .add(egui::Button::new(format!("📄 {name}")).frame(false))
                            .on_hover_text(&path);
                        if resp.clicked() {
                            if let Err(e) = open::that(&path) {
                                tracing::warn!("failed to open received file '{path}': {e:#}");
                            }
                        }
                    }
                }
            } else {
                ui.label("Ваше имя и фамилия (увидит преподаватель):");
                bordered_text_edit(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.student_name).frame(false).desired_width(280.0));
                });
                let name_ready = !self.student_name.trim().is_empty();
                if !name_ready {
                    ui.colored_label(theme::WARN, "Введите имя, чтобы можно было подключиться");
                }
                ui.add_space(8.0);

                ui.label("PIN-код урока (сообщит преподаватель):");
                bordered_text_edit(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.pin_input).frame(false).desired_width(120.0));
                });
                let pin_trimmed = self.pin_input.trim().to_string();
                let pin_ready = (4..=6).contains(&pin_trimmed.len())
                    && pin_trimmed.chars().all(|c| c.is_ascii_digit());
                if !pin_ready {
                    ui.colored_label(theme::WARN, "Введите PIN-код урока (4-6 цифр)");
                }
                ui.add_space(8.0);

                ui.separator();

                if connecting {
                    ui.label("Подключение...");
                }
                ui.label("Доступные преподаватели в сети:");
                ui.add_space(8.0);

                let mut entries: Vec<(SocketAddr, String)> = {
                    let guard = self.state.lock().unwrap();
                    guard
                        .discovered
                        .iter()
                        .map(|(addr, t)| (*addr, t.name.clone()))
                        .collect()
                };
                entries.sort_by(|a, b| a.1.cmp(&b.1));

                if entries.is_empty() {
                    ui.colored_label(theme::muted(), "Пока никого не найдено. Убедитесь, что находитесь в одной сети с преподавателем.");
                }

                let mut to_connect = None;
                for (addr, name) in &entries {
                    ui.horizontal(|ui| {
                        ui.label(format!("{name} ({})", addr.ip()));
                        if ui
                            .add_enabled(name_ready && pin_ready, egui::Button::new("Подключиться"))
                            .clicked()
                        {
                            to_connect = Some(*addr);
                        }
                    });
                }
                if let Some(addr) = to_connect {
                    self.connect(addr);
                }
            }
        });
    }
}

/// Draws a `frame(false)` text field wrapped in an explicit border — the
/// default `TextEdit` frame's stroke barely shows up against this app's
/// customized (very dark) backgrounds, and this is the very first field a
/// student sees, so it needs to visibly read as an input rather than blend
/// into the panel behind it.
fn bordered_text_edit(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .stroke(egui::Stroke::new(1.0_f32, theme::muted()))
        .rounding(egui::Rounding::same(4.0))
        .inner_margin(egui::Margin::symmetric(6.0, 4.0))
        .show(ui, |ui| add_contents(ui));
}

fn default_student_name() -> String {
    // Left blank on purpose: the student types their own name/surname before
    // connecting (shown to the teacher), rather than us guessing from the OS.
    std::env::var("VOCALIS_STUDENT_NAME").unwrap_or_default()
}

impl StudentApp {
    /// Spins up the student's background tasks (discovery listener, audio pipeline,
    /// mic capture) and returns a ready-to-run app. Called once the launcher screen
    /// learns the user picked the student role.
    pub fn launch() -> Self {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

        let settings = crate::settings::Settings::load();
        // Must happen before `audio::default_output_sample_rate()` below, or
        // the (lazily-started, only-once-per-process) output stream — see
        // `audio_devices::configure_output_device`'s doc comment.
        crate::audio_devices::configure_output_device(settings.output_device.clone());

        let state: AppState = Arc::new(Mutex::new(SharedState {
            saved_recordings: recording::list_existing(),
            video_quality: settings.video_quality,
            ..SharedState::default()
        }));

        {
            let state = state.clone();
            rt.spawn(async move {
                if let Err(e) = net::run_discovery(state).await {
                    tracing::warn!("discovery task stopped: {e:#}");
                }
            });
        }
        {
            let state = state.clone();
            rt.spawn(net::run_discovery_pruner(state));
        }
        {
            let state = state.clone();
            rt.spawn(audio::run_level_telemetry(state));
        }

        let mix = audio::new_mix_state();
        let output_rate = audio::default_output_sample_rate();
        {
            let state = state.clone();
            let mix = mix.clone();
            rt.spawn(async move {
                if let Err(e) = audio::run_mic_broadcast_receiver(state, mix, output_rate).await {
                    tracing::warn!("mic broadcast receiver stopped: {e:#}");
                }
            });
        }
        {
            let state = state.clone();
            let mix = mix.clone();
            rt.spawn(async move {
                if let Err(e) = audio::run_intercom_receiver(state, mix, output_rate).await {
                    tracing::warn!("intercom receiver stopped: {e:#}");
                }
            });
        }
        {
            let state = state.clone();
            rt.spawn(async move {
                if let Err(e) = screen::run_screen_demo_receiver(state).await {
                    tracing::warn!("screen-demo video receiver stopped: {e:#}");
                }
            });
        }
        {
            let state = state.clone();
            let mix = mix.clone();
            rt.spawn(async move {
                if let Err(e) = audio::run_screen_audio_receiver(state, mix, output_rate).await {
                    tracing::warn!("screen-demo audio receiver stopped: {e:#}");
                }
            });
        }

        let (mic_tx, mic_rx) = mpsc::unbounded_channel::<Vec<i16>>();
        let (mic_capture, mic_sample_rate) =
            mic::start_mic_capture(mic_tx, settings.input_device.as_deref()).expect("failed to start microphone capture");
        {
            let state = state.clone();
            let mix = mix.clone();
            rt.spawn(async move {
                if let Err(e) = audio::run_outbound_and_group_audio(
                    state,
                    mix,
                    mic_rx,
                    mic_sample_rate,
                    output_rate,
                )
                .await
                {
                    tracing::warn!("outbound/group audio task stopped: {e:#}");
                }
            });
        }

        StudentApp {
            state,
            _rt: rt,
            _mic_capture: mic_capture,
            mic_native_rate: mic_sample_rate,
            student_name: default_student_name(),
            pin_input: String::new(),
            connect_task: None,
            was_locked: false,
            was_focused: true,
            chat_input: String::new(),
            demo_texture: None,
            test_answers: HashMap::new(),
            settings,
            settings_open: false,
        }
    }
}
