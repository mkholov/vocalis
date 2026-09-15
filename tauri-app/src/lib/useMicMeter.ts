import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { startStudentMicMeter, stopStudentMicMeter, type MicLevelDto } from "./commands";

/** Real mic level for the student's own VU meter (step 7, part A —
 * `vocalis_roadmap.md`, section 8) — starts a real `cpal` capture on mount
 * (`commands/student_mic.rs`, reusing `teacher::mic::start_mic_capture`
 * unchanged) and reports whatever this machine's actual microphone picks up.
 * `error` is set on a genuine capture failure (e.g. no input device — a real
 * possibility on a CI runner, not a bug) rather than thrown, since a student
 * console without a working mic meter should still be usable. */
export function useMicMeter() {
  const [level, setLevel] = useState(0);
  const [error, setError] = useState<string | undefined>();

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    startStudentMicMeter()
      .then(() => (cancelled ? undefined : listen<MicLevelDto>("mic-level", (event) => setLevel(event.payload.level))))
      .then((fn) => {
        if (cancelled) fn?.();
        else unlisten = fn;
      })
      .catch((err) => setError(String(err)));

    return () => {
      cancelled = true;
      unlisten?.();
      stopStudentMicMeter().catch(() => {});
    };
  }, []);

  return { level, error };
}
