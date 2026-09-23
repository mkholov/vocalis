// Typed wrappers around the real Tauri commands from step 2
// (src-tauri/src/commands/) — the shapes here must match those Rust DTOs
// exactly. No mocking here: these hit the real Rust core (SQLite, UDP
// discovery). What's mocked in this step is only what happens *after* a
// successful pick (see screens/*.tsx's onSubmit handlers).
import { invoke } from "@tauri-apps/api/core";

export interface ClassDto {
  id: number;
  name: string;
  /** Lessons held for this class so far — what deleting it would erase with it. */
  lessons: number;
  /** Names on the class's roster. */
  roster: number;
}

export function listClasses(): Promise<ClassDto[]> {
  return invoke<ClassDto[]>("list_classes");
}

/** Real `db::insert_class`. Rejects with a ready-to-show Russian message (empty or already-taken name). */
export function createClass(name: string): Promise<ClassDto> {
  return invoke<ClassDto>("create_class", { name });
}

/** Renames a class (`commands/db.rs`). Rejects with a ready-to-show message: empty or taken name, a lesson
 * of that class running, or the class already gone. */
export function renameClass(id: number, name: string): Promise<ClassDto> {
  return invoke<ClassDto>("rename_class", { id, name });
}

/** Deletes a class *and its history* (lessons, students, results, roster) in one transaction. The UI asks
 * for confirmation first. Rejects with a ready-to-show message, like `renameClass`. */
export function deleteClass(id: number): Promise<void> {
  return invoke<void>("delete_class", { id });
}

export interface DiscoveredTeacherDto {
  ip: string;
  teacherName: string;
  controlPort: number;
}

export function discoverTeachers(timeoutMs: number): Promise<DiscoveredTeacherDto[]> {
  return invoke<DiscoveredTeacherDto[]>("discover_teachers", { timeoutMs });
}

export interface AudioDevicesDto {
  inputDevices: string[];
  outputDevices: string[];
}

export function listAudioDevices(): Promise<AudioDevicesDto> {
  return invoke<AudioDevicesDto>("list_audio_devices");
}

// --- Step 7, part A (vocalis_roadmap.md, section 8): live mic levels ---
// commands/teacher_session.rs and commands/student_mic.rs.

export interface TeacherSessionInfo {
  pin: string;
  controlPort: number;
  className: string;
}

export function startTeacherSession(className: string): Promise<TeacherSessionInfo> {
  return invoke<TeacherSessionInfo>("start_teacher_session", { className });
}

export function stopTeacherSession(): Promise<void> {
  return invoke("stop_teacher_session");
}

/** One event payload entry from the `student-levels` event — a real,
 * currently-connected student's mic level, not a mock tick. */
export interface StudentLevelDto {
  id: string;
  name: string;
  level: number;
  secondsSinceReport: number;
  /** Step 7.5's groups/pairs: which group (if any) this student is
   * currently in, straight from the real `Student::group`. */
  group: number | null;
  /** Real "поднять руку" (`ClientToServer::RequestHelp`), straight from `Student::needs_help`. */
  needsHelp: boolean;
}

/** `deviceName`: which input to open; omitted (or a name that no longer exists) means the system default. */
export function startStudentMicMeter(deviceName?: string): Promise<void> {
  return invoke("start_student_mic_meter", { deviceName });
}

export function stopStudentMicMeter(): Promise<void> {
  return invoke("stop_student_mic_meter");
}

export interface MicLevelDto {
  level: number;
}

// --- Step 7, part B (vocalis_roadmap.md, section 8): live screen-demo video ---
// commands/screen_demo.rs (self-preview: this machine's own screen, into its
// own webview — used by TeacherClassGrid's "Превью своего экрана") and
// commands/student_session.rs (the real class-wide broadcast: teacher's own
// screen -> every connected student, over the real network). Both emit the
// identical `screen-demo-frame` event shape below.

export function startScreenDemo(): Promise<void> {
  return invoke("start_screen_demo");
}

