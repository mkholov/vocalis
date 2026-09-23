import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { startTeacherSession, stopTeacherSession, type StudentLevelDto } from "./commands";
import type { MockStudent } from "./mockClassroom";

// Same threshold `teacher::state::Student::presence()` uses to decide
// "Speaking" vs "Connected" from `last_level` — kept in sync by reference
// (see that function's `SPEAKING_THRESHOLD` constant) rather than re-derived.
const SPEAKING_THRESHOLD = 120;
// `Student::presence()` also treats a level as stale (drops back to
// "Connected") once its `last_level_at` is old enough — `run_level_telemetry`
// reports every 250ms, so anything much older than that means the student's
// last report just hasn't arrived yet, not that they're still speaking.
const STALE_AFTER_SECONDS = 1.5;

/** One `useLiveClassroom` student — `MockStudent` plus the real UUID
 * `StudentLevelDto.id` behind the synthetic numeric `id` (see this module's
 * doc comment for why that mapping exists). Step 7.5's `start_listen` needs
 * the real UUID string, not the numeric one `StudentCard` was already built
 * around. */
export interface LiveStudent extends MockStudent {
  realId: string;
  group: number | null;
}

/** Starts a real teacher session (step 2/7's `start_teacher_session`) on
 * mount and turns its `student-levels` events into `StudentCard`-shaped
 * students — only ever real, connected ones (empty until someone connects).
 * Real student IDs are UUIDs; `MockStudent.id` is a number, so this assigns
 * each newly-seen UUID a stable sequential number (kept in a ref) rather than
 * changing `StudentCard`'s prop type.
 *
 * Called once, by `TeacherConsole`, so the session lives as long as the
 * console does — not per tab (the class grid used to own it, which stopped
 * the session whenever the teacher switched to another tab).
 */
export function useLiveClassroom(className: string) {
  const [pin, setPin] = useState<string | null>(null);
  const [error, setError] = useState<string | undefined>();
  const [realStudents, setRealStudents] = useState<LiveStudent[]>([]);
  const idsRef = useRef(new Map<string, number>());
  const nextIdRef = useRef(1);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    startTeacherSession(className)
      .then((info) => {
        if (cancelled) return;
        setPin(info.pin);
        return listen<StudentLevelDto[]>("student-levels", (event) => {
          const mapped: LiveStudent[] = event.payload.map((s) => {
            let numericId = idsRef.current.get(s.id);
            if (numericId === undefined) {
              numericId = nextIdRef.current++;
              idsRef.current.set(s.id, numericId);
            }
            const fresh = s.secondsSinceReport < STALE_AFTER_SECONDS;
            const level = fresh ? s.level : 0;
            // Same priority as egui's `Student::presence()`: a raised hand outranks "speaking".
            const presence = s.needsHelp ? "needsHelp" : level >= SPEAKING_THRESHOLD ? "speaking" : "connected";
            return {
              id: numericId,
              realId: s.id,
              seat: numericId,
              name: s.name,
              presence,
              level,
              screenLocked: false,
              micLocked: false,
              group: s.group,
            };
          });
          setRealStudents(mapped);
        });
      })
      .then((fn) => {
        if (cancelled) fn?.();
        else unlisten = fn;
      })
      .catch((err) => setError(String(err)));

    return () => {
      cancelled = true;
      unlisten?.();
      stopTeacherSession().catch(() => {});
    };
  }, [className]);

  return { pin, error, realStudents };
}

/** What `useLiveClassroom` returns — the console hands this to the class grid. */
export type LiveClassroom = ReturnType<typeof useLiveClassroom>;
