import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { StudentCard } from "../components/StudentCard";
import { LessonTimer } from "../components/LessonTimer";
import { Button } from "../components/ui/Button";
import { useMockClassroom } from "../lib/mockClassroom";
import { useLiveClassroom } from "../lib/useLiveClassroom";
import { useScreenDemo } from "../lib/useScreenDemo";
import { startOwnScreenDemo, stopOwnScreenDemo, startMicBroadcast, stopMicBroadcast } from "../lib/commands";

interface Props {
  className: string;
  onEnd: () => void;
}

/** Step 4 (grid/timer/lock UI) + step 7/7.5 (live levels, screen-demo
 * broadcast, mic broadcast) of the Tauri migration (vocalis_roadmap.md,
 * section 8). A real teacher session (`useLiveClassroom`) starts on
 * mount — its per-student mic levels are genuine, reported over the network
 * exactly the way the egui console's own grid gets them
 * (`student::audio::run_level_telemetry` → `ClientToServer::AudioLevel` →
 * `Student::last_level`), not simulated. As long as the step-3 login screens
 * stay mocked, nothing ever *dials into* this real session on its own,
 * though — so `useMockClassroom`'s simulation is still what's shown until at
 * least one real student actually connects (see the step-7 report for how
 * that was tested: a second local process really connecting and reporting
 * real mic levels). "Показать классу" and "Говорить с классом" broadcast
 * for real to whoever's really connected at the time — same caveat. Lock
 * buttons are the one remaining local-only UI state — nothing sends
 * `LockScreen`/`SetMicLocked` over the network yet. */
