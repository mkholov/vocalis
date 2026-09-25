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
 * unreachable teacher) rather than thrown.
 *
 * `disconnected` starts `false` and only ever goes `true`, on a real `"teacher-disconnected"` event —
 * `commands/student_session.rs`'s own watcher, which fires once the *real* control connection actually
 * closes (the teacher ended the lesson, quit, or the network died), not a guess. Before that event
 * existed, nothing told this hook (or `StudentConsole`) a stale "подключено к …" was no longer true. */
export function useStudentSession(params: Params) {
  const [frame, setFrame] = useState<ScreenDemoFrameDto | null>(null);
  const [error, setError] = useState<string | undefined>();
  const [teacherName, setTeacherName] = useState<string | undefined>();
  const [disconnected, setDisconnected] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let unlistenFrame: (() => void) | undefined;
    let unlistenStopped: (() => void) | undefined;
    let unlistenDisconnected: (() => void) | undefined;
    setDisconnected(false);

    connectStudentSession(params.teacherIp, params.controlPort, params.studentName, params.pin)
      .then((info) => {
        if (cancelled) return undefined;
        setTeacherName(info.teacherName);
        return Promise.all([
          listen<ScreenDemoFrameDto>("screen-demo-frame", (event) => setFrame(event.payload)),
          listen("screen-demo-stopped", () => setFrame(null)),
          listen("teacher-disconnected", () => setDisconnected(true)),
        ]);
      })
      .then((fns) => {
        if (!fns) return;
        if (cancelled) {
          fns.forEach((fn) => fn());
        } else {
          [unlistenFrame, unlistenStopped, unlistenDisconnected] = fns;
        }
      })
      .catch((err) => setError(String(err)));

    return () => {
      cancelled = true;
      unlistenFrame?.();
      unlistenStopped?.();
      unlistenDisconnected?.();
      setFrame(null);
      disconnectStudentSession().catch(() => {});
    };
  }, [params.teacherIp, params.controlPort, params.studentName, params.pin]);

  return { frame, error, teacherName, disconnected };
}
