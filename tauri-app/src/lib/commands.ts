// Typed wrappers around the real Tauri commands from step 2
// (src-tauri/src/commands/) — the shapes here must match those Rust DTOs
// exactly. No mocking here: these hit the real Rust core (SQLite, UDP
// discovery). What's mocked in this step is only what happens *after* a
// successful pick (see screens/*.tsx's onSubmit handlers).
import { invoke } from "@tauri-apps/api/core";

export interface ClassDto {
  id: number;
  name: string;
}

export function listClasses(): Promise<ClassDto[]> {
  return invoke<ClassDto[]>("list_classes");
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
}

export function startStudentMicMeter(): Promise<void> {
  return invoke("start_student_mic_meter");
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
