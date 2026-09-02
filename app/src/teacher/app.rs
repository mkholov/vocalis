use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use lingua_common::{AssignmentKind, ServerToClient, StudentId, CONTROL_PORT};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{audio_devices, settings, theme};

use super::state::{self, AppState, Presence, SharedState};
use super::{csv_export, db, listen, materials, mic, net, screen, system_audio};

// Test/Listening/Reading now go through the "Задания" tab's authored-template
// library (real questions/content, not a label) — this quick-send list is left
// for Dialogue, which is inherently a live/verbal exercise (paired students +
// existing grouping/intercom infrastructure) rather than something with content
// to author.
const ASSIGNMENT_TEMPLATES: &[(&str, AssignmentKind)] = &[
    ("Диалог в парах: интервью", AssignmentKind::Dialogue),
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

fn event_label_color(event: &str) -> (&'static str, egui::Color32) {
    match event {
        "connected" => ("Подключился", theme::OK),
        "disconnected" => ("Отключился", theme::MUTED),
        "rejected_pin" => ("⚠ Отклонён: неверный PIN", theme::DANGER),
        _ => ("?", theme::MUTED),
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
    Materials,
    Assignments,
    Roster,
    ConnectionLog,
    Settings,
}

/// Which kind of assignment the "Задания" tab's editor is currently authoring.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorKind {
    Test,
    Listening,
    Reading,
}

impl Default for EditorKind {
    fn default() -> Self {
        Self::Test
    }
}

/// One question being built in the editor — `options`/`correct_index` are only
/// meaningful for `EditorKind::Test`; a `Listening` question is just `text`.
struct DraftQuestion {
    text: String,
    options: Vec<String>,
    correct_index: usize,
}

impl Default for DraftQuestion {
    fn default() -> Self {
        Self {
            text: String::new(),
            options: vec![String::new(), String::new()],
            correct_index: 0,
        }
    }
}

/// In-progress state for the "Задания" tab's assignment editor — has to live on
/// `TeacherApp` (not be locally-scoped per frame) since egui is immediate-mode and
/// authoring a multi-question test spans many frames of typing.
#[derive(Default)]
struct AssignmentDraft {
    kind: EditorKind,
    title: String,
    reading_text: String,
    material_id: Option<i64>,
    questions: Vec<DraftQuestion>,
}

impl AssignmentDraft {
    fn reset(&mut self) {
        *self = Self::default();
    }
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

/// Same shape as `MicHandle` (capture handle + its broadcast task, the task
/// ending on its own once the capture side drops its channel sender), for the
/// teacher's system-audio stream during a self-sourced screen demo.
struct SystemAudioHandle {
    _capture: system_audio::SystemAudioCapture,
    _broadcast_task: tokio::task::JoinHandle<()>,
}

pub struct TeacherApp {
    state: AppState,
    _rt: tokio::runtime::Runtime,
    mic: Option<MicHandle>,
    /// Own mic capture + send task for the private intercom leg. Entirely
    /// independent of `mic` (a second, concurrent `cpal` input stream) so a
    /// class-wide broadcast and a private word with one student can run at once.
    intercom: Option<MicHandle>,
    /// The materials-playback send task, if a material is currently playing.
    /// Unlike `mic`/`intercom` there's no live device capture to own here — just
    /// the task streaming pre-decoded samples out — so `.abort()` on stop is the
    /// only way to end it early (dropping the handle alone would not).
    playback: Option<tokio::task::JoinHandle<()>>,
    /// The teacher's own-screen capture/send task, if `screen_demo`'s source is
    /// `Teacher`. Demoing a *student's* screen instead needs no task here at all —
    /// `teacher::net` just relays that student's existing uploads as they arrive.
    screen_demo_task: Option<tokio::task::JoinHandle<()>>,
    /// The teacher's system-audio capture, alongside `screen_demo_task` — only
    /// ever `Some` for a `ScreenDemoSource::Teacher` demo (system audio is the
    /// *teacher's* machine's sound; demoing a student's screen has no
    /// equivalent to capture here). `None` whenever loopback capture isn't
    /// available (e.g. on macOS, or a Windows machine with no default playback
    /// device) — the video demo still runs fine without it.
    screen_system_audio: Option<SystemAudioHandle>,
    textures: HashMap<StudentId, (u64, egui::TextureHandle)>,
    selected: HashSet<StudentId>,
    dragging: Option<StudentId>,
    chat_input: String,
    teacher_name: Arc<str>,
    tab: Tab,
    grid_mode: GridMode,
    focus: bool,
    score_edit: Option<(StudentId, String)>,
    assignment_draft: AssignmentDraft,
    /// New-student-name field on the "Список класса" tab.
    roster_input: String,
    /// Inline rename in progress on the roster list (row id, buffer) — mirrors
    /// `score_edit`'s pattern.
    roster_edit: Option<(i64, String)>,
    /// (name, class_id) of the student whose cross-lesson history card is open,
    /// if any — a drill-down reachable from both the roster and stats tabs, so
    /// it's tracked independently of `tab` rather than being one itself. Carries
    /// its own `class_id` since it can be opened for a class other than the
    /// active lesson's (e.g. browsing another class's roster).
    history_card: Option<(String, i64)>,
    /// "Только текущий урок" toggle on the Журнал tab. Defaults on: right after
    /// class, the current lesson's log is almost always what you want first.
    log_filter_current_lesson: bool,
    /// Which class's roster the "Список класса" tab is currently browsing/editing
    /// — independent of `SharedState.current_class_id` (the active lesson's
    /// class, fixed for the session) so the teacher can manage another class's
    /// list without disturbing the running lesson. Defaults to the active class.
    roster_view_class_id: i64,
    /// New-class-name field for the "Создать класс" inline form on the roster tab.
    roster_new_class_name: String,
    /// Local, per-machine preferences (audio devices, video quality ceiling,
    /// UI language) — loaded once at launch, saved back to disk immediately
    /// on every change from the "Настройки" tab. See `settings::Settings`'s
    /// doc comment for why these live in their own file rather than the
    /// lesson database.
    settings: settings::Settings,
}

impl TeacherApp {
    #[allow(clippy::too_many_arguments)]
    fn new(state: AppState, rt: tokio::runtime::Runtime, teacher_name: Arc<str>, class_id: i64, settings: settings::Settings) -> Self {
        Self {
            state,
            _rt: rt,
            mic: None,
            intercom: None,
            playback: None,
            screen_demo_task: None,
            screen_system_audio: None,
            textures: HashMap::new(),
            selected: HashSet::new(),
            dragging: None,
            chat_input: String::new(),
            teacher_name,
            tab: Tab::Class,
            grid_mode: GridMode::Individual,
            focus: false,
            score_edit: None,
            assignment_draft: AssignmentDraft::default(),
            roster_input: String::new(),
            roster_edit: None,
            history_card: None,
            log_filter_current_lesson: true,
            roster_view_class_id: class_id,
            roster_new_class_name: String::new(),
            settings,
        }
    }

    fn toggle_mic(&mut self) {
        if self.mic.take().is_some() {
            self.state.lock().unwrap().mic_broadcasting = false;
            return;
        }
        // Live mic broadcast and materials playback both go out over MIC_PORT as a
        // single Opus stream — can't have two independent streams on one port (see
        // `materials::run_playback`'s doc comment), so starting one stops the other.
        self.stop_playback();

        let (tx, rx) = mpsc::unbounded_channel::<Vec<i16>>();
        match mic::start_mic_capture(tx, &mic::MIC_LEVEL_MILLIS, self.settings.input_device.as_deref()) {
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

    /// Opens a file picker for an mp3/wav, decodes it, and adds it to the library
    /// (persisted immediately so it survives a restart).
    fn upload_material(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Аудио", &["mp3", "wav"]).pick_file() else {
            return;
        };
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Материал".to_string());
        let path_str = path.display().to_string();

        let mut guard = self.state.lock().unwrap();
        match db::insert_material(&guard.db, &title, &path_str) {
            Ok(id) => guard.materials.insert(0, db::MaterialRow { id, title, file_path: path_str }),
            Err(e) => tracing::warn!("failed to save material '{title}': {e:#}"),
        }
    }

    /// Decodes and streams `material_id` to the current selection (or the whole
    /// class if nothing's selected — the same `action_targets` rule used for
    /// assignments/files), over MIC_PORT exactly like a live broadcast. Stops
    /// whatever was playing before, and stops a live mic broadcast if one is running.
    fn play_material(&mut self, material_id: i64) {
        self.stop_playback();
        if self.mic.is_some() {
            self.toggle_mic();
        }

        let (title, path) = {
            let guard = self.state.lock().unwrap();
            match guard.materials.iter().find(|m| m.id == material_id) {
                Some(m) => (m.title.clone(), m.file_path.clone()),
                None => return,
            }
        };

        let (samples, native_rate) = match materials::decode_to_mono_pcm(std::path::Path::new(&path)) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to decode material '{title}' at {path}: {e:#}");
                return;
            }
        };
        let total_ms = (samples.len() as u64 * 1000) / native_rate.max(1) as u64;

        let target_ids: Vec<StudentId> = {
            let guard = self.state.lock().unwrap();
            self.action_targets(&guard)
        };
        if target_ids.is_empty() {
            return;
        }

        {
            let mut guard = self.state.lock().unwrap();
            for id in &target_ids {
                if let Some(s) = guard.students.get(id) {
                    let _ = s.to_client.send(ServerToClient::MaterialPlaying { title: title.clone() });
                }
            }
            guard.playing = Some(state::PlayingMaterial {
                material_id,
                title,
                total_ms,
                elapsed_ms: 0,
                targets: target_ids.clone(),
            });
        }

        let state = self.state.clone();
        let task = self
            ._rt
            .spawn(async move { let _ = materials::run_playback(state, samples, native_rate, target_ids).await; });
        self.playback = Some(task);
    }

    /// Stops whatever material is currently playing, if any, and tells everyone it
    /// was playing to that it's over. Safe to call when nothing is playing.
    fn stop_playback(&mut self) {
        if let Some(task) = self.playback.take() {
            task.abort();
        }
        let mut guard = self.state.lock().unwrap();
        if let Some(playing) = guard.playing.take() {
            for id in &playing.targets {
                if let Some(s) = guard.students.get(id) {
                    let _ = s.to_client.send(ServerToClient::MaterialStopped);
                }
            }
        }
    }

    /// Toggles demoing the teacher's own screen to the current selection (or the
    /// whole class if nothing's selected — same `action_targets` rule as
    /// assignments/files/materials).
    fn toggle_own_screen_demo(&mut self) {
        let already_teacher = matches!(
            self.state.lock().unwrap().screen_demo.as_ref().map(|d| d.source),
            Some(state::ScreenDemoSource::Teacher)
        );
        if already_teacher {
            self.stop_screen_demo();
            return;
        }
        let target_ids: Vec<StudentId> = {
            let guard = self.state.lock().unwrap();
            self.action_targets(&guard)
        };
        if target_ids.is_empty() {
            return;
        }
        self.start_screen_demo(state::ScreenDemoSource::Teacher, self.teacher_name.to_string(), target_ids);
    }

    /// Toggles demoing `id`'s screen to the rest of the class (deliberately
    /// everyone *but* `id` — showing someone's screen back to themselves makes no
    /// sense, so this ignores the current selection rather than reusing
    /// `action_targets`).
    fn toggle_student_screen_demo(&mut self, id: StudentId) {
        let already_this_student = matches!(
            self.state.lock().unwrap().screen_demo.as_ref().map(|d| d.source),
            Some(state::ScreenDemoSource::Student(sid)) if sid == id
        );
        if already_this_student {
            self.stop_screen_demo();
            return;
        }
        let (name, target_ids) = {
            let guard = self.state.lock().unwrap();
            let Some(name) = guard.students.get(&id).map(|s| s.name.clone()) else { return };
            let targets = guard.students.keys().filter(|&&sid| sid != id).copied().collect();
            (name, targets)
        };
        self.start_screen_demo(state::ScreenDemoSource::Student(id), name, target_ids);
    }

    fn start_screen_demo(&mut self, source: state::ScreenDemoSource, presenter_name: String, targets: Vec<StudentId>) {
        self.stop_screen_demo();
        if targets.is_empty() {
            return;
        }

        {
            let mut guard = self.state.lock().unwrap();
            for id in &targets {
                if let Some(s) = guard.students.get(id) {
                    let _ = s.to_client.send(ServerToClient::StartScreenDemo {
                        presenter: presenter_name.clone(),
                    });
                }
            }
            if let state::ScreenDemoSource::Student(presenter_id) = source {
                if let Some(s) = guard.students.get(&presenter_id) {
                    let _ = s.to_client.send(ServerToClient::StartVideoUpload);
                }
            }
            guard.screen_demo = Some(state::ScreenDemo {
                source,
                presenter_name,
                targets: targets.clone(),
            });
        }

        if let state::ScreenDemoSource::Teacher = source {
            let state = self.state.clone();
            let video_targets = targets.clone();
            let starting_level = self.settings.video_quality.ladder_level();
            let task = self._rt.spawn(async move { screen::run_own_screen_demo(state, video_targets, starting_level).await });
            self.screen_demo_task = Some(task);

            self.screen_system_audio = self.start_screen_system_audio(targets);
        }
    }

    /// Starts capturing and broadcasting the teacher's system audio for a
    /// teacher-sourced screen demo, if loopback capture is available on this
    /// platform/machine (Windows only — see `system_audio`'s doc comment).
    /// Returns `None` (logging why) rather than treating it as a hard error:
    /// the video keeps working with or without this stream.
    fn start_screen_system_audio(&mut self, targets: Vec<StudentId>) -> Option<SystemAudioHandle> {
        let (tx, rx) = mpsc::unbounded_channel::<Vec<i16>>();
        let (capture, native_rate) = match system_audio::start_system_audio_capture(tx) {
            Ok(v) => v,
            Err(e) => {
                tracing::info!("no system audio for this screen demo: {e:#}");
                return None;
            }
        };
        let state = self.state.clone();
        let task = self
            ._rt
            .spawn(async move { let _ = mic::run_screen_audio_broadcast(state, rx, native_rate, targets).await; });
        Some(SystemAudioHandle { _capture: capture, _broadcast_task: task })
    }

    /// Stops whatever screen demo is currently active, if any. Safe to call when
    /// nothing is running.
    fn stop_screen_demo(&mut self) {
        if let Some(task) = self.screen_demo_task.take() {
            task.abort();
        }
        self.screen_system_audio = None;
        let mut guard = self.state.lock().unwrap();
        if let Some(demo) = guard.screen_demo.take() {
            for id in &demo.targets {
                if let Some(s) = guard.students.get(id) {
                    let _ = s.to_client.send(ServerToClient::StopScreenDemo);
                }
            }
            if let state::ScreenDemoSource::Student(presenter_id) = demo.source {
                if let Some(s) = guard.students.get(&presenter_id) {
                    let _ = s.to_client.send(ServerToClient::StopVideoUpload);
                }
            }
        }
    }

    /// Locks (or unlocks) a student's screen. `test_mode` additionally starts the
    /// focus-loss monitoring/resistance on the student's side and resets their
    /// violation count — meaningless when `locked` is false.
    fn set_locked(&mut self, id: StudentId, locked: bool, test_mode: bool) {
        let mut guard = self.state.lock().unwrap();
        if let Some(s) = guard.students.get_mut(&id) {
            s.locked = locked;
            s.test_mode = locked && test_mode;
            if s.test_mode {
                s.test_violations = 0;
            }
            let msg = if locked {
                let message = if test_mode {
                    "Тестовый режим: не переключайтесь на другие приложения".to_string()
                } else {
                    "Экран заблокирован преподавателем".to_string()
                };
                ServerToClient::LockScreen { message, test_mode }
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
            // Can't keep privately talking to someone we've just stopped listening to.
            let was_talking = guard.talking_to == Some(id);
            drop(guard);
            if was_talking {
                self.stop_intercom(id);
            }
            return;
        }
        guard.start_listening(id);
    }

    /// Opens (or closes) a private two-way intercom with `id`: the teacher's own
    /// mic goes to just this student (a second, independent `cpal` capture — the
    /// class-wide broadcast, if running, is untouched), and listen-in is switched
    /// to this student too so the teacher hears them back (reusing the exact same
    /// listen-in mechanism the plain "Слушать" button uses).
    fn toggle_intercom(&mut self, id: StudentId) {
        let currently_talking_to = self.state.lock().unwrap().talking_to;
        if currently_talking_to == Some(id) {
            self.stop_intercom(id);
            return;
        }
        if let Some(prev) = currently_talking_to {
            self.stop_intercom(prev);
        }

        let target = {
            let guard = self.state.lock().unwrap();
            guard.students.get(&id).map(|s| (s.ip, s.session_key))
        };
        let Some((ip, key)) = target else { return };

        self.state.lock().unwrap().start_listening(id);

        let (tx, rx) = mpsc::unbounded_channel::<Vec<i16>>();
        match mic::start_mic_capture(tx, &mic::INTERCOM_MIC_LEVEL_MILLIS, self.settings.input_device.as_deref()) {
            Ok((capture, sample_rate)) => {
                let target = std::net::SocketAddr::new(ip, lingua_common::TEACHER_INTERCOM_PORT);
                let send_task = self
                    ._rt
                    .spawn(async move { let _ = mic::run_intercom_send(rx, sample_rate, target, key).await; });
                self.intercom = Some(MicHandle {
                    _capture: capture,
                    _broadcast_task: send_task,
                });
                let mut guard = self.state.lock().unwrap();
                guard.talking_to = Some(id);
                if let Some(s) = guard.students.get(&id) {
                    let _ = s.to_client.send(ServerToClient::StartIntercom);
                }
            }
            Err(e) => tracing::warn!("failed to start intercom microphone capture: {e:#}"),
        }
    }

    /// Stops the intercom leg for `id` (if it's the one currently active) — drops
    /// the second mic capture and tells the student the private channel is closed.
    /// Leaves plain listen-in (`listening_to`) alone; the two are independent once
    /// intercom has started.
    fn stop_intercom(&mut self, id: StudentId) {
        self.intercom = None;
        let mut guard = self.state.lock().unwrap();
        if guard.talking_to == Some(id) {
            guard.talking_to = None;
        }
        if let Some(s) = guard.students.get(&id) {
            let _ = s.to_client.send(ServerToClient::StopIntercom);
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
                test_score: None,
            });
            let _ = s.to_client.send(ServerToClient::AssignmentOffer {
                id: assignment_id,
                title: title.to_string(),
                kind,
                content: None,
            });
        }
    }

    /// Sends an authored assignment template (Test/Listening/Reading) — same
    /// recipient rule as `send_assignment` (current selection, or the whole class
    /// if nothing's selected) — with its real content attached this time.
    fn send_assignment_template(&mut self, template_id: i64) {
        let mut guard = self.state.lock().unwrap();
        let Some(template) = guard.assignment_templates.iter().find(|t| t.id == template_id) else {
            return;
        };
        let kind = template.kind;
        let title = template.title.clone();
        let content = match kind {
            AssignmentKind::Test => lingua_common::AssignmentContent::Test {
                questions: template
                    .questions
                    .iter()
                    .map(|q| lingua_common::TestQuestion {
                        text: q.text.clone(),
                        options: q.options.clone(),
                        correct_index: q.correct_index.unwrap_or(0),
                    })
                    .collect(),
            },
            AssignmentKind::Listening => {
                let material_title = template
                    .material_id
                    .and_then(|mid| guard.materials.iter().find(|m| m.id == mid))
                    .map(|m| m.title.clone())
                    .unwrap_or_else(|| "материал удалён".to_string());
                lingua_common::AssignmentContent::Listening {
                    material_title,
                    questions: template.questions.iter().map(|q| q.text.clone()).collect(),
                }
            }
            _ => lingua_common::AssignmentContent::Reading {
                text: template.reading_text.clone().unwrap_or_default(),
            },
        };

        let targets = self.action_targets(&guard);
        for id in targets {
            let student_db_id = match guard.students.get(&id) {
                Some(s) => s.db_id,
                None => continue,
            };
            let assignment_db_id =
                student_db_id.and_then(|row_id| db::insert_assignment(&guard.db, row_id, &title, kind).ok());

            let Some(s) = guard.students.get_mut(&id) else { continue };
            let assignment_id = Uuid::new_v4();
            s.assignments.push(state::AssignmentInstance {
                id: assignment_id,
                title: title.clone(),
                kind,
                done: false,
                db_id: assignment_db_id,
                test_score: None,
            });
            let _ = s.to_client.send(ServerToClient::AssignmentOffer {
                id: assignment_id,
                title: title.clone(),
                kind,
                content: Some(content.clone()),
            });
        }
    }

    /// Saves the current editor draft as a new assignment template and clears it.
    /// No-ops (leaves the draft alone) if the title's empty or a Test has no
    /// answerable questions — cheap validation, not full form feedback.
    fn save_assignment_draft(&mut self) {
        let draft = &self.assignment_draft;
        if draft.title.trim().is_empty() {
            return;
        }
        let (kind, reading_text, material_id, questions): (AssignmentKind, Option<String>, Option<i64>, Vec<db::NewQuestion>) =
            match draft.kind {
                EditorKind::Test => {
                    let questions: Vec<db::NewQuestion> = draft
                        .questions
                        .iter()
                        .filter(|q| !q.text.trim().is_empty() && q.options.iter().any(|o| !o.trim().is_empty()))
                        .map(|q| db::NewQuestion {
                            text: q.text.clone(),
                            options: q.options.clone(),
                            correct_index: Some(q.correct_index.min(q.options.len().saturating_sub(1))),
                        })
                        .collect();
                    if questions.is_empty() {
                        return;
                    }
                    (AssignmentKind::Test, None, None, questions)
                }
                EditorKind::Listening => {
                    let questions: Vec<db::NewQuestion> = draft
                        .questions
                        .iter()
                        .filter(|q| !q.text.trim().is_empty())
                        .map(|q| db::NewQuestion {
                            text: q.text.clone(),
                            options: Vec::new(),
                            correct_index: None,
                        })
                        .collect();
                    (AssignmentKind::Listening, None, draft.material_id, questions)
                }
                EditorKind::Reading => {
                    if draft.reading_text.trim().is_empty() {
                        return;
                    }
                    (AssignmentKind::Pronunciation, Some(draft.reading_text.clone()), None, Vec::new())
                }
            };

        let mut guard = self.state.lock().unwrap();
        let title = draft.title.clone();
        match db::insert_assignment_template(&mut guard.db, kind, &title, reading_text.as_deref(), material_id, &questions) {
            Ok(id) => {
                let template = db::AssignmentTemplate {
                    id,
                    kind,
                    title,
                    reading_text,
                    material_id,
                    questions: questions
                        .into_iter()
                        .map(|q| db::TemplateQuestion {
                            text: q.text,
                            options: q.options,
                            correct_index: q.correct_index,
                        })
                        .collect(),
                };
                guard.assignment_templates.insert(0, template);
                drop(guard);
                self.assignment_draft.reset();
            }
            Err(e) => tracing::warn!("failed to save assignment template: {e:#}"),
        }
    }

    fn add_roster_student(&mut self) {
        let name = self.roster_input.trim().to_string();
        if name.is_empty() {
            return;
        }
        let view_class_id = self.roster_view_class_id;
        let mut guard = self.state.lock().unwrap();
        let class_name = match db::list_classes(&guard.db)
            .ok()
            .and_then(|classes| classes.into_iter().find(|c| c.id == view_class_id))
        {
            Some(c) => c.name,
            None => {
                tracing::warn!("failed to save roster student '{name}': unknown class {view_class_id}");
                return;
            }
        };
        match db::insert_roster_student(&guard.db, view_class_id, &class_name, &name) {
            Ok(id) => {
                // The live connection-matching cache only ever reflects the
                // active lesson's class — only sync it when the teacher is
                // viewing that same class, otherwise it'd bleed another
                // class's roster into student-connection matching.
                if view_class_id == guard.current_class_id {
                    guard.roster.push(db::RosterEntry { id, full_name: name });
                }
                drop(guard);
                self.roster_input.clear();
            }
            Err(e) => tracing::warn!("failed to save roster student '{name}': {e:#}"),
        }
    }

    fn save_roster_rename(&mut self) {
        let Some((id, name)) = self.roster_edit.take() else { return };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let mut guard = self.state.lock().unwrap();
        if let Err(e) = db::rename_roster_student(&guard.db, id, &name) {
            tracing::warn!("failed to rename roster student: {e:#}");
            return;
        }
        if let Some(entry) = guard.roster.iter_mut().find(|r| r.id == id) {
            entry.full_name = name;
        }
    }

    fn delete_roster_student(&mut self, id: i64) {
        let mut guard = self.state.lock().unwrap();
        if let Err(e) = db::delete_roster_student(&guard.db, id) {
            tracing::warn!("failed to delete roster student: {e:#}");
            return;
        }
        guard.roster.retain(|r| r.id != id);
    }

    /// Dismisses the "not on the roster" warning for `id` — purely acknowledges
    /// it for the teacher's own bookkeeping; the connection was never at risk.
    fn accept_as_guest(&mut self, id: StudentId) {
        let mut guard = self.state.lock().unwrap();
        if let Some(s) = guard.students.get_mut(&id) {
            s.roster_status = state::RosterStatus::AcceptedGuest;
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

        // If the student we were privately talking to disconnected, net.rs already
        // cleared `talking_to` — drop our side of the intercom (mic capture + send
        // task) to match instead of leaving it running against a dead address.
        if self.intercom.is_some() && self.state.lock().unwrap().talking_to.is_none() {
            self.intercom = None;
        }

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

        if let Some((name, class_id)) = self.history_card.clone() {
            self.history_card_view(ctx, &name, class_id);
        } else if self.tab == Tab::Stats {
            self.stats_tab(ctx);
        } else if self.tab == Tab::Materials {
            self.materials_tab(ctx);
        } else if self.tab == Tab::Assignments {
            self.assignments_tab(ctx);
        } else if self.tab == Tab::Roster {
            self.roster_tab(ctx);
        } else if self.tab == Tab::ConnectionLog {
            self.connection_log_tab(ctx);
        } else if self.tab == Tab::Settings {
            self.settings_tab(ctx);
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
                ui.selectable_value(&mut self.tab, Tab::Materials, "Материалы");
                ui.selectable_value(&mut self.tab, Tab::Assignments, "Задания");
                ui.selectable_value(&mut self.tab, Tab::Roster, "Список класса");
                ui.selectable_value(&mut self.tab, Tab::ConnectionLog, "Журнал");
                ui.selectable_value(&mut self.tab, Tab::Settings, "⚙ Настройки");

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

                ui.add_space(8.0);
                let demoing_own_screen = matches!(
                    self.state.lock().unwrap().screen_demo.as_ref().map(|d| d.source),
                    Some(state::ScreenDemoSource::Teacher)
                );
                let demo_label = if demoing_own_screen { "⏹ Остановить демонстрацию" } else { "🖥 Показать мой экран" };
                if ui.button(demo_label).clicked() {
                    self.toggle_own_screen_demo();
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
            let (name, locked, test_mode, test_violations, listening, talking, demoing_this_student, roster_status) = {
                let guard = self.state.lock().unwrap();
                let demoing_this_student = matches!(
                    guard.screen_demo.as_ref().map(|d| d.source),
                    Some(state::ScreenDemoSource::Student(sid)) if sid == id
                );
                match guard.students.get(&id) {
                    Some(s) => (
                        s.name.clone(),
                        s.locked,
                        s.test_mode,
                        s.test_violations,
                        guard.listening_to == Some(id),
                        guard.talking_to == Some(id),
                        demoing_this_student,
                        s.roster_status,
                    ),
                    None => (String::new(), false, false, 0, false, false, false, state::RosterStatus::Matched),
                }
            };
            ui.strong(&name);
            if roster_status == state::RosterStatus::UnrecognizedPending {
                ui.horizontal(|ui| {
                    ui.colored_label(theme::WARN, "⚠ Не найден(а) в списке класса");
                    if ui.button("Принять как гостя").clicked() {
                        self.accept_as_guest(id);
                    }
                });
            } else if roster_status == state::RosterStatus::AcceptedGuest {
                ui.colored_label(theme::MUTED, "Гость (принят вручную)");
            }
            ui.horizontal(|ui| {
                let btn_label = if locked { "Разблокировать" } else { "Заблокировать" };
                if ui.button(btn_label).clicked() {
                    self.set_locked(id, !locked, false);
                }
                if !locked {
                    if ui.button("📝 Начать тест").on_hover_text("Блокировка + мониторинг переключений на другие приложения").clicked() {
                        self.set_locked(id, true, true);
                    }
                }
                let listen_label = if listening { "Прекратить" } else { "🎧 Слушать" };
                if ui.button(listen_label).clicked() {
                    self.toggle_listen(id);
                }
                if listening {
                    theme::wave_meter(ui, listen::LISTEN_LEVEL_MILLIS.load(Ordering::Relaxed));
                }
            });
            ui.horizontal(|ui| {
                let talk_label = if talking { "🔴 Завершить связь" } else { "🎙️ Говорить с учеником" };
                let button = egui::Button::new(talk_label).fill(if talking {
                    theme::DANGER.linear_multiply(0.35)
                } else {
                    ui.style().visuals.widgets.inactive.bg_fill
                });
                if ui.add(button).clicked() {
                    self.toggle_intercom(id);
                }
                if talking {
                    theme::wave_meter(ui, mic::INTERCOM_MIC_LEVEL_MILLIS.load(Ordering::Relaxed));
                }
            });
            if talking {
                ui.colored_label(theme::MUTED, "Приватный разговор — не слышен остальному классу");
            }
            ui.horizontal(|ui| {
                let demo_label = if demoing_this_student { "⏹ Остановить демонстрацию" } else { "🖥 Показать классу" };
                if ui.button(demo_label).on_hover_text("Показать экран этого ученика остальному классу").clicked() {
                    self.toggle_student_screen_demo(id);
                }
            });
            if demoing_this_student {
                ui.colored_label(theme::MUTED, "Экран этого ученика виден остальному классу");
            }
            if test_mode {
                let (color, text) = if test_violations > 0 {
                    (theme::DANGER, format!("⚠️ Переключений на другое приложение: {test_violations}"))
                } else {
                    (theme::OK, "Тестовый режим: переключений пока не было".to_string())
                };
                ui.colored_label(color, text);
            }
            // Same incoming pipeline serves plain listen-in and intercom's "hear
            // the student" leg (intercom always implies listening) — one slider
            // covers both, since it's really "how loud is what I'm hearing right now".
            if listening || talking {
                let mut gain_percent = listen::LISTEN_GAIN_PERCENT.load(Ordering::Relaxed);
                if ui
                    .add(egui::Slider::new(&mut gain_percent, 0..=200).text("Громкость").suffix("%"))
                    .changed()
                {
                    listen::LISTEN_GAIN_PERCENT.store(gain_percent, Ordering::Relaxed);
                }
            }
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

        let (class_size, seats, mut waiting_names): (usize, HashMap<usize, StudentId>, std::collections::VecDeque<String>) = {
            let guard = self.state.lock().unwrap();
            (
                guard.class_size,
                guard.students.iter().map(|(id, s)| (s.seat, *id)).collect(),
                guard.waiting_roster_names().into(),
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
                        let waiting_name = if occupant.is_none() { waiting_names.pop_front() } else { None };
                        let response = self.seat_card(ui, seat, occupant, waiting_name, card_width, card_h);
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
        waiting_name: Option<String>,
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
                            if let Some(name) = &waiting_name {
                                ui.vertical_centered(|ui| {
                                    ui.colored_label(theme::MUTED, name);
                                    ui.colored_label(theme::MUTED, "ожидание");
                                });
                            } else {
                                let (label, color) = presence_label_color(Presence::Empty);
                                ui.colored_label(color, label);
                            }
                        });
                    }
                    Some(id) => {
                        let (name, group, needs_help, presence, level_active, mic_level, test_badge, unrecognized) = {
                            let guard = self.state.lock().unwrap();
                            match guard.students.get(&id) {
                                Some(s) => {
                                    let presence = s.presence();
                                    let level_active = presence == Presence::Speaking;
                                    // Only show a live level while it's fresh (same recency
                                    // rule `presence()` uses for "Speaking") — otherwise a
                                    // stale reading (e.g. after a network hiccup) would just
                                    // sit there looking like a frozen VU meter.
                                    let mic_level = if level_active { s.last_level } else { 0 };
                                    let test_badge = s.test_mode.then_some(s.test_violations);
                                    let unrecognized = s.roster_status == state::RosterStatus::UnrecognizedPending;
                                    (s.name.clone(), s.group.is_some(), s.needs_help, presence, level_active, mic_level, test_badge, unrecognized)
                                }
                                None => return,
                            }
                        };
                        ui.horizontal(|ui| {
                            ui.colored_label(theme::MUTED, format!("#{seat}"));
                            if group {
                                ui.colored_label(theme::ACCENT, "🔗");
                            }
                            if unrecognized {
                                ui.colored_label(theme::WARN, "❓").on_hover_text("Не найден(а) в списке класса");
                            }
                            if let Some(violations) = test_badge {
                                let color = if violations > 0 { theme::DANGER } else { theme::WARN };
                                let text = if violations > 0 { format!("📝⚠️{violations}") } else { "📝".to_string() };
                                ui.colored_label(color, text).on_hover_text(if violations > 0 {
                                    format!("Тестовый режим — переключений на другое приложение: {violations}")
                                } else {
                                    "Тестовый режим — переключений пока не было".to_string()
                                });
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
                                theme::wave_meter(ui, mic_level);
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
                    self.set_locked(id, !locked, false);
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
            let (avg_score, done_total, connected, class_size, attendance, history, roster_nonempty, current_class_id) = {
                let guard = self.state.lock().unwrap();
                (
                    guard.average_score(),
                    guard.assignments_completed_total(),
                    guard.connected_count(),
                    guard.class_size.max(1),
                    guard.ever_connected_seats.len(),
                    guard.history,
                    !guard.roster.is_empty(),
                    guard.current_class_id,
                )
            };

            ui.horizontal(|ui| {
                ui.heading("Статистика");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(roster_nonempty, egui::Button::new("📥 Экспорт всего класса"))
                        .on_hover_text("Выгрузить историю всех учеников списка класса в один CSV-файл")
                        .clicked()
                    {
                        self.export_class_history();
                    }
                });
            });
            ui.add_space(6.0);

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

            type Row = (usize, StudentId, String, Presence, usize, Vec<(String, AssignmentKind, bool, Option<(u32, u32)>)>, Option<u32>);
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
                            s.assignments.iter().map(|a| (a.title.clone(), a.kind, a.done, a.test_score)).collect(),
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
                        if ui
                            .add(egui::Button::new(&name).frame(false))
                            .on_hover_text("Открыть историю по урокам")
                            .clicked()
                        {
                            self.history_card = Some((name.clone(), current_class_id));
                        }
                        let (label, color) = presence_label_color(presence);
                        ui.colored_label(color, label);
                        let total = assignments.len();
                        ui.label(format!("{done}/{total}"))
                            .on_hover_ui(|ui| {
                                if assignments.is_empty() {
                                    ui.colored_label(theme::MUTED, "Заданий пока не отправлено");
                                }
                                for (title, kind, is_done, test_score) in &assignments {
                                    ui.horizontal(|ui| {
                                        ui.colored_label(
                                            if *is_done { theme::OK } else { egui::Color32::GRAY },
                                            if *is_done { "✔" } else { "…" },
                                        );
                                        ui.colored_label(assignment_kind_color(*kind), kind.label());
                                        ui.label(title);
                                        if let Some((correct, total)) = test_score {
                                            ui.colored_label(theme::MUTED, format!("({correct}/{total})"));
                                        }
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

    fn materials_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Аудиоматериалы");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📂 Загрузить файл (mp3/wav)").clicked() {
                        self.upload_material();
                    }
                });
            });
            ui.add_space(6.0);
            ui.colored_label(
                theme::MUTED,
                "Воспроизведение идёт по тому же каналу, что и живой микрофон: выберите учеников в разделе «Класс», чтобы включить только им — иначе прозвучит всему классу.",
            );
            ui.add_space(12.0);

            let (materials, playing): (Vec<(i64, String)>, Option<(i64, String, u64, u64)>) = {
                let guard = self.state.lock().unwrap();
                (
                    guard.materials.iter().map(|m| (m.id, m.title.clone())).collect(),
                    guard
                        .playing
                        .as_ref()
                        .map(|p| (p.material_id, p.title.clone(), p.elapsed_ms, p.total_ms)),
                )
            };

            if let Some((_, title, elapsed_ms, total_ms)) = &playing {
                egui::Frame::group(ui.style()).rounding(egui::Rounding::same(10.0)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(theme::ACCENT, format!("▶ Сейчас играет: {title}"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("⏹ Остановить").clicked() {
                                self.stop_playback();
                            }
                        });
                    });
                    let progress = if *total_ms > 0 {
                        (*elapsed_ms as f32 / *total_ms as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(progress)
                            .text(format!("{} / {}", format_timer(Duration::from_millis(*elapsed_ms)), format_timer(Duration::from_millis(*total_ms)))),
                    );
                });
                ui.add_space(12.0);
            }

            if materials.is_empty() {
                ui.colored_label(theme::MUTED, "Материалов пока нет — загрузите mp3 или wav файл.");
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                for (id, title) in materials {
                    ui.horizontal(|ui| {
                        ui.label(&title);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let is_playing = playing.as_ref().map(|p| p.0) == Some(id);
                            let label = if is_playing { "⏹ Остановить" } else { "▶ Воспроизвести" };
                            if ui.button(label).clicked() {
                                if is_playing {
                                    self.stop_playback();
                                } else {
                                    self.play_material(id);
                                }
                            }
                        });
                    });
                    ui.separator();
                }
            });
        });
    }

    fn assignments_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Библиотека заданий");
                ui.colored_label(
                    theme::MUTED,
                    "Отправка — выберите учеников в разделе «Класс», иначе задание уйдёт всему классу.",
                );
                ui.add_space(10.0);

                let templates: Vec<(i64, AssignmentKind, String)> = {
                    let guard = self.state.lock().unwrap();
                    guard.assignment_templates.iter().map(|t| (t.id, t.kind, t.title.clone())).collect()
                };

                if templates.is_empty() {
                    ui.colored_label(theme::MUTED, "Пока нет ни одного созданного задания — соберите его ниже.");
                }
                let mut to_send = None;
                for (id, kind, title) in &templates {
                    ui.horizontal(|ui| {
                        let color = assignment_kind_color(*kind);
                        egui::Frame::none()
                            .fill(color.linear_multiply(0.25))
                            .rounding(egui::Rounding::same(6.0))
                            .inner_margin(egui::Margin::symmetric(6.0, 1.0))
                            .show(ui, |ui| {
                                ui.colored_label(color, kind.label());
                            });
                        ui.label(title);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("📤 Отправить").clicked() {
                                to_send = Some(*id);
                            }
                        });
                    });
                }
                if let Some(id) = to_send {
                    self.send_assignment_template(id);
                }

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);
                ui.heading("Создать задание");

                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.assignment_draft.kind, EditorKind::Test, "Тест");
                    ui.selectable_value(&mut self.assignment_draft.kind, EditorKind::Listening, "Аудирование");
                    ui.selectable_value(&mut self.assignment_draft.kind, EditorKind::Reading, "Чтение / произношение");
                });
                ui.add_space(6.0);
                ui.label("Название:");
                ui.add(egui::TextEdit::singleline(&mut self.assignment_draft.title).desired_width(400.0));
                ui.add_space(10.0);

                match self.assignment_draft.kind {
                    EditorKind::Test => self.test_editor(ui),
                    EditorKind::Listening => self.listening_editor(ui),
                    EditorKind::Reading => {
                        ui.label("Текст для чтения:");
                        ui.add(
                            egui::TextEdit::multiline(&mut self.assignment_draft.reading_text)
                                .desired_rows(6)
                                .desired_width(f32::INFINITY),
                        );
                    }
                }

                ui.add_space(10.0);
                if ui.button("💾 Сохранить задание").clicked() {
                    self.save_assignment_draft();
                }
            });
        });
    }

    fn test_editor(&mut self, ui: &mut egui::Ui) {
        let draft = &mut self.assignment_draft;
        for (qi, q) in draft.questions.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Вопрос {}:", qi + 1));
                    ui.add(egui::TextEdit::singleline(&mut q.text).desired_width(320.0));
                });
                for (oi, opt) in q.options.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut q.correct_index, oi, "").on_hover_text("Правильный вариант");
                        ui.add(egui::TextEdit::singleline(opt).desired_width(260.0));
                    });
                }
                if ui.small_button("+ Вариант ответа").clicked() {
                    q.options.push(String::new());
                }
            });
        }
        if ui.button("+ Добавить вопрос").clicked() {
            draft.questions.push(DraftQuestion::default());
        }
    }

    fn listening_editor(&mut self, ui: &mut egui::Ui) {
        let materials: Vec<(i64, String)> = {
            let guard = self.state.lock().unwrap();
            guard.materials.iter().map(|m| (m.id, m.title.clone())).collect()
        };
        let draft = &mut self.assignment_draft;

        ui.label("Материал:");
        let selected_label = draft
            .material_id
            .and_then(|id| materials.iter().find(|(mid, _)| *mid == id))
            .map(|(_, title)| title.clone())
            .unwrap_or_else(|| "— выберите материал —".to_string());
        egui::ComboBox::new("listening_material_picker", "")
            .selected_text(selected_label)
            .show_ui(ui, |ui| {
                for (id, title) in &materials {
                    ui.selectable_value(&mut draft.material_id, Some(*id), title);
                }
            });
        if materials.is_empty() {
            ui.colored_label(theme::MUTED, "Материалов пока нет — загрузите их во вкладке «Материалы».");
        }
        ui.add_space(10.0);
        ui.label("Вопросы по материалу (без автопроверки — ученик просто отмечает задание выполненным):");
        for (qi, q) in draft.questions.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{}.", qi + 1));
                ui.add(egui::TextEdit::singleline(&mut q.text).desired_width(400.0));
            });
        }
        if ui.button("+ Добавить вопрос").clicked() {
            draft.questions.push(DraftQuestion::default());
        }
    }

    fn roster_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Список класса");
            ui.colored_label(
                theme::MUTED,
                "Заранее заданный список ФИО — просто для порядка и статистики посещаемости: подключение \
                 с любым другим именем всё равно проходит (по PIN-коду), но учитель увидит мягкое предупреждение.",
            );
            ui.add_space(10.0);

            let classes = {
                let guard = self.state.lock().unwrap();
                db::list_classes(&guard.db).unwrap_or_default()
            };
            // The active lesson's class may not exist in `classes` yet only if the
            // DB was wiped out from under us mid-session — fall back to it anyway
            // so the picker always shows *something* selected.
            let current_name = classes
                .iter()
                .find(|c| c.id == self.roster_view_class_id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "Класс".to_string());

            ui.horizontal(|ui| {
                ui.label("Класс:");
                egui::ComboBox::from_id_salt("roster_class_picker")
                    .selected_text(&current_name)
                    .show_ui(ui, |ui| {
                        for class in &classes {
                            ui.selectable_value(&mut self.roster_view_class_id, class.id, &class.name);
                        }
                    });

                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.roster_new_class_name)
                        .desired_width(180.0)
                        .hint_text("название нового класса"),
                );
                let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("➕ Создать класс").clicked() || enter_pressed {
                    self.create_class();
                }
            });
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                let resp = ui.add(egui::TextEdit::singleline(&mut self.roster_input).desired_width(300.0).hint_text("Фамилия Имя"));
                let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("➕ Добавить").clicked() || enter_pressed {
                    self.add_roster_student();
                }
            });
            ui.add_space(14.0);

            let view_class_id = self.roster_view_class_id;
            let roster: Vec<(i64, String)> = {
                let guard = self.state.lock().unwrap();
                db::list_roster(&guard.db, view_class_id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|r| (r.id, r.full_name))
                    .collect()
            };
            if roster.is_empty() {
                ui.colored_label(theme::MUTED, "Список пока пуст.");
            }

            let mut to_delete = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (id, name) in &roster {
                    ui.horizontal(|ui| {
                        if self.roster_edit.as_ref().map(|(eid, _)| eid) == Some(id) {
                            let buf = &mut self.roster_edit.as_mut().unwrap().1;
                            let resp = ui.add(egui::TextEdit::singleline(buf).desired_width(280.0));
                            let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if resp.lost_focus() || enter_pressed {
                                self.save_roster_rename();
                            }
                        } else {
                            if ui
                                .add(egui::Button::new(name).frame(false))
                                .on_hover_text("Открыть историю по урокам")
                                .clicked()
                            {
                                self.history_card = Some((name.clone(), view_class_id));
                            }
                            if ui.small_button("✏️").on_hover_text("Переименовать").clicked() {
                                self.roster_edit = Some((*id, name.clone()));
                            }
                        }
                        if ui.small_button("🗑").on_hover_text("Удалить").clicked() {
                            to_delete = Some(*id);
                        }
                    });
                }
            });
            if let Some(id) = to_delete {
                self.delete_roster_student(id);
            }
        });
    }

    fn create_class(&mut self) {
        let name = self.roster_new_class_name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let guard = self.state.lock().unwrap();
        match db::insert_class(&guard.db, &name) {
            Ok(id) => {
                drop(guard);
                self.roster_view_class_id = id;
                self.roster_new_class_name.clear();
            }
            Err(e) => tracing::warn!("failed to create class '{name}': {e:#}"),
        }
    }

    /// Connection log for incident review — every successful connect, disconnect,
    /// and (the important one) rejected-wrong-PIN attempt. Last 200 rows, newest
    /// first, optionally restricted to the current lesson.
    fn connection_log_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Журнал подключений");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.log_filter_current_lesson, "Только текущий урок");
                });
            });
            ui.colored_label(
                theme::MUTED,
                "Последние 200 записей, сначала новые. Особое внимание — попытки с неверным PIN: \
                 повторяющиеся с одного имени/IP могут значить, что кто-то подбирает код урока.",
            );
            ui.add_space(10.0);

            let entries: Vec<db::ConnectionLogEntry> = {
                let guard = self.state.lock().unwrap();
                let lesson_filter = self.log_filter_current_lesson.then_some(guard.lesson_row_id);
                db::list_connection_log(&guard.db, lesson_filter, 200).unwrap_or_default()
            };

            if entries.is_empty() {
                ui.colored_label(theme::MUTED, "Записей пока нет.");
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("connection_log_table").num_columns(4).striped(true).min_col_width(100.0).show(ui, |ui| {
                    ui.colored_label(theme::MUTED, "ВРЕМЯ");
                    ui.colored_label(theme::MUTED, "УЧЕНИК");
                    ui.colored_label(theme::MUTED, "IP");
                    ui.colored_label(theme::MUTED, "СОБЫТИЕ");
                    ui.end_row();

                    for entry in &entries {
                        ui.label(format_epoch_date(entry.at));
                        ui.label(&entry.name_raw);
                        ui.label(&entry.ip);
                        let (label, color) = event_label_color(&entry.event);
                        ui.colored_label(color, label);
                        ui.end_row();
                    }
                });
            });
        });
    }

    /// Local, per-machine preferences: audio devices, the screen-demo video
    /// quality ceiling, and (for now, a single-choice placeholder) UI
    /// language. Saved to disk immediately on any change.
    fn settings_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Настройки");
            ui.colored_label(
                theme::MUTED,
                "Сохраняются сразу. Выбор устройств ввода/вывода звука применяется при \
                 следующем запуске захвата/воспроизведения — например, при повторном нажатии \
                 «Транслировать», начале нового разговора с учеником, или следующем запуске \
                 приложения.",
            );
            ui.add_space(14.0);

            let mut changed = false;

            ui.group(|ui| {
                ui.set_width(440.0);
                ui.strong("Аудиоустройства");
                ui.add_space(6.0);

                ui.label("Микрофон:");
                let input_names = audio_devices::list_input_device_names();
                let current_input = self.settings.input_device.clone().unwrap_or_else(|| "Системное по умолчанию".to_string());
                egui::ComboBox::from_id_salt("settings_input_device").selected_text(current_input).show_ui(ui, |ui| {
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
                ui.add_space(8.0);

                ui.label("Устройство вывода:");
                let output_names = audio_devices::list_output_device_names();
                let current_output = self.settings.output_device.clone().unwrap_or_else(|| "Системное по умолчанию".to_string());
                egui::ComboBox::from_id_salt("settings_output_device").selected_text(current_output).show_ui(ui, |ui| {
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
            });

            ui.add_space(14.0);

            ui.group(|ui| {
                ui.set_width(440.0);
                ui.strong("Качество видео-трансляции");
                ui.add_space(6.0);
                ui.colored_label(
                    theme::MUTED,
                    "Это потолок — при нехватке производительности во время демонстрации \
                     автоматическая деградация всё равно может снижать качество дальше.",
                );
                ui.add_space(6.0);
                for quality in settings::VideoQuality::ALL {
                    if ui.radio_value(&mut self.settings.video_quality, quality, quality.label()).clicked() {
                        changed = true;
                    }
                }
            });

            ui.add_space(14.0);

            ui.group(|ui| {
                ui.set_width(440.0);
                ui.strong("Язык интерфейса");
                ui.add_space(6.0);
                egui::ComboBox::from_id_salt("settings_language").selected_text(self.settings.language.label()).show_ui(ui, |ui| {
                    if ui
                        .selectable_value(&mut self.settings.language, settings::Language::Russian, settings::Language::Russian.label())
                        .clicked()
                    {
                        changed = true;
                    }
                });
            });

            if changed {
                if let Err(e) = self.settings.save() {
                    tracing::warn!("failed to save settings: {e:#}");
                }
            }
        });
    }

    /// Opens a save dialog and writes `name`'s cross-lesson history as CSV. Runs
    /// its own `student_history` query rather than taking an already-fetched
    /// slice — only happens on a click, so the extra round-trip against a small
    /// local SQLite file isn't worth threading through the render call chain for.
    fn export_student_history(&mut self, name: &str, class_id: i64) {
        let normalized = db::normalize_name(name);
        let history = {
            let guard = self.state.lock().unwrap();
            db::student_history(&guard.db, &normalized, class_id).unwrap_or_default()
        };
        let safe_name = name.replace(['/', '\\', ':'], "_");
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("ученик_{safe_name}_статистика.csv"))
            .save_file()
        else {
            return;
        };
        if let Err(e) = csv_export::write_student_history(&path, &history) {
            tracing::warn!("failed to export student history to {path:?}: {e:#}");
        }
    }

    /// Opens a save dialog and writes every roster student's history into one
    /// CSV — the "Экспорт всего класса" button on the Stats tab. No-op if the
    /// roster is empty (nothing to export); the button is disabled in that case.
    fn export_class_history(&mut self) {
        let students: Vec<(String, Vec<db::LessonHistoryEntry>)> = {
            let guard = self.state.lock().unwrap();
            let class_id = guard.current_class_id;
            guard
                .roster
                .iter()
                .map(|r| {
                    let normalized = db::normalize_name(&r.full_name);
                    let history = db::student_history(&guard.db, &normalized, class_id).unwrap_or_default();
                    (r.full_name.clone(), history)
                })
                .collect()
        };
        if students.is_empty() {
            return;
        }
        let today_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("класс_статистика_{}.csv", format_epoch_date_ymd(today_epoch)))
            .save_file()
        else {
            return;
        };
        if let Err(e) = csv_export::write_class_history(&path, &students) {
            tracing::warn!("failed to export class history to {path:?}: {e:#}");
        }
    }

    /// Cross-lesson history for one student, matched by normalized name (the same
    /// comparison the roster check uses) — reachable from a click on a name in
    /// either the roster or stats tab. Simple time-ordered table, no charts.
    fn history_card_view(&mut self, ctx: &egui::Context, name: &str, class_id: i64) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← Назад").clicked() {
                    self.history_card = None;
                }
                ui.heading(format!("История: {name}"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📥 Экспорт в CSV").clicked() {
                        self.export_student_history(name, class_id);
                    }
                });
            });
            ui.add_space(10.0);

            let normalized = db::normalize_name(name);
            let history = {
                let guard = self.state.lock().unwrap();
                db::student_history(&guard.db, &normalized, class_id).unwrap_or_default()
            };

            if history.is_empty() {
                ui.colored_label(theme::MUTED, "Данных пока нет — ученик ещё не участвовал в уроках под этим именем.");
                return;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("history_table").num_columns(4).striped(true).min_col_width(100.0).show(ui, |ui| {
                    ui.colored_label(theme::MUTED, "ДАТА");
                    ui.colored_label(theme::MUTED, "КЛАСС");
                    ui.colored_label(theme::MUTED, "ОЦЕНКА");
                    ui.colored_label(theme::MUTED, "ТЕСТЫ");
                    ui.end_row();

                    for entry in &history {
                        ui.label(format_epoch_date(entry.started_at));
                        ui.label(&entry.class_name);
                        ui.label(entry.score.map(|v| format!("{v}%")).unwrap_or_else(|| "—".to_string()));
                        if entry.test_results.is_empty() {
                            ui.colored_label(theme::MUTED, "—");
                        } else {
                            ui.vertical(|ui| {
                                for tr in &entry.test_results {
                                    ui.label(format!("{}: {}/{}", tr.title, tr.correct, tr.total));
                                }
                            });
                        }
                        ui.end_row();
                    }
                });
            });
        });
    }
}

