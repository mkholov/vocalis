import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { startStudentMicMeter, stopStudentMicMeter, type MicLevelDto } from "./commands";

interface Options {
  /** Capture only while true. Default true: the student console meters from mount to unmount. */
  active?: boolean;
  /** Input to open — the Settings screen's selected microphone. Omitted: the system default. */
  deviceName?: string;
}

/** Real mic level for a VU meter (step 7, part A — `vocalis_roadmap.md`, section 8) — starts a real
 * `cpal` capture (`commands/student_mic.rs`, reusing `teacher::mic::start_mic_capture` unchanged) and
 * reports whatever this machine's actual microphone picks up. Used by the student console (always on) and
 * by the Settings "Проверить микрофон" test (`active` toggled by the button, `deviceName` = the chosen
 * input; changing either restarts the capture, and leaving the screen stops it).
 * `error` is set on a genuine capture failure (e.g. no input device — a real possibility on a CI runner,
 * not a bug) rather than thrown, since a screen without a working mic meter should still be usable. */
export function useMicMeter({ active = true, deviceName }: Options = {}) {
  const [level, setLevel] = useState(0);
  const [error, setError] = useState<string | undefined>();

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    setError(undefined);

    startStudentMicMeter(deviceName)
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
      setLevel(0);
    };
  }, [active, deviceName]);

  return { level, error };
}
