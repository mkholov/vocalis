use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use lingua_common::{AssignmentKind, ServerToClient, StudentId, CONTROL_PORT};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::theme;

use super::state::{self, AppState, Presence, SharedState};
use super::{db, listen, mic, net};

const ASSIGNMENT_TEMPLATES: &[(&str, AssignmentKind)] = &[
    ("Аудирование: заказ в кафе", AssignmentKind::Listening),
    ("Тест: неправильные глаголы", AssignmentKind::Test),
    ("Диалог в парах: интервью", AssignmentKind::Dialogue),
    ("Чтение вслух: текст 4", AssignmentKind::Pronunciation),
];

fn assignment_kind_color(kind: AssignmentKind) -> egui::Color32 {
    match kind {
        AssignmentKind::Listening => theme::ACCENT,
        AssignmentKind::Test => theme::WARN,
        AssignmentKind::Dialogue => egui::Color32::from_rgb(122, 162, 247),
        AssignmentKind::Pronunciation => theme::OK,
    }
}

fn presence_label_color(p: Presence) -> (&'static str, egui::Color32) {
    match p {
        Presence::Empty => ("Не подключен", theme::MUTED),
        Presence::NeedsHelp => ("Просит помощь", theme::WARN),
        Presence::Speaking => ("Говорит", theme::ACCENT),
        Presence::Connected => ("На связи", theme::OK),
    }
}

fn initials(name: &str) -> String {
    let mut it = name.split_whitespace().filter_map(|w| w.chars().next());
    match (it.next(), it.next()) {
        (Some(a), Some(b)) => format!("{a}{b}").to_uppercase(),
        (Some(a), None) => a.to_uppercase().to_string(),
        _ => "?".to_string(),
    }
}