export function stopScreenDemo(): Promise<void> {
  return invoke("stop_screen_demo");
}

/** One `screen-demo-frame` event payload — a real captured/H.264-encoded/
 * decoded/JPEG-re-encoded frame, base64-framed for direct use as an `<img
 * src>` (see video-bench's report for why base64/emit was chosen over
 * `tauri::ipc::Channel`). */
export interface ScreenDemoFrameDto {
  width: number;
  height: number;
  dataUrl: string;
}

export function startOwnScreenDemo(): Promise<{ targetCount: number }> {
  return invoke("start_own_screen_demo");
}

export function stopOwnScreenDemo(): Promise<void> {
  return invoke("stop_own_screen_demo");
}

export interface StudentSessionInfo {
  teacherName: string;
}

/** Connects for real to a teacher's control channel (same Hello/Welcome
 * handshake, same session-key derivation as the egui student app) and starts
 * the always-on decoded-frame receiver — idle until the teacher actually
 * starts a demo, at which point real frames start arriving as
 * `screen-demo-frame` events (see `commands/student_session.rs`). */
export function connectStudentSession(teacherIp: string, controlPort: number, studentName: string, pin: string): Promise<StudentSessionInfo> {
  return invoke<StudentSessionInfo>("connect_student_session", { teacherIp, controlPort, studentName, pin });
}

export function disconnectStudentSession(): Promise<void> {
  return invoke("disconnect_student_session");
}

/** Real "поднять руку" (`commands/student_session.rs`'s `set_hand_raised`): sends `ClientToServer::
 * RequestHelp` over this student's already-connected session. Rejects if there is no active connection
 * yet — the caller should keep its own local toggle state and revert it on a rejection. */
export function setHandRaised(raised: boolean): Promise<void> {
  return invoke("set_hand_raised", { raised });
}

// --- Step 7.5 (vocalis_roadmap.md, section 8): teacher's mic broadcast ---
// commands/teacher_session.rs. Reuses teacher::mic::{start_mic_capture,
// run_mic_broadcast} unchanged — same capture/resample/Opus-encode/UDP
// pipeline the egui teacher console's own mic toggle uses. The receiving
// side needs no frontend wiring at all: `connect_student_session` already
// starts the real mic-broadcast receiver, which plays through real speakers
// on its own (`student::audio::ensure_output_started`).

export function startMicBroadcast(): Promise<void> {
  return invoke("start_mic_broadcast");
}

export function stopMicBroadcast(): Promise<void> {
  return invoke("stop_mic_broadcast");
}

// --- Step 7.5 (vocalis_roadmap.md, section 8): listen in on a student ---
// commands/teacher_session.rs. Reuses `SharedState::start_listening`
// unchanged. `studentId` must be the real UUID (`StudentLevelDto.id` /
// `LiveStudent.realId`, not the synthetic numeric id `StudentCard` uses).
// The student's own outbound mic (real capture, started automatically by
// `connectStudentSession`) starts sending audio the instant the teacher's
// `ServerToClient::StartMicUpload` arrives — no frontend action needed on
// the student side.

export function startListen(studentId: string): Promise<void> {
  return invoke("start_listen", { studentId });
}

export function stopListen(): Promise<void> {
  return invoke("stop_listen");
}

// --- Step 7.5 (vocalis_roadmap.md, section 8): private teacher<->student intercom ---
// commands/teacher_session.rs. Reuses `teacher::mic::run_intercom_send`
// unchanged (a second, independent mic capture from the class-wide
// broadcast) plus `SharedState::start_listening` so the teacher hears the
// student back over the same mechanism plain listen-in uses. The student's
// side needs no frontend wiring: `connect_student_session` already starts
// the real intercom receiver, which plays through real speakers on its own.

export interface IntercomInfo {
  studentName: string;
}

export function startIntercom(studentId: string): Promise<IntercomInfo> {
  return invoke<IntercomInfo>("start_intercom", { studentId });
}

export function stopIntercom(): Promise<void> {
  return invoke("stop_intercom");
}

