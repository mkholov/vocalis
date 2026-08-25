use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use eframe::egui;
use lingua_common::ClientToServer;
use tokio::sync::mpsc;

use crate::theme;

use super::state::{self, AppState, SharedState};
use super::{audio, mic, net};

pub struct StudentApp {
    state: AppState,
    _rt: tokio::runtime::Runtime,
    _mic_capture: mic::MicCapture,
    student_name: String,
    connect_task: Option<tokio::task::JoinHandle<()>>,
    was_locked: bool,
    chat_input: String,
}

impl StudentApp {
    fn connect(&mut self, addr: SocketAddr) {
        if let Some(task) = self.connect_task.take() {
            task.abort();
        }
        self.state.lock().unwrap().connecting = true;
        let state = self.state.clone();
        let name = self.student_name.clone();
        let handle = self._rt.spawn(async move {
            if let Err(e) = net::connect_to_teacher(state.clone(), addr, name).await {
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
        guard.peer_addrs.clear();
        guard.peer_names.clear();
        guard.uploading_to_teacher = false;
        guard.locked_message = None;
        guard.mic_locked = false;
        guard.needs_help = false;
        guard.assignments.clear();
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
}

impl eframe::App for StudentApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(150));

        let locked_message = self.state.lock().unwrap().locked_message.clone();

        if let Some(message) = &locked_message {
            if !self.was_locked {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
                self.was_locked = true;
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

        let already_connected = self.state.lock().unwrap().connected_teacher.is_some();

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("Vocalis — клиент ученика").color(theme::ACCENT));
                ui.separator();
                if already_connected {
                    ui.label(format!("Имя: {}", self.student_name));
                }
                ui.separator();
                ui.label("🎙");
                theme::wave_meter(ui, audio::MIC_LEVEL_MILLIS.load(Ordering::Relaxed));
            });
        });

        if let Some(err) = self.state.lock().unwrap().last_error.clone() {
            egui::TopBottomPanel::top("error_banner").show(ctx, |ui| {
                ui.colored_label(theme::DANGER, format!("⚠ {err}"));
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let (connected, connecting, peer_names, uploading, files, mic_locked, needs_help, assignments) = {
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
                        .map(|a| (a.id, a.title.clone(), a.kind, a.done))
                        .collect::<Vec<_>>(),
                )
            };

            if let Some(teacher_name) = connected {
                ui.colored_label(theme::OK, format!("✅ Подключено к: {teacher_name}"));
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
                    for (id, title, kind, done) in &assignments {
                        ui.horizontal(|ui| {
                            ui.label(format!("[{}] {title}", kind.label()));
                            if *done {
                                ui.colored_label(theme::OK, "✔ Готово");
                            } else if ui.button("Отметить готовым").clicked() {
                                to_complete = Some(*id);
                            }
                        });
                    }
                    if let Some(id) = to_complete {
                        self.mark_assignment_done(id);
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
                    ui.label("Полученные файлы:");
                    for (name, path) in files {
                        ui.label(format!("📄 {name}")).on_hover_text(path);
                    }
                }
            } else {
                ui.label("Ваше имя и фамилия (увидит преподаватель):");
                ui.add(egui::TextEdit::singleline(&mut self.student_name).desired_width(280.0));
                ui.add_space(8.0);

                let name_ready = !self.student_name.trim().is_empty();
                if !name_ready {
                    ui.colored_label(theme::WARN, "Введите имя, чтобы можно было подключиться");
                }

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
                    ui.colored_label(theme::MUTED, "Пока никого не найдено. Убедитесь, что находитесь в одной сети с преподавателем.");
                }

                let mut to_connect = None;
                for (addr, name) in &entries {
                    ui.horizontal(|ui| {
                        ui.label(format!("{name} ({})", addr.ip()));
                        if ui
                            .add_enabled(name_ready, egui::Button::new("Подключиться"))
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
        let state: AppState = Arc::new(Mutex::new(SharedState::default()));

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
            let mix = mix.clone();
            rt.spawn(async move {
                if let Err(e) = audio::run_mic_broadcast_receiver(mix, output_rate).await {
                    tracing::warn!("mic broadcast receiver stopped: {e:#}");
                }
            });
        }

        let (mic_tx, mic_rx) = mpsc::unbounded_channel::<Vec<i16>>();
        let (mic_capture, mic_sample_rate) =
            mic::start_mic_capture(mic_tx).expect("failed to start microphone capture");
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
            student_name: default_student_name(),
            connect_task: None,
            was_locked: false,
            chat_input: String::new(),
        }
    }
}