fn avatar_color(name: &str) -> egui::Color32 {
    let hash: u32 = name.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let hue = (hash % 360) as f32;
    egui::ecolor::Hsva::new(hue / 360.0, 0.45, 0.55, 1.0).into()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Class,
    Stats,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GridMode {
    Individual,
    Pairs,
    Groups,
}

struct MicHandle {
    _capture: mic::MicCapture,
    // Terminates on its own once `_capture` is dropped (that closes the channel the
    // capture callback feeds); kept only to tie its lifetime to this handle.
    _broadcast_task: tokio::task::JoinHandle<()>,
}

pub struct TeacherApp {
    state: AppState,
    _rt: tokio::runtime::Runtime,
    mic: Option<MicHandle>,
    textures: HashMap<StudentId, (u64, egui::TextureHandle)>,
    selected: HashSet<StudentId>,
    dragging: Option<StudentId>,
    chat_input: String,
    teacher_name: Arc<str>,
    tab: Tab,
    grid_mode: GridMode,
    focus: bool,
    score_edit: Option<(StudentId, String)>,
}

impl TeacherApp {
    fn new(state: AppState, rt: tokio::runtime::Runtime, teacher_name: Arc<str>) -> Self {
        Self {
            state,
            _rt: rt,
            mic: None,
            textures: HashMap::new(),
            selected: HashSet::new(),
            dragging: None,
            chat_input: String::new(),
            teacher_name,
            tab: Tab::Class,
            grid_mode: GridMode::Individual,
            focus: false,
            score_edit: None,
        }
    }

    fn toggle_mic(&mut self) {
        if self.mic.take().is_some() {
            self.state.lock().unwrap().mic_broadcasting = false;
            return;
        }
        let (tx, rx) = mpsc::unbounded_channel::<Vec<i16>>();
        match mic::start_mic_capture(tx) {
            Ok((capture, sample_rate)) => {
                let state = self.state.clone();
                let broadcast_task = self
                    ._rt
                    .spawn(async move { let _ = mic::run_mic_broadcast(state, rx, sample_rate).await; });
                self.mic = Some(MicHandle {
                    _capture: capture,
                    _broadcast_task: broadcast_task,
                });
                self.state.lock().unwrap().mic_broadcasting = true;
            }
            Err(e) => {
                tracing::warn!("failed to start microphone capture: {e:#}");
            }
        }
    }

    fn set_locked(&mut self, id: StudentId, locked: bool) {
        let mut guard = self.state.lock().unwrap();
        if let Some(s) = guard.students.get_mut(&id) {
            s.locked = locked;
            let msg = if locked {
                ServerToClient::LockScreen {
                    message: "Экран заблокирован преподавателем".to_string(),
                }
            } else {
                ServerToClient::UnlockScreen
            };
            let _ = s.to_client.send(msg);
        }
    }

    fn toggle_listen(&mut self, id: StudentId) {
        let mut guard = self.state.lock().unwrap();
        if guard.listening_to == Some(id) {
            if let Some(s) = guard.students.get(&id) {
                let _ = s.to_client.send(ServerToClient::StopMicUpload);
            }
            guard.listening_to = None;
            return;
        }
        if let Some(prev) = guard.listening_to.take() {
            if let Some(s) = guard.students.get(&prev) {
                let _ = s.to_client.send(ServerToClient::StopMicUpload);
            }
        }
        if let Some(s) = guard.students.get(&id) {
            let _ = s.to_client.send(ServerToClient::StartMicUpload);
            guard.listening_to = Some(id);
        }
    }

    /// Cards to act on for sidebar quick actions/assignments: the current selection,
    /// or everyone connected when nothing is selected.
    fn action_targets(&self, guard: &state::SharedState) -> Vec<StudentId> {
        if self.selected.is_empty() {
            guard.students.keys().copied().collect()
        } else {
            self.selected.iter().copied().collect()
        }
    }

    fn send_assignment(&mut self, title: &str, kind: AssignmentKind) {
        let mut guard = self.state.lock().unwrap();
        let targets = self.action_targets(&guard);
        for id in targets {
            let student_db_id = match guard.students.get(&id) {
                Some(s) => s.db_id,
                None => continue,
            };
            let assignment_db_id = student_db_id
                .and_then(|row_id| db::insert_assignment(&guard.db, row_id, title, kind).ok());

            let Some(s) = guard.students.get_mut(&id) else { continue };
            let assignment_id = Uuid::new_v4();
            s.assignments.push(state::AssignmentInstance {
                id: assignment_id,
                title: title.to_string(),
                kind,
                done: false,
                db_id: assignment_db_id,
            });
            let _ = s.to_client.send(ServerToClient::AssignmentOffer {
                id: assignment_id,
                title: title.to_string(),
                kind,
            });
        }
    }

    fn send_chat(&mut self) {
        let text = self.chat_input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let mut guard = self.state.lock().unwrap();
        let targets = self.action_targets(&guard);
        let msg = ServerToClient::ChatMessage {
            from: self.teacher_name.to_string(),
            text: text.clone(),
        };
        for id in targets {
            if let Some(s) = guard.students.get(&id) {
                let _ = s.to_client.send(msg.clone());
            }
        }
        guard.chat_log.push(state::ChatEntry {
            from: format!("Я ({})", self.teacher_name),
            text,
        });
        drop(guard);
        self.chat_input.clear();
    }

    fn send_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().pick_file() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("failed to read file {path:?}: {e:#}");
                return;
            }
        };
        let guard = self.state.lock().unwrap();
        let targets = self.action_targets(&guard);
        let msg = ServerToClient::FileOffer { name, data };
        for id in targets {
            if let Some(s) = guard.students.get(&id) {
                let _ = s.to_client.send(msg.clone());
            }
        }
    }

    /// Click behavior on a seat card depends on the current grouping mode.
    fn handle_card_click(&mut self, id: StudentId) {
        match self.grid_mode {
            GridMode::Individual => {
                self.selected.clear();
                self.selected.insert(id);
            }
            GridMode::Pairs => {
                if self.selected.contains(&id) {
                    self.selected.remove(&id);
                } else {
                    self.selected.insert(id);
                    if self.selected.len() == 2 {
                        let ids: Vec<StudentId> = self.selected.iter().copied().collect();
                        self.state.lock().unwrap().create_group(&[ids[0], ids[1]]);
                        self.selected.clear();
                    }
                }
            }
            GridMode::Groups => {
                if self.selected.contains(&id) {
                    self.selected.remove(&id);
                } else {
                    self.selected.insert(id);
                }
            }
        }
    }

    fn update_texture_cache(&mut self, ctx: &egui::Context, id: StudentId, jpeg: &[u8], version: u64) {
        let needs_update = self
            .textures
            .get(&id)
            .map(|(v, _)| *v != version)
            .unwrap_or(true);
        if !needs_update {
            return;
        }
        if let Ok(img) = image::load_from_memory(jpeg) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let color_image =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
            let handle = ctx.load_texture(format!("thumb-{id}"), color_image, egui::TextureOptions::LINEAR);
            self.textures.insert(id, (version, handle));
        }
    }
}

