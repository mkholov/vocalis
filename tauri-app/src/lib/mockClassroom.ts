import { useEffect, useState } from "react";

// Local-only simulated classroom state for step 4 of the Tauri migration
// (vocalis_roadmap.md, section 8) — no network, no real audio (that's step 7:
// "видео/аудио превью внутри веб-вью"). This hook exists purely to give the
// grid/VU-meter/timer something plausible-looking to animate against.

export type Presence = "empty" | "needsHelp" | "speaking" | "connected";

export interface MockStudent {
  id: number;
  seat: number;
  name: string;
  presence: Presence;
  level: number; // 0-100 mock VU level
  screenLocked: boolean;
  micLocked: boolean;
}

// Exported so other mock screens (stats, chat) can reference the same names
// without inventing a second roster.
export const ROSTER = ["Иванов Пётр", "Смирнова Анна", "Кузнецов Дмитрий", "Соколова Мария", "Попов Егор", "Волкова Дарья", "Новиков Илья", "Морозова Ксения"];

function clamp(v: number, min: number, max: number) {
  return Math.max(min, Math.min(max, v));
}

function initialStudents(seatCount: number): MockStudent[] {
  return Array.from({ length: seatCount }, (_, i) => {
    const seat = i + 1;
    if (i >= ROSTER.length) {
      return { id: seat, seat, name: "", presence: "empty", level: 0, screenLocked: false, micLocked: false };
    }
    const presence: Presence = i === 1 ? "needsHelp" : i === 2 ? "speaking" : "connected";
    return {
      id: seat,
      seat,
      name: ROSTER[i],
      presence,
      level: presence === "speaking" ? 55 : 0,
      screenLocked: false,
      micLocked: false,
    };
  });
}

/** Ticks every 180ms (matching the egui app's own `request_repaint_after`
 * cadence) — random-walks the level of anyone currently "speaking" and
 * occasionally flips a "connected" student into "speaking" and back, so the
 * grid looks like a live class instead of a static mock. */
export function useMockClassroom(seatCount = 12) {
  const [students, setStudents] = useState<MockStudent[]>(() => initialStudents(seatCount));

  useEffect(() => {
    const id = setInterval(() => {
      setStudents((prev) =>
        prev.map((s) => {
          if (s.presence === "empty" || s.presence === "needsHelp") return s;
          if (s.presence === "speaking") {
            const level = clamp(s.level + (Math.random() - 0.45) * 35, 8, 100);
            return Math.random() < 0.04 ? { ...s, presence: "connected", level: 0 } : { ...s, level };
          }
          // connected, idle
          return Math.random() < 0.03 ? { ...s, presence: "speaking", level: 30 } : s;
        }),
      );
    }, 180);
    return () => clearInterval(id);
  }, []);

  function toggleScreenLock(id: number) {
    setStudents((prev) => prev.map((s) => (s.id === id ? { ...s, screenLocked: !s.screenLocked } : s)));
  }

  function toggleMicLock(id: number) {
    setStudents((prev) => prev.map((s) => (s.id === id ? { ...s, micLocked: !s.micLocked } : s)));
  }

  return { students, toggleScreenLock, toggleMicLock };
}