// --- Step 7.5 (vocalis_roadmap.md, section 8): groups/pairs ---
// commands/teacher_session.rs. Reuses `SharedState::create_group`/
// `leave_group` unchanged — the same methods the egui teacher console's
// drag-and-drop grouping UI calls, sending each member a real
// `ServerToClient::JoinGroup`/`LeaveGroup`. The student side needs no
// frontend wiring at all: `connect_student_session` already starts
// `student::audio::run_outbound_and_group_audio`, which starts sending/
// receiving real peer audio the moment `JoinGroup` arrives.

export interface CreateGroupInfo {
  memberCount: number;
}

export function createGroup(studentIds: string[]): Promise<CreateGroupInfo> {
  return invoke<CreateGroupInfo>("create_group", { studentIds });
}

export function leaveGroup(studentId: string): Promise<void> {
  return invoke("leave_group", { studentId });
}

// --- Step 7.5 (vocalis_roadmap.md, section 8): audio materials library ---
// commands/teacher_session.rs. Reuses `db::{insert_material, list_materials}`
// and `teacher::materials::{decode_to_mono_pcm, run_playback}` unchanged —
// the same decode/resample/Opus-encode/UDP pipeline the live mic broadcast
// uses, over the same MIC_PORT, so a receiving student needs no separate
// wiring: `connect_student_session`'s already-running mic-broadcast receiver
// picks it up on its own. `filePath` must be a real absolute path — get one
// via `@tauri-apps/plugin-dialog`'s `open()`, Tauri's native file picker.

export interface MaterialDto {
  id: number;
  title: string;
}

export function listMaterials(): Promise<MaterialDto[]> {
  return invoke<MaterialDto[]>("list_materials");
}

export function uploadMaterial(filePath: string, title: string): Promise<MaterialDto> {
  return invoke<MaterialDto>("upload_material", { filePath, title });
}

export interface PlaybackInfo {
  title: string;
  targetCount: number;
}

/** `studentIds` empty means "everyone currently connected" ("проиграть
 * всем"); non-empty plays only to those real students ("проиграть
 * выбранным"). Stops whatever was playing before, and stops a live mic
 * broadcast if one is running (same one-stream-per-MIC_PORT constraint
 * `startMicBroadcast` enforces the other way). */
export function playMaterial(materialId: number, studentIds: string[]): Promise<PlaybackInfo> {
  return invoke<PlaybackInfo>("play_material", { materialId, studentIds });
}

export function stopPlayback(): Promise<void> {
  return invoke("stop_playback");
}

// --- Step 7.5 (vocalis_roadmap.md, section 8): the student's own voice recordings ---
// commands/student_recording.rs. Reuses `student::recording` (save/list/delete) and
// the recording tap in `student::audio::run_outbound_and_group_audio` unchanged.
// Recordings are addressed by file `name` only, never by path — the backend refuses
// anything that isn't an entry of its own recordings directory. Comparing a
// recording to the teacher's reference and sending one to the teacher are not here.

export interface RecordingDto {
  /** File name (`recording_<epoch>.wav`) — the recording's id. */
  name: string;
  durationSecs: number;
  /** Unix seconds, parsed from the file name when it has the usual shape. */
  recordedAtEpoch: number | null;
}

/** Rejects with "микрофон недоступен" when this machine has no usable input device. */
export function startRecording(): Promise<void> {
  return invoke("start_recording");
}

/** Resolves to `null` if nothing was being recorded or nothing was captured. */
export function stopRecording(): Promise<RecordingDto | null> {
  return invoke<RecordingDto | null>("stop_recording");
}

export function listRecordings(): Promise<RecordingDto[]> {
  return invoke<RecordingDto[]>("list_recordings");
}

/** The recording's audio as a `data:audio/wav;base64,…` URL, for `new Audio(url)`. */
export function readRecording(name: string): Promise<string> {
  return invoke<string>("read_recording", { name });
}

export function deleteRecording(name: string): Promise<void> {
  return invoke("delete_recording", { name });
}
