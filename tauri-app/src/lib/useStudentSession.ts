import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { connectStudentSession, disconnectStudentSession, type ScreenDemoFrameDto } from "./commands";

interface Params {
  teacherIp: string;
  controlPort: number;
  studentName: string;
  pin: string;
}

/** Real network connection to a teacher's session (step 7 part B —
 * `vocalis_roadmap.md`, section 8): connects on mount via
 * `commands/student_session.rs` (the same Hello/Welcome handshake the egui
 * student app uses) and listens for real decoded screen-demo frames — the
 * teacher's own screen, broadcast to the whole class, not a local capture.
 * `frame` is `null` whenever no demo is currently running (either none has
 * started yet, or the teacher just stopped one — `screen-demo-stopped`
 * clears it so a finished demo doesn't leave a frozen last frame on
 * screen). `error` is set on a genuine connection failure (wrong PIN,
 * unreachable teacher) rather than thrown. */
export function useStudentSession(params: Params) {
  const [frame, setFrame] = useState<ScreenDemoFrameDto | null>(null);
  const [error, setError] = useState<string | undefined>();
  const [teacherName, setTeacherName] = useState<string | undefined>();

  useEffect(() => {
    let cancelled = false;
    let unlistenFrame: (() => void) | undefined;
    let unlistenStopped: (() => void) | undefined;

    connectStudentSession(params.teacherIp, params.controlPort, params.studentName, params.pin)
      .then((info) => {
        if (cancelled) return undefined;
        setTeacherName(info.teacherName);
        return Promise.all([
          listen<ScreenDemoFrameDto>("screen-demo-frame", (event) => setFrame(event.payload)),
          listen("screen-demo-stopped", () => setFrame(null)),
        ]);
      })
      .then((fns) => {
        if (!fns) return;
        if (cancelled) {
          fns.forEach((fn) => fn());
        } else {
          [unlistenFrame, unlistenStopped] = fns;
        }
      })
      .catch((err) => setError(String(err)));

    return () => {
      cancelled = true;
      unlistenFrame?.();
      unlistenStopped?.();
      setFrame(null);
      disconnectStudentSession().catch(() => {});
    };
  }, [params.teacherIp, params.controlPort, params.studentName, params.pin]);

  return { frame, error, teacherName };
}
