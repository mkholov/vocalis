import { useEffect, useRef } from "react";
import type { ToastKind } from "../components/Toast";
import type { LiveStudent } from "./useLiveClassroom";

/** Turns changes in the real connected-students list into toasts: a student raising their hand
 * (`presence` becoming "needsHelp"), and students joining. Only *edges* toast — a hand that stays up, or
 * a student who stays connected, does not re-toast on every `student-levels` tick.
 *
 * Runs at the teacher-console level (not in the class grid) so a raised hand shows up whichever tab
 * the teacher is on. */
export function useClassroomToasts(students: LiveStudent[], push: (kind: ToastKind, text: string) => void) {
  const before = useRef<{ ids: Set<string>; helping: Set<string> } | null>(null);

  useEffect(() => {
    const ids = new Set(students.map((s) => s.realId));
    const helping = new Set(students.filter((s) => s.presence === "needsHelp").map((s) => s.realId));
    const prev = before.current;
    before.current = { ids, helping };
    if (!prev) return;

    for (const s of students) {
      if (helping.has(s.realId) && !prev.helping.has(s.realId)) push("help", `${s.name} просит помощи`);
    }

    const joined = students.filter((s) => !prev.ids.has(s.realId));
    if (joined.length === 1) push("info", `${joined[0].name} подключился`);
    else if (joined.length > 1) push("info", `Подключились ученики: ${joined.length}`);
  }, [students, push]);
}