/// Formats a Unix timestamp as `DD.MM.YYYY HH:MM` with no timezone/date library —
/// this app avoids adding one anywhere, so this is the proleptic Gregorian
/// calendar conversion by hand (Howard Hinnant's well-known `civil_from_days`
/// algorithm; the same integer arithmetic `chrono` and others use internally).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// `pub(crate)`: also used by `csv_export` for the CSV's date column.
pub(crate) fn format_epoch_date(epoch_secs: i64) -> String {
    let days = epoch_secs.div_euclid(86400);
    let secs_of_day = epoch_secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let h = secs_of_day / 3600;
    let mi = (secs_of_day % 3600) / 60;
    format!("{d:02}.{m:02}.{y} {h:02}:{mi:02}")
}

/// Filesystem-safe date (no colons — Windows forbids them in filenames), for the
/// default "класс_статистика_ДАТА.csv" export filename.
fn format_epoch_date_ymd(epoch_secs: i64) -> String {
    let days = epoch_secs.div_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
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
    /// `class_id`/`class_name` come from the class-picker screen that runs before
    /// this — every lesson session is for exactly one class, chosen up front.
    pub fn launch(teacher_name: String, class_id: i64, class_name: String) -> Self {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

        let settings = settings::Settings::load();
        // Must happen before anything below queries an output sample rate or
        // starts the (lazily-started, only-once-per-process) output stream —
        // see `audio_devices::configure_output_device`'s doc comment.
        audio_devices::configure_output_device(settings.output_device.clone());

        let class_size: usize = std::env::var("VOCALIS_CLASS_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(16);
        let lesson_pin = std::env::var("VOCALIS_LESSON_PIN")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(state::generate_pin);

        let db_conn = db::open().expect("failed to open Vocalis database");
        let history = db::load_history_summary(&db_conn, class_id).unwrap_or_default();
        let lesson_row_id =
            db::insert_lesson(&db_conn, class_id, &class_name).expect("failed to record lesson start");
        let materials = db::list_materials(&db_conn).unwrap_or_default();
        let assignment_templates = db::list_assignment_templates(&db_conn).unwrap_or_default();
        let roster = db::list_roster(&db_conn, class_id).unwrap_or_default();

        let state: AppState = Arc::new(Mutex::new(SharedState::new(
            class_id,
            class_name,
            class_size,
            lesson_pin,
            db_conn,
            lesson_row_id,
            history,
            materials,
            assignment_templates,
            roster,
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
            let state = state.clone();
            let listen_queue = listen_queue.clone();
            rt.spawn(async move {
                if let Err(e) = listen::run_listen_receiver(state, listen_queue, listen_output_rate).await {
                    tracing::warn!("listen-in receiver stopped: {e:#}");
                }
            });
        }
        {
            let state = state.clone();
            rt.spawn(async move {
                if let Err(e) = screen::run_screen_relay_receiver(state).await {
                    tracing::warn!("screen-demo relay receiver stopped: {e:#}");
                }
            });
        }

        TeacherApp::new(state, rt, teacher_name, class_id, settings)
    }
}
