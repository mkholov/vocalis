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
// commands/screen_demo.rs. Self-preview only (this machine's own screen, into
// its own webview) — see that file's doc comment for why: the class-wide
// network relay (teacher/presenting-student -> every student) isn't wired
// into the Tauri layer yet.

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
