import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { open } from "@tauri-apps/plugin-dialog";
import { Cast, LogOut, Mic, MicOff, Monitor, MonitorOff, Music, Play, Square, Users } from "lucide-react";
import { readSelectedMicDevice } from "../lib/micDevice";
import { StudentCard } from "../components/StudentCard";
import { LessonTimer } from "../components/LessonTimer";
import { Button } from "../components/ui/Button";
import { EmptyState } from "../components/ui/EmptyState";
import { WaitingForStudents } from "../components/WaitingForStudents";
import { PinChip } from "../components/PinDisplay";
import type { MockStudent } from "../lib/mockClassroom";
import type { LiveClassroom, LiveStudent } from "../lib/useLiveClassroom";
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
  /** The real teacher session, owned by `TeacherConsole` (so it survives tab switches). */
  live: LiveClassroom;
  onEnd: () => void;
}

/** Header action buttons: compact so all six fit one row. The three that open a panel keep a constant
 * label and show "pressed" instead, so the row never changes width as panels open and close. */
const HEADER_BTN = "px-3.5 py-2.5 text-sm ";
const PRESSED = "!border-violet-400/50 !bg-violet-400/15 !text-accent-text";

/** Seats always shown, taken or not. More than this many connected students just extend the grid. */
const SEAT_COUNT = 12;

function emptySeat(seat: number): MockStudent {
  // Negative ids can't collide with the live students' sequential positive ones.
  return { id: -seat, seat, name: "", presence: "empty", level: 0, screenLocked: false, micLocked: false };
}

/** Step 4 (grid/timer/lock UI) + step 7/7.5 (live levels, screen-demo
 * broadcast, mic broadcast, listen-in, private intercom, groups/pairs, audio
 * materials library) of the Tauri migration (vocalis_roadmap.md, section 8).
 * Shows only what is real: the students actually connected to the teacher
 * session (`live`, from `useLiveClassroom` — per-student mic levels are
 * genuine, reported over the network the way the egui console's own grid gets
 * them), padded with "Свободно" seats up to `SEAT_COUNT`. With nobody
 * connected it shows the lesson PIN and a "waiting for students" banner
 * instead of any simulated class. "Показать классу" and "Говорить с
 * классом" broadcast for real to whoever's connected at the time. Lock
 * buttons are the one remaining local-only UI state — nothing sends
 * `LockScreen`/`SetMicLocked` over the network yet. */
