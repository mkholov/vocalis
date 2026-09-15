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
