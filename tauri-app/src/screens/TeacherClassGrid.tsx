import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { open } from "@tauri-apps/plugin-dialog";
import { StudentCard } from "../components/StudentCard";
import { LessonTimer } from "../components/LessonTimer";
import { Button } from "../components/ui/Button";
import { useMockClassroom } from "../lib/mockClassroom";
import { useLiveClassroom, type LiveStudent } from "../lib/useLiveClassroom";
import { useScreenDemo } from "../lib/useScreenDemo";
import {
  startOwnScreenDemo,
  stopOwnScreenDemo,
  startMicBroadcast,
  stopMicBroadcast,
  startListen,
  stopListen,
  startIntercom,
  stopIntercom,
  createGroup,
  leaveGroup,
  listMaterials,
  uploadMaterial,
  playMaterial,
  stopPlayback,
  type MaterialDto,
} from "../lib/commands";

interface Props {
  className: string;
  onEnd: () => void;
}

/** Step 4 (grid/timer/lock UI) + step 7/7.5 (live levels, screen-demo
 * broadcast, mic broadcast, listen-in, private intercom, groups/pairs, audio
 * materials library) of the Tauri migration (vocalis_roadmap.md, section 8).
 * A real teacher
 * session (`useLiveClassroom`) starts on mount — its per-student mic levels
 * are genuine, reported over the network exactly the way the egui console's
 * own grid gets them
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
      // Materials playback and the live mic broadcast share MIC_PORT — the
      // backend already stopped any playback to start this, so keep the
      // materials panel's "⏹ Остановить" state honest about it.
      setPlayingTitle(null);
    } catch (err) {
      setMicBroadcastError(String(err));
    }
  }

  // Step 7.5: real-time listen-in on one selected student — `listeningId`
  // holds the real UUID (`LiveStudent.realId`), not the synthetic numeric
  // `MockStudent.id` `selectedId` below uses, since `start_listen` needs the
  // real one. Switching to a different student's "🎧 Слушать" is a single
  // `startListen` call — `SharedState::start_listening` already tells
  // whoever was previously listened to (if different) to stop, so no
  // explicit `stopListen` is needed first.
  const [listeningId, setListeningId] = useState<string | null>(null);
  const [listenError, setListenError] = useState<string | undefined>();
  const listeningIdRef = useRef<string | null>(null);
  useEffect(() => {
    listeningIdRef.current = listeningId;
  }, [listeningId]);
  useEffect(() => {
    return () => {
      if (listeningIdRef.current) stopListen().catch(() => {});
    };
  }, []);

  async function toggleListen(realId: string) {
    if (listeningId === realId) {
      await stopListen().catch(() => {});
      setListeningId(null);
      return;
    }
    try {
      await startListen(realId);
      setListeningId(realId);
      setListenError(undefined);
    } catch (err) {
      setListenError(String(err));
    }
  }

  // Step 7.5: private two-way intercom — same real-UUID shape as
  // `listeningId` above. `start_intercom` also calls `SharedState::
  // start_listening` internally (so the teacher hears the student back over
  // the same mechanism plain listen-in uses), so this keeps `listeningId` in
  // sync too — otherwise the "🎧 Слушать" button on this card would show as
  // inactive while the backend is, in fact, listening.
  const [intercomId, setIntercomId] = useState<string | null>(null);
  const [intercomError, setIntercomError] = useState<string | undefined>();
  const intercomIdRef = useRef<string | null>(null);
  useEffect(() => {
    intercomIdRef.current = intercomId;
  }, [intercomId]);
  useEffect(() => {
    return () => {
      if (intercomIdRef.current) stopIntercom().catch(() => {});
    };
  }, []);

  async function toggleIntercom(realId: string) {
    if (intercomId === realId) {
      await stopIntercom().catch(() => {});
      setIntercomId(null);
      return;
    }
    try {
      await startIntercom(realId);
      setIntercomId(realId);
      setListeningId(realId);
      setIntercomError(undefined);
    } catch (err) {
      setIntercomError(String(err));
    }
  }

  // Step 7.5: groups/pairs — a minimal list-with-checkboxes UI (drag-and-drop
  // wasn't required), calling `SharedState::create_group`/`leave_group`
  // unchanged. Only meaningful for real students (`live.realStudents`), so
  // the panel lists those directly rather than the merged mock/live
  // `students` below. `groupSelection` holds real UUIDs, cleared after a
  // successful `createGroup` call.
  const [groupPanelOpen, setGroupPanelOpen] = useState(false);
  const [groupSelection, setGroupSelection] = useState<Set<string>>(new Set());
  const [groupError, setGroupError] = useState<string | undefined>();

  function toggleGroupSelection(realId: string) {
    setGroupSelection((prev) => {
      const next = new Set(prev);
      if (next.has(realId)) next.delete(realId);
      else next.add(realId);
      return next;
    });
  }

  async function handleCreateGroup() {
    if (groupSelection.size < 2) return;
    try {
      await createGroup(Array.from(groupSelection));
      setGroupSelection(new Set());
      setGroupError(undefined);
    } catch (err) {
      setGroupError(String(err));
    }
  }

  async function handleLeaveGroup(realId: string) {
    await leaveGroup(realId).catch((err) => setGroupError(String(err)));
  }

  // Step 7.5: audio materials library — reuses `groupSelection` above as
  // "выбранные" for "проиграть выбранным" (both are "pick some real
  // students" operations, so one shared checkbox list avoids duplicating
  // the same UI for two features). Loads the real library lazily, once,
  // the first time this panel is opened against a real session.
  const [materialsPanelOpen, setMaterialsPanelOpen] = useState(false);
  const [materialsLoaded, setMaterialsLoaded] = useState(false);
  const [materialsList, setMaterialsList] = useState<MaterialDto[]>([]);
  const [materialsError, setMaterialsError] = useState<string | undefined>();
  const [playingTitle, setPlayingTitle] = useState<string | null>(null);
  const playingRef = useRef(false);
  useEffect(() => {
    playingRef.current = playingTitle !== null;
  }, [playingTitle]);
  useEffect(() => {
    return () => {
      if (playingRef.current) stopPlayback().catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!materialsPanelOpen || materialsLoaded || !usingLiveData) return;
    listMaterials()
      .then((list) => {
        setMaterialsList(list);
        setMaterialsLoaded(true);
      })
      .catch((err) => setMaterialsError(String(err)));
  }, [materialsPanelOpen, materialsLoaded, usingLiveData]);

  async function handleUploadMaterial() {
    const path = await open({ multiple: false, filters: [{ name: "Аудио", extensions: ["mp3", "wav"] }] });
    if (!path) return;
    const fileName = path.split(/[\\/]/).pop() ?? "Материал";
    const title = fileName.replace(/\.[^./]+$/, "");
    try {
      const material = await uploadMaterial(path, title);
      setMaterialsList((prev) => [material, ...prev]);
      setMaterialsError(undefined);
    } catch (err) {
      setMaterialsError(String(err));
    }
  }

  async function handlePlayMaterial(materialId: number, targetIds: string[]) {
    try {
      const info = await playMaterial(materialId, targetIds);
      setPlayingTitle(info.title);
      // Materials playback and the live mic broadcast share MIC_PORT — the
      // backend already stops one when the other starts, this just keeps
      // the "🎙 Говорить с классом" button's label honest about it.
      setMicBroadcasting(false);
      setMaterialsError(undefined);
    } catch (err) {
      setMaterialsError(String(err));
    }
  }

  async function handleStopPlayback() {
    await stopPlayback().catch(() => {});
    setPlayingTitle(null);
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
          {listenError && <p className="text-xs text-rose-400">Не удалось начать прослушку: {listenError}</p>}
          {intercomError && <p className="text-xs text-rose-400">Не удалось начать интерком: {intercomError}</p>}
          {groupError && <p className="text-xs text-rose-400">Ошибка группировки: {groupError}</p>}
          {materialsError && <p className="text-xs text-rose-400">Ошибка материалов: {materialsError}</p>}
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
        <Button variant="secondary" onClick={() => setGroupPanelOpen((v) => !v)}>
          {groupPanelOpen ? "Скрыть группы" : "👥 Группы"}
        </Button>
        <Button variant="secondary" onClick={() => setMaterialsPanelOpen((v) => !v)}>
          {materialsPanelOpen ? "Скрыть материалы" : "🎵 Материалы"}
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

      <AnimatePresence>
        {groupPanelOpen && (
          <motion.div
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 12 }}
            className="absolute bottom-4 right-4 z-30 w-72 rounded-xl border border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] p-4 shadow-2xl"
          >
            <div className="mb-3 text-xs font-medium text-[var(--color-text-muted)]">Пары и группы</div>
            {live.realStudents.length === 0 ? (
              <p className="text-sm text-[var(--color-text-muted)]">Нет подключённых учеников.</p>
            ) : (
              <div className="flex flex-col gap-2">
                {live.realStudents.map((s) => (
                  <div key={s.realId} className="flex items-center justify-between gap-2 text-sm">
                    <label className="flex min-w-0 flex-1 cursor-pointer items-center gap-2">
                      <input
                        type="checkbox"
                        checked={groupSelection.has(s.realId)}
                        onChange={() => toggleGroupSelection(s.realId)}
                        className="accent-violet-400"
                      />
                      <span className="truncate">{s.name}</span>
                    </label>
                    {s.group !== null && (
                      <button
                        type="button"
                        onClick={() => handleLeaveGroup(s.realId)}
                        className="shrink-0 rounded-md bg-white/5 px-2 py-1 text-xs text-[var(--color-text-muted)] hover:bg-white/10"
                      >
                        Группа {s.group} · выйти
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}
            <Button variant="secondary" className="mt-3 w-full" disabled={groupSelection.size < 2} onClick={handleCreateGroup}>
              Создать пару/группу
            </Button>
          </motion.div>
        )}
      </AnimatePresence>

      <AnimatePresence>
        {materialsPanelOpen && (
          <motion.div
            initial={{ opacity: 0, y: -12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -12 }}
            className="absolute right-4 top-24 z-30 w-80 rounded-xl border border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] p-4 shadow-2xl"
          >
            <div className="mb-3 flex items-center justify-between">
              <span className="text-xs font-medium text-[var(--color-text-muted)]">Библиотека аудио</span>
              <button
                type="button"
                onClick={handleUploadMaterial}
                className="rounded-md bg-white/5 px-2 py-1 text-xs text-[var(--color-text-muted)] hover:bg-white/10"
              >
                + Добавить файл
              </button>
            </div>

            {playingTitle && (
              <div className="mb-3 flex items-center justify-between gap-2 rounded-lg bg-violet-400/10 px-3 py-2 text-sm text-violet-300">
                <span className="truncate">▶ {playingTitle}</span>
                <button type="button" onClick={handleStopPlayback} className="shrink-0 hover:text-violet-100">
                  ⏹ Стоп
                </button>
              </div>
            )}

            {materialsList.length === 0 ? (
              <p className="text-sm text-[var(--color-text-muted)]">Библиотека пуста — добавьте mp3/wav файл.</p>
            ) : (
              <div className="flex max-h-64 flex-col gap-2 overflow-y-auto">
                {materialsList.map((m) => (
                  <div key={m.id} className="flex items-center justify-between gap-2 text-sm">
                    <span className="min-w-0 flex-1 truncate">{m.title}</span>
                    <button
                      type="button"
                      onClick={() => handlePlayMaterial(m.id, [])}
                      className="shrink-0 rounded-md bg-white/5 px-2 py-1 text-xs text-[var(--color-text-muted)] hover:bg-white/10"
                      title="Проиграть всем подключённым"
                    >
                      ▶ Всем
                    </button>
                    <button
                      type="button"
                      onClick={() => handlePlayMaterial(m.id, Array.from(groupSelection))}
                      disabled={groupSelection.size === 0}
                      className="shrink-0 rounded-md bg-white/5 px-2 py-1 text-xs text-[var(--color-text-muted)] hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-40"
                      title="Проиграть выбранным в панели «Группы»"
                    >
                      ▶ Выбранным
                    </button>
                  </div>
                ))}
              </div>
            )}
            <p className="mt-3 text-[11px] text-[var(--color-text-muted)]">
              «Выбранным» использует отметки из панели «Группы» ({groupSelection.size} выбрано).
            </p>
          </motion.div>
        )}
      </AnimatePresence>

      <motion.div
        className="relative z-10 grid flex-1 auto-rows-min grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4 overflow-y-auto pb-4"
        initial="hidden"
        animate="visible"
        variants={{ visible: { transition: { staggerChildren: 0.04 } } }}
      >
        {students.map((s) => {
          // Only real students carry a `realId` (see `LiveStudent`) — mock
          // ones don't, so `listening`/`onToggleListen` stay `undefined` and
          // `StudentCard` simply doesn't render the button for them.
          const realId = usingLiveData ? (s as LiveStudent).realId : undefined;
          return (
            <StudentCard
              key={s.id}
              student={s}
              selected={selectedId === s.id}
              onSelect={() => setSelectedId((cur) => (cur === s.id ? null : s.id))}
              onToggleScreenLock={() => toggleScreenLock(s.id)}
              onToggleMicLock={() => toggleMicLock(s.id)}
              listening={realId ? listeningId === realId : undefined}
              onToggleListen={realId ? () => toggleListen(realId) : undefined}
              intercomActive={realId ? intercomId === realId : undefined}
              onToggleIntercom={realId ? () => toggleIntercom(realId) : undefined}
            />
          );
        })}
      </motion.div>
    </div>
  );
}