fn format_timer(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

impl eframe::App for TeacherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(150));

        // Pull any new screen frames into the texture cache regardless of whether the
        // compact grid shows them — the focus view needs them ready immediately.
        let frames: Vec<(StudentId, Vec<u8>, u64)> = {
            let guard = self.state.lock().unwrap();
            guard
                .students
                .iter()
                .filter_map(|(id, s)| s.last_frame_jpeg.as_ref().map(|j| (*id, j.clone(), s.frame_version)))
                .collect()
        };
        for (id, jpeg, version) in frames {
            self.update_texture_cache(ctx, id, &jpeg, version);
        }

        self.top_bar(ctx);

        if self.tab == Tab::Stats {
            self.stats_tab(ctx);
        } else if self.focus {
            self.focus_view(ctx);
        } else {
            egui::SidePanel::right("sidebar").resizable(true).default_width(380.0).width_range(320.0..=520.0).show(ctx, |ui| {
                self.sidebar(ui);
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                self.class_grid(ui, ctx);
            });
        }
    }
}

impl TeacherApp {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("Vocalis").color(theme::ACCENT));
                ui.label("Лингафонный кабинет");
                ui.add_space(8.0);

                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(41, 46, 54))
                    .rounding(egui::Rounding::same(14.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                    .show(ui, |ui| {
                        let mut guard = self.state.lock().unwrap();
                        ui.add(
                            egui::TextEdit::singleline(&mut guard.class_name)
                                .frame(false)
                                .desired_width(160.0),
                        );
                    });

                ui.add_space(8.0);
                ui.colored_label(theme::MUTED, "PIN урока:");
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(41, 46, 54))
                    .rounding(egui::Rounding::same(14.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                    .show(ui, |ui| {
                        let mut guard = self.state.lock().unwrap();
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut guard.lesson_pin)
                                .frame(false)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(70.0),
                        );
                        if resp.changed() {
                            guard.lesson_pin.retain(|c| c.is_ascii_digit());
                            guard.lesson_pin.truncate(6);
                        }
                    })
                    .response
                    .on_hover_text("Сообщите этот код ученикам — он понадобится при подключении");

                let elapsed = self.state.lock().unwrap().lesson_started_at.elapsed();
                ui.label(format_timer(elapsed));

                ui.add_space(12.0);
                ui.selectable_value(&mut self.tab, Tab::Class, "Класс");
                ui.selectable_value(&mut self.tab, Tab::Stats, "Статистика");

                ui.add_space(12.0);
                let locked = self.state.lock().unwrap().mics_locked;
                let lock_icon = if locked { "🔒" } else { "🔓" };
                if ui.button(lock_icon).on_hover_text("Заблокировать все микрофоны").clicked() {
                    let new_val = !locked;
                    self.state.lock().unwrap().set_mics_locked(new_val);
                }

                let mic_on = self.state.lock().unwrap().mic_broadcasting;
                let label = if mic_on { "⏹ Остановить" } else { "📡 Транслировать" };
                if ui.button(label).clicked() {
                    self.toggle_mic();
                }
                if mic_on {
                    theme::wave_meter(ui, mic::MIC_LEVEL_MILLIS.load(Ordering::Relaxed));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let enabled = self.selected.len() == 1;
                    if ui
                        .add_enabled(enabled, egui::Button::new("Экран ученика →"))
                        .clicked()
                    {
                        self.focus = true;
                    }
                });
            });
            ui.add_space(6.0);
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let selected_one = if self.selected.len() == 1 {
            self.selected.iter().next().copied()
        } else {
            None
        };

        if let Some(id) = selected_one {
            let (name, locked, listening) = {
                let guard = self.state.lock().unwrap();
                match guard.students.get(&id) {
                    Some(s) => (s.name.clone(), s.locked, guard.listening_to == Some(id)),
                    None => (String::new(), false, false),
                }
            };
            ui.strong(&name);
            ui.horizontal(|ui| {
                let btn_label = if locked { "Разблокировать" } else { "Заблокировать" };
                if ui.button(btn_label).clicked() {
                    self.set_locked(id, !locked);
                }
                let listen_label = if listening { "Прекратить" } else { "🎧 Слушать" };
                if ui.button(listen_label).clicked() {
                    self.toggle_listen(id);
                }
                if listening {
                    theme::wave_meter(ui, listen::LISTEN_LEVEL_MILLIS.load(Ordering::Relaxed));
                }
            });
        } else {
            ui.colored_label(theme::MUTED, "Выберите ученика в сетке, чтобы прослушать его или отправить задание.");
        }

        ui.add_space(14.0);
        ui.colored_label(theme::MUTED, "БЫСТРЫЕ ДЕЙСТВИЯ");
        ui.add_space(4.0);

        let locked = self.state.lock().unwrap().mics_locked;
        let lock_label = if locked { "🔓 Разблокировать все микрофоны" } else { "🔒 Заблокировать все микрофоны" };
        if ui.add_sized([ui.available_width(), 40.0], egui::Button::new(lock_label)).clicked() {
            self.state.lock().unwrap().set_mics_locked(!locked);
        }
        let mic_on = self.state.lock().unwrap().mic_broadcasting;
        let mic_label = if mic_on { "⏹ Остановить трансляцию" } else { "📡 Транслировать" };
        if ui.add_sized([ui.available_width(), 40.0], egui::Button::new(mic_label)).clicked() {
            self.toggle_mic();
        }

        if self.grid_mode == GridMode::Groups && self.selected.len() >= 2 {
            if ui
                .add_sized([ui.available_width(), 40.0], egui::Button::new("🔗 Создать группу из выбранных"))
                .clicked()
            {
                let ids: Vec<StudentId> = self.selected.iter().copied().collect();
                self.state.lock().unwrap().create_group(&ids);
                self.selected.clear();
            }
        }
        if ui.add_sized([ui.available_width(), 36.0], egui::Button::new("Разъединить все группы")).clicked() {
            self.state.lock().unwrap().leave_all_groups();
        }

        ui.add_space(16.0);
        ui.colored_label(theme::MUTED, "ЗАДАНИЯ");
        ui.add_space(4.0);
        for (title, kind) in ASSIGNMENT_TEMPLATES {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(*title);
                    let color = assignment_kind_color(*kind);
                    egui::Frame::none()
                        .fill(color.linear_multiply(0.25))
                        .rounding(egui::Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(6.0, 1.0))
                        .show(ui, |ui| {
                            ui.colored_label(color, kind.label());
                        });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📤").on_hover_text("Отправить").clicked() {
                        self.send_assignment(title, *kind);
                    }
                });
            });
        }

        ui.add_space(16.0);
        ui.separator();
        ui.colored_label(theme::MUTED, "ЧАТ");
        egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
            let guard = self.state.lock().unwrap();
            for entry in &guard.chat_log {
                ui.label(format!("{}: {}", entry.from, entry.text));
            }
        });
        ui.horizontal(|ui| {
            let resp = ui.text_edit_singleline(&mut self.chat_input);
            let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("➤").clicked() || enter_pressed {
                self.send_chat();
            }
            if ui.button("📎").on_hover_text("Отправить файл").clicked() {
                self.send_file();
            }
        });
    }

    fn class_grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let class_size = self.state.lock().unwrap().class_size;
            ui.heading(format!("Класс — {class_size} мест"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut self.grid_mode, GridMode::Groups, "Группы");
                ui.selectable_value(&mut self.grid_mode, GridMode::Pairs, "Пары");
                ui.selectable_value(&mut self.grid_mode, GridMode::Individual, "Индивидуально");
                let mut size = class_size as i32;
                if ui.add(egui::DragValue::new(&mut size).range(1..=60)).changed() {
                    self.state.lock().unwrap().class_size = size.max(1) as usize;
                }
                ui.colored_label(theme::MUTED, "мест:");
            });
        });
        ui.add_space(10.0);

        let (class_size, seats): (usize, HashMap<usize, StudentId>) = {
            let guard = self.state.lock().unwrap();
            (
                guard.class_size,
                guard.students.iter().map(|(id, s)| (s.seat, *id)).collect(),
            )
        };

        let pointer_released = ctx.input(|i| i.pointer.any_released());
        let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
        let mut group_action: Option<(StudentId, StudentId)> = None;
        let mut click_action: Option<StudentId> = None;

        // Responsive sizing: pick a column count that both (a) never squeezes a card
        // below `min_card_w`, and (b) roughly fills the visible height too — with few
        // seats on a wide screen this yields fewer, bigger cards instead of a small
        // cluster in the corner with the rest of the window empty. Cards then stretch
        // to fill each row evenly, up to `max_card_w`.
        let spacing = 18.0;
        let min_card_w = 230.0;
        let max_card_w = 340.0;
        let card_h = 148.0;

        let avail_w = ui.available_width();
        let avail_h = ui.available_height().max(200.0);

        let max_columns_by_width =
            ((avail_w + spacing) / (min_card_w + spacing)).floor().max(1.0) as usize;
        let rows_that_fit_height =
            ((avail_h + spacing) / (card_h + spacing)).floor().max(1.0) as usize;
        let ideal_columns_for_fill = (class_size as f32 / rows_that_fit_height as f32)
            .ceil()
            .max(1.0) as usize;
        let columns = max_columns_by_width.min(ideal_columns_for_fill).max(1);
        let card_width = ((avail_w - (columns as f32 - 1.0) * spacing) / columns as f32).min(max_card_w);

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("seats_grid")
                .num_columns(columns)
                .spacing([spacing, spacing])
                .show(ui, |ui| {
                    for seat in 1..=class_size {
                        let occupant = seats.get(&seat).copied();
                        let response = self.seat_card(ui, seat, occupant, card_width, card_h);
                        if let Some(id) = occupant {
                            if response.clicked() {
                                click_action = Some(id);
                            }
                            if response.drag_started() {
                                self.dragging = Some(id);
                            }
                            if let Some(dragged_id) = self.dragging {
                                if dragged_id != id && pointer_released {
                                    if let Some(pos) = pointer_pos {
                                        if response.rect.contains(pos) {
                                            group_action = Some((dragged_id, id));
                                        }
                                    }
                                }
                            }
                        }
                        if seat % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        });

        if pointer_released {
            self.dragging = None;
        }
        if let Some(id) = click_action {
            self.handle_card_click(id);
        }
        if let Some((a, b)) = group_action {
            self.state.lock().unwrap().group_with(a, b);
        }
    }

    fn seat_card(
        &mut self,
        ui: &mut egui::Ui,
        seat: usize,
        occupant: Option<StudentId>,
        width: f32,
        height: f32,
    ) -> egui::Response {
        let selected = occupant.map(|id| self.selected.contains(&id)).unwrap_or(false);

        let frame = egui::Frame::group(ui.style())
            .rounding(egui::Rounding::same(12.0))
            .inner_margin(egui::Margin::same(14.0))
            .stroke(if selected {
                egui::Stroke::new(2.5_f32, theme::ACCENT)
            } else {
                ui.style().visuals.widgets.noninteractive.bg_stroke
            });

        let outer = ui.scope(|ui| {
            ui.set_width(width);
            frame.show(ui, |ui| {
                ui.set_min_height(height);
                match occupant {
                    None => {
                        ui.horizontal(|ui| {
                            ui.colored_label(theme::MUTED, format!("#{seat}"));
                        });
                        ui.centered_and_justified(|ui| {
                            let (label, color) = presence_label_color(Presence::Empty);
                            ui.colored_label(color, label);
                        });
                    }
                    Some(id) => {
                        let (name, group, needs_help, presence, level_active) = {
                            let guard = self.state.lock().unwrap();
                            match guard.students.get(&id) {
                                Some(s) => (s.name.clone(), s.group.is_some(), s.needs_help, s.presence(), s.presence() == Presence::Speaking),
                                None => return,
                            }
                        };
                        ui.horizontal(|ui| {
                            ui.colored_label(theme::MUTED, format!("#{seat}"));
                            if group {
                                ui.colored_label(theme::ACCENT, "🔗");
                            }
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let color = avatar_color(&name);
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 24.0, color);
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                initials(&name),
                                egui::FontId::proportional(17.0),
                                egui::Color32::WHITE,
                            );
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                let display_name = if name.chars().count() > 16 {
                                    format!("{}…", name.chars().take(15).collect::<String>())
                                } else {
                                    name.clone()
                                };
                                ui.strong(display_name);
                                let (label, color) = presence_label_color(if needs_help { Presence::NeedsHelp } else { presence });
                                ui.horizontal(|ui| {
                                    ui.colored_label(color, if level_active { "🎙" } else { "⚪" });
                                    ui.colored_label(color, label);
                                });
                            });
                        });
                    }
                }
            });
        });

        let sense = if occupant.is_some() {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::hover()
        };
        ui.interact(outer.response.rect, egui::Id::new(("seat_card", seat)), sense)
    }

    fn focus_view(&mut self, ctx: &egui::Context) {
        let id = match self.selected.iter().next().copied() {
            Some(id) => id,
            None => {
                self.focus = false;
                return;
            }
        };
        egui::CentralPanel::default().show(ctx, |ui| {
            let (name, locked) = {
                let guard = self.state.lock().unwrap();
                match guard.students.get(&id) {
                    Some(s) => (s.name.clone(), s.locked),
                    None => {
                        self.focus = false;
                        return;
                    }
                }
            };
            ui.horizontal(|ui| {
                if ui.button("← Назад").clicked() {
                    self.focus = false;
                }
                ui.heading(&name);
                let btn_label = if locked { "Разблокировать" } else { "Заблокировать" };
                if ui.button(btn_label).clicked() {
                    self.set_locked(id, !locked);
                }
            });
            ui.add_space(10.0);
            if let Some((_, tex)) = self.textures.get(&id) {
                let avail = ui.available_size();
                let size = tex.size_vec2();
                let scale = (avail.x / size.x).min(avail.y / size.y);
                ui.centered_and_justified(|ui| {
                    ui.image((tex.id(), size * scale));
                });
            } else {
                ui.colored_label(theme::MUTED, "Пока нет изображения экрана.");
            }
        });
    }

    fn stats_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let (avg_score, done_total, connected, class_size, attendance, history) = {
                let guard = self.state.lock().unwrap();
                (
                    guard.average_score(),
                    guard.assignments_completed_total(),
                    guard.connected_count(),
                    guard.class_size.max(1),
                    guard.ever_connected_seats.len(),
                    guard.history,
                )
            };

            ui.horizontal(|ui| {
                stat_tile(ui, "СРЕДНИЙ БАЛЛ", &avg_score.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "—".to_string()));
                stat_tile(ui, "ЗАДАНИЙ ВЫПОЛНЕНО", &done_total.to_string());
                stat_tile(ui, "АКТИВНЫ СЕЙЧАС", &format!("{connected} / {class_size}"));
                stat_tile(ui, "ПОСЕЩАЕМОСТЬ", &format!("{:.0}%", attendance as f32 / class_size as f32 * 100.0));
            });

            ui.add_space(10.0);
            ui.colored_label(theme::MUTED, "ЗА ВСЁ ВРЕМЯ (сохранено локально)");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                stat_tile(ui, "УРОКОВ ПРОВЕДЕНО", &history.lessons_count.to_string());
                stat_tile(
                    ui,
                    "СРЕДНИЙ БАЛЛ ЗА ВСЁ ВРЕМЯ",
                    &history.avg_score.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "—".to_string()),
                );
                stat_tile(ui, "ЗАДАНИЙ ВЫПОЛНЕНО ЗА ВСЁ ВРЕМЯ", &history.assignments_done.to_string());
            });

            ui.add_space(16.0);
            ui.colored_label(theme::MUTED, "ПРОГРЕСС ПО УЧЕНИКАМ");
            ui.add_space(6.0);

            type Row = (usize, StudentId, String, Presence, usize, Vec<(String, AssignmentKind, bool)>, Option<u32>);
            let mut rows: Vec<Row> = {
                let guard = self.state.lock().unwrap();
                guard
                    .students
                    .iter()
                    .map(|(id, s)| {
                        (
                            s.seat,
                            *id,
                            s.name.clone(),
                            s.presence(),
                            s.assignments_done(),
                            s.assignments.iter().map(|a| (a.title.clone(), a.kind, a.done)).collect(),
                            s.score,
                        )
                    })
                    .collect()
            };
            rows.sort_by_key(|r| r.0);

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("stats_table").num_columns(5).striped(true).min_col_width(80.0).show(ui, |ui| {
                    ui.colored_label(theme::MUTED, "МЕСТО");
                    ui.colored_label(theme::MUTED, "УЧЕНИК");
                    ui.colored_label(theme::MUTED, "СТАТУС");
                    ui.colored_label(theme::MUTED, "ЗАДАНИЙ");
                    ui.colored_label(theme::MUTED, "СРЕДНИЙ БАЛЛ");
                    ui.end_row();

                    for (seat, id, name, presence, done, assignments, score) in rows {
                        ui.label(format!("#{seat}"));
                        ui.label(name);
                        let (label, color) = presence_label_color(presence);
                        ui.colored_label(color, label);
                        let total = assignments.len();
                        ui.label(format!("{done}/{total}"))
                            .on_hover_ui(|ui| {
                                if assignments.is_empty() {
                                    ui.colored_label(theme::MUTED, "Заданий пока не отправлено");
                                }
                                for (title, kind, is_done) in &assignments {
                                    ui.horizontal(|ui| {
                                        ui.colored_label(
                                            if *is_done { theme::OK } else { egui::Color32::GRAY },
                                            if *is_done { "✔" } else { "…" },
                                        );
                                        ui.colored_label(assignment_kind_color(*kind), kind.label());
                                        ui.label(title);
                                    });
                                }
                            });

                        if self.score_edit.as_ref().map(|(eid, _)| *eid) == Some(id) {
                            let buf = &mut self.score_edit.as_mut().unwrap().1;
                            let resp = ui.add(egui::TextEdit::singleline(buf).desired_width(50.0));
                            if resp.lost_focus() {
                                if let Ok(v) = buf.parse::<u32>() {
                                    let score = v.min(100);
                                    let mut guard = self.state.lock().unwrap();
                                    let db_id = guard.students.get(&id).and_then(|s| s.db_id);
                                    if let Some(s) = guard.students.get_mut(&id) {
                                        s.score = Some(score);
                                    }
                                    if let Some(db_id) = db_id {
                                        let _ = db::update_score(&guard.db, db_id, score);
                                    }
                                }
                                self.score_edit = None;
                            }
                        } else {
                            let text = score.map(|v| format!("{v}%")).unwrap_or_else(|| "—".to_string());
                            if ui.button(text).on_hover_text("Изменить оценку").clicked() {
                                self.score_edit = Some((id, score.map(|v| v.to_string()).unwrap_or_default()));
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        });
    }
}

fn stat_tile(ui: &mut egui::Ui, label: &str, value: &str) {
    egui::Frame::group(ui.style())
        .rounding(egui::Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(16.0, 12.0))
        .show(ui, |ui| {
            ui.set_min_width(200.0);
            ui.colored_label(theme::MUTED, label);
            ui.add_space(4.0);
            ui.heading(value);
        });
}

impl TeacherApp {
    /// Spins up the teacher's background tasks (discovery announcer, control server,
    /// listen-in receiver) and returns a ready-to-run app. Called once the launcher
    /// screen learns the user picked the teacher role.
    pub fn launch(teacher_name: String) -> Self {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

        let class_name = std::env::var("VOCALIS_CLASS_NAME").unwrap_or_else(|_| "9А · Английский язык".to_string());
        let class_size: usize = std::env::var("VOCALIS_CLASS_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(16);
        let lesson_pin = std::env::var("VOCALIS_LESSON_PIN")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(state::generate_pin);

        let db_conn = db::open().expect("failed to open Vocalis database");
        let history = db::load_history_summary(&db_conn).unwrap_or_default();
        let lesson_row_id = db::insert_lesson(&db_conn, &class_name).expect("failed to record lesson start");

        let state: AppState = Arc::new(Mutex::new(SharedState::new(
            class_name,
            class_size,
            lesson_pin,
            db_conn,
            lesson_row_id,
            history,
        )));
        let teacher_name: Arc<str> = Arc::from(teacher_name);

        {
            let name_for_announce = teacher_name.to_string();
            rt.spawn(async move {
                if let Err(e) = lingua_common::run_teacher_announcer(name_for_announce, CONTROL_PORT).await {
                    tracing::warn!("discovery announcer stopped: {e:#}");
                }
            });
        }
        {
            let state = state.clone();
            let teacher_name = teacher_name.clone();
            rt.spawn(async move {
                if let Err(e) = net::run_control_server(state, teacher_name).await {
                    tracing::error!("control server stopped: {e:#}");
                }
            });
        }

        let listen_queue = listen::new_listen_queue();
        let listen_output_rate = listen::default_output_sample_rate();
        {
            let listen_queue = listen_queue.clone();
            rt.spawn(async move {
                if let Err(e) = listen::run_listen_receiver(listen_queue, listen_output_rate).await {
                    tracing::warn!("listen-in receiver stopped: {e:#}");
                }
            });
        }

        TeacherApp::new(state, rt, teacher_name)
    }
}
