// Shared shapes for the class grid (`MockStudent` is the shape `LiveStudent`/`StudentCard` are built on —
// the name is historical) plus `ROSTER`, the fake names the still-mock Stats/Chat/student-console screens
// use. The old `useMockClassroom` simulation is gone: the class grid now shows only real students.

export type Presence = "empty" | "needsHelp" | "speaking" | "connected";

export interface MockStudent {
  id: number;
  seat: number;
  name: string;
  presence: Presence;
  level: number; // VU level (raw mic level as reported by the student)
  screenLocked: boolean;
  micLocked: boolean;
}

// Exported so other mock screens (stats, chat) can reference the same names
// without inventing a second roster.
export const ROSTER = ["Иванов Пётр", "Смирнова Анна", "Кузнецов Дмитрий", "Соколова Мария", "Попов Егор", "Волкова Дарья", "Новиков Илья", "Морозова Ксения"];