export function TeacherClassGrid({ className, onEnd }: Props) {
  const mock = useMockClassroom(12);
  const live = useLiveClassroom(className);
  const usingLiveData = live.realStudents.length > 0;
  const baseStudents = usingLiveData ? live.realStudents : mock.students;

  // Step 7 part B: own-screen self-preview, same real capture/codec/JPEG
  // loop as the student console's demo view (`useScreenDemo`'s doc comment
  // explains what "real" means here). Not the same thing as actually
  // broadcasting a demo to the class — that's a separate, unbuilt network
  // path — this just lets the teacher see what their own capture pipeline
  // is producing before/while relying on it.
  const [previewOpen, setPreviewOpen] = useState(false);
  const preview = useScreenDemo(previewOpen);

  // Step 7 part B's real class-wide broadcast (`teacher_session.rs`'s
  // `start_own_screen_demo`/`stop_own_screen_demo`, already covered by the
  // real two-process E2E test) — separate from `preview` above, which only
  // ever shows the teacher their own capture locally. `broadcastingRef`
  // mirrors `broadcasting` so the unmount-only cleanup below reads the
  // latest value without needing to run (and needlessly re-call `stop`) on
  // every toggle, the way a `[broadcasting]`-dependent effect would.
  const [broadcasting, setBroadcasting] = useState(false);
  const [broadcastError, setBroadcastError] = useState<string | undefined>();
  const broadcastingRef = useRef(false);
  useEffect(() => {
    broadcastingRef.current = broadcasting;
  }, [broadcasting]);
  useEffect(() => {
    return () => {
      if (broadcastingRef.current) stopOwnScreenDemo().catch(() => {});
    };
  }, []);

  async function toggleBroadcast() {
    if (broadcasting) {
      await stopOwnScreenDemo().catch(() => {});
      setBroadcasting(false);
      return;
    }
    try {
      await startOwnScreenDemo();
      setBroadcasting(true);
      setBroadcastError(undefined);
    } catch (err) {
      setBroadcastError(String(err));
    }
  }

  // Step 7.5: teacher's mic broadcast to the whole class
  // (`teacher_session.rs`'s `start_mic_broadcast`/`stop_mic_broadcast`,
  // reusing `teacher::mic::run_mic_broadcast` unchanged) — same
  // start/stop/error/unmount-cleanup shape as `broadcasting` above.
  const [micBroadcasting, setMicBroadcasting] = useState(false);
  const [micBroadcastError, setMicBroadcastError] = useState<string | undefined>();
  const micBroadcastingRef = useRef(false);
  useEffect(() => {
    micBroadcastingRef.current = micBroadcasting;
  }, [micBroadcasting]);
  useEffect(() => {
    return () => {
      if (micBroadcastingRef.current) stopMicBroadcast().catch(() => {});
    };
  }, []);

  async function toggleMicBroadcast() {
    if (micBroadcasting) {
      await stopMicBroadcast().catch(() => {});
      setMicBroadcasting(false);
      return;
    }
    try {
      await startMicBroadcast();
      setMicBroadcasting(true);
      setMicBroadcastError(undefined);
    } catch (err) {
      setMicBroadcastError(String(err));
    }
  }

  const [selectedId, setSelectedId] = useState<number | null>(null);
  // Lock buttons are local-only UI state regardless of data source — nothing
  // sends `LockScreen`/`SetMicLocked` over the network yet, so overlaying
  // them here (rather than threading through `useMockClassroom`, which real
  // students don't come from) works for both at once.
  const [locks, setLocks] = useState<Map<number, { screenLocked: boolean; micLocked: boolean }>>(new Map());
  const students = baseStudents.map((s) => ({ ...s, ...locks.get(s.id) }));

  function toggleScreenLock(id: number) {
    setLocks((prev) => {
      const next = new Map(prev);
      const cur = next.get(id) ?? { screenLocked: false, micLocked: false };
      next.set(id, { ...cur, screenLocked: !cur.screenLocked });
      return next;
    });
  }
  function toggleMicLock(id: number) {
    setLocks((prev) => {
      const next = new Map(prev);
      const cur = next.get(id) ?? { screenLocked: false, micLocked: false };
      next.set(id, { ...cur, micLocked: !cur.micLocked });
      return next;
    });
  }

  return (
    // `h-full w-full` (not `h-screen w-screen`) and `absolute` (not `fixed`)
    // below — step 5 nests this screen inside the new sidebar shell
    // (screens/TeacherConsole.tsx) instead of rendering it alone at the
    // viewport root, so it needs to fill its parent's box, not the whole
    // window. No other change: same design, same behavior, still full-screen
    // in the one case (this tab, sidebar visible) that exists today.
    <div className="relative flex h-full w-full flex-col overflow-hidden bg-[var(--color-app)] p-6">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_50%_0%,rgba(167,139,250,0.08),transparent_55%)]" />

      <motion.header
        initial={{ opacity: 0, y: -12 }}
        animate={{ opacity: 1, y: 0 }}
        className="relative z-10 mb-6 flex flex-wrap items-center justify-between gap-4"
      >
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">{className}</h1>
          <p className="text-sm text-[var(--color-text-muted)]">
            {usingLiveData ? (
              <span className="text-emerald-400">● живая сессия — {students.length} подключено</span>
            ) : (
              <>
                {students.filter((s) => s.presence !== "empty").length} / {students.length} мест занято (демо-режим)
              </>
            )}
            {live.pin && <span className="ml-3 font-mono tracking-wider text-[var(--color-text-muted)]">PIN: {live.pin}</span>}
          </p>
          {live.error && <p className="text-xs text-rose-400">Реальная сессия недоступна: {live.error}</p>}
          {broadcastError && <p className="text-xs text-rose-400">Не удалось начать показ: {broadcastError}</p>}
          {micBroadcastError && <p className="text-xs text-rose-400">Не удалось включить микрофон: {micBroadcastError}</p>}
        </div>

        <LessonTimer />

        <Button variant="secondary" onClick={() => setPreviewOpen((v) => !v)}>
          {previewOpen ? "Скрыть превью экрана" : "🖥 Превью своего экрана"}
        </Button>
        <Button variant="secondary" onClick={toggleBroadcast}>
          {broadcasting ? "⏹ Остановить показ" : "📡 Показать классу"}
        </Button>
        <Button variant="secondary" onClick={toggleMicBroadcast}>
          {micBroadcasting ? "🔇 Выключить микрофон" : "🎙 Говорить с классом"}
        </Button>
        <Button variant="secondary" onClick={onEnd}>
          Завершить урок
        </Button>
      </motion.header>

      <AnimatePresence>
        {previewOpen && (
          <motion.div
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 12 }}
            className="absolute bottom-4 left-4 z-30 w-64 overflow-hidden rounded-xl border border-[var(--color-border-subtle)] bg-black shadow-2xl"
          >
            <div className="relative flex aspect-video items-center justify-center">
              {preview.frame ? (
                <img src={preview.frame.dataUrl} alt="Ваш экран" className="h-full w-full object-contain" />
              ) : (
                <span className="text-xs text-white/50">{preview.error ? `Ошибка: ${preview.error}` : "Подключение…"}</span>
              )}
              <span className="absolute left-2 top-2 rounded bg-black/60 px-1.5 py-0.5 text-[10px] font-medium text-violet-300">
                Ваш экран (превью)
              </span>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <motion.div
        className="relative z-10 grid flex-1 auto-rows-min grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4 overflow-y-auto pb-4"
        initial="hidden"
        animate="visible"
        variants={{ visible: { transition: { staggerChildren: 0.04 } } }}
      >
        {students.map((s) => (
          <StudentCard
            key={s.id}
            student={s}
            selected={selectedId === s.id}
            onSelect={() => setSelectedId((cur) => (cur === s.id ? null : s.id))}
            onToggleScreenLock={() => toggleScreenLock(s.id)}
            onToggleMicLock={() => toggleMicLock(s.id)}
          />
        ))}
      </motion.div>
    </div>
  );
}
