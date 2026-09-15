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