export function TeacherClassGrid({ className, live, onEnd }: Props) {
  const usingLiveData = live.realStudents.length > 0;
  // Real students first, renumbered 1..n so seat labels stay contiguous; then empty seats up to SEAT_COUNT.
  const baseStudents: (LiveStudent | MockStudent)[] = [
    ...live.realStudents.map((s, i) => ({ ...s, seat: i + 1 })),
    ...Array.from({ length: Math.max(0, SEAT_COUNT - live.realStudents.length) }, (_, i) =>
      emptySeat(live.realStudents.length + i + 1),
    ),
  ];

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
      await startMicBroadcast(readSelectedMicDevice());
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
      await startIntercom(realId, readSelectedMicDevice());
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

  // Gated on the session existing (`live.pin`), not on students being connected: the library is the
  // teacher's own, and is there — and uploadable — before anyone joins.
  const sessionReady = live.pin !== null;
  useEffect(() => {
    if (!materialsPanelOpen || materialsLoaded || !sessionReady) return;
    listMaterials()
      .then((list) => {
        setMaterialsList(list);
        setMaterialsLoaded(true);
      })
      .catch((err) => setMaterialsError(String(err)));
  }, [materialsPanelOpen, materialsLoaded, sessionReady]);

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
  // Lock buttons are local-only UI state — nothing
  // sends `LockScreen`/`SetMicLocked` over the network yet, so overlaying
  // them here works for real students and empty seats alike.
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
          <p className="mt-0.5 flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-[var(--color-text-muted)]">
            {usingLiveData ? (
              <span className="flex items-center gap-2 text-ok-text"><span className="h-2 w-2 rounded-full bg-current" />{live.realStudents.length} подключено</span>
            ) : (
              <span className="flex items-center gap-2"><span className="h-2 w-2 rounded-full border border-current" />Никто не подключён</span>
            )}
            {/* While nobody's connected the waiting banner below already shows the PIN, large. */}
            {live.pin && usingLiveData && <PinChip pin={live.pin} />}
          </p>
          {live.error && <p className="text-xs text-danger-text">Реальная сессия недоступна: {live.error}</p>}
          {broadcastError && <p className="text-xs text-danger-text">Не удалось начать показ: {broadcastError}</p>}
          {micBroadcastError && <p className="text-xs text-danger-text">Не удалось включить микрофон: {micBroadcastError}</p>}
          {listenError && <p className="text-xs text-danger-text">Не удалось начать прослушку: {listenError}</p>}
          {intercomError && <p className="text-xs text-danger-text">Не удалось начать интерком: {intercomError}</p>}
          {groupError && <p className="text-xs text-danger-text">Ошибка группировки: {groupError}</p>}
          {materialsError && <p className="text-xs text-danger-text">Ошибка материалов: {materialsError}</p>}
        </div>

        <LessonTimer />

        <div className="flex flex-wrap items-center gap-2">
          <Button variant="secondary" aria-pressed={previewOpen} className={HEADER_BTN + (previewOpen ? PRESSED : "")} onClick={() => setPreviewOpen((v) => !v)}>
            {previewOpen ? <MonitorOff size={16} /> : <Monitor size={16} />}
            Превью экрана
          </Button>
          <Button variant="secondary" className={HEADER_BTN} onClick={toggleBroadcast}>
            {broadcasting ? <Square size={16} /> : <Cast size={16} />}
            {broadcasting ? "Остановить показ" : "Показать классу"}
          </Button>
          <Button variant="secondary" className={HEADER_BTN} onClick={toggleMicBroadcast}>
            {micBroadcasting ? <MicOff size={16} /> : <Mic size={16} />}
            {micBroadcasting ? "Выключить микрофон" : "Говорить с классом"}
          </Button>
          <Button variant="secondary" aria-pressed={groupPanelOpen} className={HEADER_BTN + (groupPanelOpen ? PRESSED : "")} onClick={() => setGroupPanelOpen((v) => !v)}>
            <Users size={16} />
            Группы
          </Button>
          <Button variant="secondary" aria-pressed={materialsPanelOpen} className={HEADER_BTN + (materialsPanelOpen ? PRESSED : "")} onClick={() => setMaterialsPanelOpen((v) => !v)}>
            <Music size={16} />
            Материалы
          </Button>
          <Button variant="secondary" className={HEADER_BTN} onClick={onEnd}>
            <LogOut size={16} />
            Завершить урок
          </Button>
        </div>
      </motion.header>

      {/* Everything under the header lives in its own positioned box, so the floating panels below anchor to
          the grid area — not to the whole screen, where they used to cover the header's own buttons. */}
      <div className="relative flex min-h-0 flex-1 flex-col">
        <AnimatePresence>
          {previewOpen && (
            <motion.div
              initial={{ opacity: 0, y: 12 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 12 }}
              className="absolute bottom-2 left-2 z-30 w-64 overflow-hidden rounded-xl border border-[var(--color-border-subtle)] bg-black shadow-2xl"
            >
              <div className="relative flex aspect-video items-center justify-center">
                {preview.frame ? (
                  <img src={preview.frame.dataUrl} alt="Ваш экран" className="h-full w-full object-contain" />
                ) : (
                  <span className="text-xs text-white/50">{preview.error ? `Ошибка: ${preview.error}` : "Подключение…"}</span>
                )}
                <span className="absolute left-2 top-2 rounded bg-black/60 px-1.5 py-0.5 text-[10px] font-medium text-accent-text">
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
              className="absolute bottom-2 right-2 z-30 w-72 rounded-xl border border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] p-4 shadow-2xl"
            >
              <div className="mb-3 text-xs font-medium text-[var(--color-text-muted)]">Пары и группы</div>
              {live.realStudents.length === 0 ? (
                <EmptyState compact icon={<Users />} title="Нет подключённых учеников" hint="Группы можно собрать, когда в классе будет хотя бы двое." />
              ) : (
                <div className="flex max-h-64 flex-col gap-2 overflow-y-auto">
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
                          className="shrink-0 rounded-md bg-overlay px-2 py-1 text-xs text-[var(--color-text-muted)] hover:bg-overlay-hover"
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
              className="absolute right-2 top-2 z-30 w-80 rounded-xl border border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] p-4 shadow-2xl"
            >
              <div className="mb-3 flex items-center justify-between">
                <span className="text-xs font-medium text-[var(--color-text-muted)]">Библиотека аудио</span>
                {materialsLoaded && materialsList.length > 0 && (
                  <button
                    type="button"
                    onClick={handleUploadMaterial}
                    className="rounded-md bg-overlay px-2 py-1 text-xs text-[var(--color-text-muted)] hover:bg-overlay-hover"
                  >
                    + Добавить файл
                  </button>
                )}
              </div>

              {playingTitle && (
                <div className="mb-3 flex items-center justify-between gap-2 rounded-lg bg-violet-400/10 px-3 py-2 text-sm text-accent-text">
                  <span className="flex min-w-0 items-center gap-2"><Play size={14} className="shrink-0" /><span className="truncate">{playingTitle}</span></span>
                  <button type="button" onClick={handleStopPlayback} className="shrink-0 hover:text-accent-text">
                    <span className="flex items-center gap-1.5"><Square size={12} /> Стоп</span>
                  </button>
                </div>
              )}

              {!materialsLoaded ? (
                <p className="py-4 text-center text-sm text-[var(--color-text-muted)]">
                  {materialsError ? "Не удалось загрузить библиотеку." : live.error ? "Сессия недоступна." : sessionReady ? "Загрузка…" : "Запускаю сессию…"}
                </p>
              ) : materialsList.length === 0 ? (
                <EmptyState
                  compact
                  icon={<Music />}
                  title="Библиотека пуста"
                  hint="Добавьте mp3 или wav — потом его можно проиграть всему классу или выбранным ученикам."
                  action={
                    <Button variant="secondary" className="px-3 py-1.5 text-sm" onClick={handleUploadMaterial}>
                      + Добавить файл
                    </Button>
                  }
                />
              ) : (
                <div className="flex max-h-64 flex-col gap-2 overflow-y-auto">
                  {materialsList.map((m) => (
                    <div key={m.id} className="flex items-center justify-between gap-2 text-sm">
                      <span className="min-w-0 flex-1 truncate">{m.title}</span>
                      <button
                        type="button"
                        onClick={() => handlePlayMaterial(m.id, [])}
                        className="inline-flex shrink-0 items-center gap-1 rounded-md bg-overlay px-2 py-1 text-xs text-[var(--color-text-muted)] hover:bg-overlay-hover"
                        title="Проиграть всем подключённым"
                      >
                        <Play size={12} /> Всем
                      </button>
                      <button
                        type="button"
                        onClick={() => handlePlayMaterial(m.id, Array.from(groupSelection))}
                        disabled={groupSelection.size === 0}
                        className="inline-flex shrink-0 items-center gap-1 rounded-md bg-overlay px-2 py-1 text-xs text-[var(--color-text-muted)] hover:bg-overlay-hover disabled:cursor-not-allowed disabled:opacity-40"
                        title="Проиграть выбранным в панели «Группы»"
                      >
                        <Play size={12} /> Выбранным
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

        <AnimatePresence initial={false}>
          {!usingLiveData && <WaitingForStudents key="waiting" pin={live.pin} error={live.error} />}
        </AnimatePresence>

        <motion.div
          className="relative z-10 grid flex-1 auto-rows-min grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4 overflow-y-auto pb-4"
          initial="hidden"
          animate="visible"
          variants={{ visible: { transition: { staggerChildren: 0.04 } } }}
        >
          {students.map((s) => {
            // Only real students carry a `realId` (see `LiveStudent`) — empty
            // seats don't, so `listening`/`onToggleListen` stay `undefined` and
            // `StudentCard` simply doesn't render the button for them.
            const realId = "realId" in s ? s.realId : undefined;
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
    </div>
  );
}
