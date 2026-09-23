import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { CircleHelp, FlaskConical, Hand, Link2, Lock, Mic, MicOff, Monitor, Play, Radio, Square, ThumbsUp, Trash2, X, type LucideIcon } from "lucide-react";
import { Button } from "../components/ui/Button";
import { Panel } from "../components/ui/Panel";
import { VuMeter } from "../components/VuMeter";
import { FlyingReactions, type FlyingReaction } from "../components/FlyingReactions";
import { ROSTER } from "../lib/mockClassroom";
import { KIND_META, tint, type AssignmentContent } from "../lib/assignments";
import { useMicMeter } from "../lib/useMicMeter";
import { useStudentSession } from "../lib/useStudentSession";
import { useRecordings } from "../lib/useRecordings";
import { setHandRaised as sendHandRaised, type AssignmentOfferDto } from "../lib/commands";
import { useAssignments } from "../lib/useAssignments";

interface Props {
  studentName: string;
  teacherIp: string;
  controlPort: number;
  pin: string;
  onDisconnect: () => void;
}

const partnerOptions = ROSTER.filter((_, i) => i % 2 === 1);

/** The "Демо-переключатели" panel is a debugging aid — most of the teacher events it stands in for
 * (listening / lock / group) aren't delivered to this screen yet. Assignments are the exception: those
 * *are* real (`useAssignments`) — its "Есть демо-задание" toggle just adds one synthetic extra on top, for
 * reviewing the card's look without a real teacher session. `import.meta.env.DEV` is true only under
 * `vite`/`tauri dev`: a production build (the installer) compiles the whole panel out, so a real student
 * never sees a synthetic assignment. */
const SHOW_DEMO_CONTROLS = import.meta.env.DEV;

/** The one synthetic assignment `showDevAssignment` can add — clearly marked so it's never mistaken for a
 * real offer, and given an id no real `Uuid::new_v4()` will ever collide with. */
const DEV_ASSIGNMENT: AssignmentOfferDto = {
  id: "dev-demo-assignment",
  title: "Времена группы Present (демо)",
  content: {
    kind: "test",
    questions: [{ text: "She ___ to school every day.", options: ["go", "goes", "going"], correctIndex: 1 }],
  },
};

/** Step 6 (screen) + step 7 parts A/B (live mic level, live screen-demo
 * video) of the Tauri migration (vocalis_roadmap.md, section 8) — takes cues
 * from `student::app` in the egui app (lock overlay, listening/group
 * banners, assignment card, raise-hand) but isn't a port: quick reactions
 * (👍/❓) are new, added directly on this stack per the roadmap's step-8
 * intro. Listening/grouped-with/lock/assignment are still local mock
 * state — there's no session delivering *those* yet — so the
 * "Демо-переключатели" panel at the bottom (dev builds only — see
 * `SHOW_DEMO_CONTROLS`) exists to make every such visual state reachable for review (its "Демонстрация экрана" toggle is now purely
 * a manual placeholder-preview trigger, since real demos start themselves —
 * see below). `useStudentSession` connects for real on mount
 * (`commands/student_session.rs`, the same Hello/Welcome handshake and
 * session key the egui student app uses) and its `frame` is the teacher's
 * *actual* screen, broadcast to the whole class over the real network — not
 * a local capture, unlike the self-preview `useScreenDemo` hook this used
 * before that path existed. The mic meter next to this student's own name is
 * likewise a real `cpal` capture (`useMicMeter`), not a mock, exactly like
 * the egui app's own top-bar VU meter. "Запись голоса" (`useRecordings`,
 * step 7.5 item 6) is real too: it records the same mic PCM the session
 * already streams, saves it as a WAV, and plays it back in-app — comparing a
 * recording to the teacher's reference is a separate, later step. */
export function StudentConsole({ studentName, teacherIp, controlPort, pin, onDisconnect }: Props) {
  const mic = useMicMeter();
  const session = useStudentSession({ studentName, teacherIp, controlPort, pin });
  const voice = useRecordings();
  const teacherLabel = session.teacherName ?? "подключение…";
  const [listening, setListening] = useState(false);
  const [partner, setPartner] = useState<string | null>(null);
  const [demoActive, setDemoActive] = useState(false);
  const showingDemo = demoActive || Boolean(session.frame);
  const [screenLocked, setScreenLocked] = useState(false);
  const [micLocked, setMicLocked] = useState(false);
  // Real assignments this session has actually received (`commands/student_session.rs`'s "assignments"
  // event, itself a real `ServerToClient::AssignmentOffer`) — `showDevAssignment` (dev builds only) appends
  // one synthetic entry on top, purely so the card is reviewable without a real teacher session running.
  const realAssignments = useAssignments();
  const [showDevAssignment, setShowDevAssignment] = useState(false);
  const assignments = showDevAssignment ? [...realAssignments, DEV_ASSIGNMENT] : realAssignments;

  // Real "поднять руку" (`commands/student_session.rs`'s `set_hand_raised` — a real
  // `ClientToServer::RequestHelp` over the network, seen by the teacher as `needsHelp` on this student's
  // card): optimistic — flips immediately, then reverts with an error if the send actually failed (no
  // session yet, or it dropped).
  const [handRaised, setHandRaisedLocal] = useState(false);
  const [handError, setHandError] = useState<string | undefined>();
  const [reactions, setReactions] = useState<FlyingReaction[]>([]);

  function toggleHand() {
    const next = !handRaised;
    setHandRaisedLocal(next);
    sendHandRaised(next)
      .then(() => setHandError(undefined))
      .catch((err) => {
        setHandRaisedLocal(!next);
        setHandError(String(err));
      });
  }

  function fireReaction(icon: LucideIcon, tone: string) {
    const id = Date.now() + Math.random();
    setReactions((prev) => [...prev, { id, icon, tone, offsetX: (Math.random() - 0.5) * 90 }]);
    setTimeout(() => setReactions((prev) => prev.filter((r) => r.id !== id)), 1300);
  }

  return (
    <div className="relative h-screen w-screen overflow-hidden bg-[var(--color-app)]">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_50%_0%,rgba(167,139,250,0.08),transparent_55%)]" />

      {!screenLocked && (
        <div className="relative z-10 flex h-full flex-col overflow-y-auto p-6">
          <motion.header
            initial={{ opacity: 0, y: -12 }}
            animate={{ opacity: 1, y: 0 }}
            className="mb-5 flex flex-wrap items-center justify-between gap-3"
          >
            <div>
              <h1 className="text-xl font-semibold tracking-tight text-accent">Vocalis — ученик</h1>
              <p className="text-sm text-[var(--color-text-muted)]">
                {studentName} · подключено к {teacherLabel}
              </p>
              {mic.error && <p className="text-xs text-danger-text">Микрофон недоступен: {mic.error}</p>}
              {session.error && <p className="text-xs text-danger-text">Не удалось подключиться: {session.error}</p>}
            </div>
            <div className="flex items-center gap-4">
              <div className="flex items-center gap-2" title="Ваш микрофон (реальный уровень)">
                <Mic size={18} className="text-[var(--color-text-muted)]" />
                {/* `active` always true here (unlike the teacher grid's per-student
                    meters, which gate on crossing SPEAKING_THRESHOLD) — this is
                    personal input monitoring, so it should move continuously with
                    the real level rather than snapping on only once "speaking". */}
                <VuMeter level={mic.level} active />
              </div>
              <Button variant="secondary" onClick={onDisconnect}>
                Отключиться
              </Button>
            </div>
          </motion.header>

          <div className="mx-auto flex w-full max-w-2xl flex-1 flex-col gap-4">
            <AnimatePresence>
              {micLocked && <StatusBanner key="mic" color="var(--color-status-danger)" icon={<MicOff size={16} />} text="Микрофон заблокирован преподавателем" />}
              {listening && <StatusBanner key="listen" color="var(--color-status-warn)" icon={<Radio size={16} />} text="Учитель слушает ваш микрофон в реальном времени" />}
              {partner && <StatusBanner key="partner" color="var(--color-status-accent)" icon={<Link2 size={16} />} text={`В группе с: ${partner}`} />}
            </AnimatePresence>

            <Panel className="relative flex aspect-video items-center justify-center overflow-hidden p-0">
              {showingDemo ? (
                <>
                  {session.frame ? (
                    <img
                      src={session.frame.dataUrl}
                      width={session.frame.width}
                      height={session.frame.height}
                      alt="Демонстрация экрана"
                      className="absolute inset-0 h-full w-full object-contain bg-black"
                    />
                  ) : (
                    <>
                      {/* Opaque backing layer: this placeholder is meant to always read as a dark video
                          screen, in both themes — same as the teacher-side preview thumbnail's plain
                          `bg-black` (`TeacherClassGrid.tsx`). Without it, `Panel`'s own theme-reactive
                          background could show through the gradient below (its `motion.div` sometimes
                          renders with a computed opacity under 1 during its keyframe animation), so the
                          "connecting" state would look lighter in light theme instead of staying dark. */}
                      <div className="absolute inset-0 bg-black" />
                      <motion.div
                        className="absolute inset-0"
                        style={{ background: "linear-gradient(120deg, #2a2140, #1a1a2e, #241a38)", backgroundSize: "200% 200%" }}
                        animate={{ backgroundPosition: ["0% 50%", "100% 50%", "0% 50%"] }}
                        transition={{ duration: 6, repeat: Infinity, ease: "linear" }}
                      />
                    </>
                  )}
                  <span className="absolute left-3 top-3 rounded-md bg-black/50 px-2 py-1 text-xs font-medium text-accent-text backdrop-blur">
                    <Monitor size={14} className="mr-1.5 inline-block align-[-2px]" />
                    Демонстрация экрана: {teacherLabel}
                  </span>
                  {!session.frame && <span className="relative text-sm text-white/60">Подключение…</span>}
                </>
              ) : (
                <motion.div
                  className="flex flex-col items-center gap-2 text-[var(--color-text-muted)]"
                  animate={{ opacity: [0.4, 0.8, 0.4] }}
                  transition={{ duration: 2.2, repeat: Infinity, ease: "easeInOut" }}
                >
                  <Monitor size={34} strokeWidth={1.5} />
                  <span className="text-sm">Нет активной трансляции</span>
                </motion.div>
              )}
            </Panel>

            {assignments.map((a) => (
              <AssignmentCard key={a.id} assignment={a} />
            ))}

            <Panel>
              <div className="flex items-center justify-between gap-3">
                <div>
                  <div className="flex items-center gap-2 font-medium">
                    <Mic size={16} />
                    Запись голоса
                  </div>
                  <div className="text-xs text-[var(--color-text-muted)]">
                    {voice.recording ? (
                      <span className="inline-flex items-center gap-1.5 text-danger-text">
                        <motion.span className="h-2 w-2 rounded-full bg-current" animate={{ opacity: [1, 0.3, 1] }} transition={{ repeat: Infinity, duration: 1.2 }} />
                        идёт запись — {formatDuration(voice.elapsedSecs)}
                      </span>
                    ) : (
                      "Запишите себя и прослушайте — запись остаётся на этом компьютере"
                    )}
                  </div>
                </div>
                <Button variant="secondary" className="px-3 py-1.5 text-sm" onClick={voice.toggleRecording}>
                  {voice.recording ? <Square size={15} /> : <span className="h-3 w-3 rounded-full bg-[var(--color-status-danger)]" />}
                  {voice.recording ? "Остановить" : "Записать"}
                </Button>
              </div>
              {voice.error && <p className="mt-3 rounded-lg bg-rose-400/10 px-3 py-2 text-sm text-danger-text">{voice.error}</p>}
              <AnimatePresence initial={false}>
                {voice.recordings.map((r) => (
                  <motion.div
                    key={r.name}
                    layout
                    initial={{ opacity: 0, height: 0 }}
                    animate={{ opacity: 1, height: "auto" }}
                    exit={{ opacity: 0, height: 0 }}
                    className="overflow-hidden"
                  >
                    <div className="mt-2 flex items-center gap-3 rounded-xl bg-overlay px-3 py-2">
                      <motion.button
                        type="button"
                        whileTap={{ scale: 0.9 }}
                        onClick={() => voice.togglePlay(r.name)}
                        className={
                          "flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-sm transition-colors " +
                          (voice.playing === r.name ? "bg-violet-400/30 text-accent-text" : "bg-overlay-hover text-[var(--color-text-muted)] hover:bg-overlay-strong")
                        }
                        title={voice.playing === r.name ? "Остановить" : "Прослушать"}
                      >
                        {voice.playing === r.name ? <Square size={13} /> : <Play size={13} />}
                      </motion.button>
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-sm">{recordingLabel(r.recordedAtEpoch, r.name)}</div>
                        <div className="text-xs text-[var(--color-text-muted)]">{formatDuration(Math.round(r.durationSecs))}</div>
                      </div>
                      <button
                        type="button"
                        onClick={() => voice.remove(r.name)}
                        className="shrink-0 rounded-md px-2 py-1 text-sm text-[var(--color-text-muted)] hover:bg-overlay-hover hover:text-danger-text"
                        title="Удалить запись"
                      >
                        <Trash2 size={16} />
                      </button>
                    </div>
                  </motion.div>
                ))}
              </AnimatePresence>
              {voice.recordings.length === 0 && !voice.recording && (
                <p className="mt-3 text-sm text-[var(--color-text-muted)]">Пока нет записей.</p>
              )}
            </Panel>

            <div className="mt-auto flex items-center justify-center gap-3 pt-4">
              <motion.div whileTap={{ scale: 0.95 }}>
                <button
                  type="button"
                  onClick={toggleHand}
                  className={
                    "inline-flex items-center gap-2 rounded-xl px-5 py-3 font-medium transition-colors " +
                    (handRaised ? "bg-amber-400/20 text-warn-text" : "bg-overlay text-[var(--color-text-muted)] hover:bg-overlay-hover")
                  }
                >
                  <motion.span
                    className="inline-block"
                    animate={handRaised ? { rotate: [0, -12, 12, -8, 0] } : { rotate: 0 }}
                    transition={{ duration: 0.5 }}
                  >
                    <Hand size={18} />
                  </motion.span>
                  {handRaised ? "Рука поднята" : "Поднять руку"}
                </button>
              </motion.div>
              {handError && <p className="self-center text-xs text-danger-text">{handError}</p>}

              <motion.button
                type="button"
                whileTap={{ scale: 0.85 }}
                onClick={() => fireReaction(ThumbsUp, "text-ok-text")}
                className="inline-flex items-center justify-center rounded-xl bg-overlay px-4 py-3 hover:bg-overlay-hover"
                title="Понял"
                aria-label="Понял"
              >
                <ThumbsUp size={20} />
              </motion.button>
              <motion.button
                type="button"
                whileTap={{ scale: 0.85 }}
                onClick={() => fireReaction(CircleHelp, "text-warn-text")}
                className="inline-flex items-center justify-center rounded-xl bg-overlay px-4 py-3 hover:bg-overlay-hover"
                title="Не понял"
                aria-label="Не понял"
              >
                <CircleHelp size={20} />
              </motion.button>
            </div>
          </div>

          <FlyingReactions reactions={reactions} />
        </div>
      )}

      <AnimatePresence>
        {screenLocked && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="absolute inset-0 z-40 flex items-center justify-center bg-rose-600 p-10 text-center"
          >
            <motion.p initial={{ scale: 0.9 }} animate={{ scale: 1 }} className="flex items-center justify-center gap-4 text-3xl font-semibold text-white">
              <Lock size={34} />
              Экран заблокирован преподавателем
            </motion.p>
          </motion.div>
        )}
      </AnimatePresence>

      {SHOW_DEMO_CONTROLS && (
        <DemoControls
          listening={listening}
          setListening={setListening}
          partner={partner}
          setPartner={setPartner}
          demoActive={demoActive}
          setDemoActive={setDemoActive}
          screenLocked={screenLocked}
          setScreenLocked={setScreenLocked}
          micLocked={micLocked}
          setMicLocked={setMicLocked}
          hasAssignment={showDevAssignment}
          setHasAssignment={setShowDevAssignment}
        />
      )}
    </div>
  );
}

function formatDuration(totalSecs: number): string {
  const m = Math.floor(totalSecs / 60);
  const sec = totalSecs % 60;
  return `${m}:${sec.toString().padStart(2, "0")}`;
}

/** "Запись 02:22" from the epoch in the file name, or the file name itself if it has none. */
function recordingLabel(epoch: number | null, fallback: string): string {
  if (epoch === null) return fallback;
  const d = new Date(epoch * 1000);
  return `Запись ${d.toLocaleDateString("ru-RU")} ${d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" })}`;
}

function StatusBanner({ color, icon, text }: { color: string; icon: React.ReactNode; text: string }) {
  return (
    <motion.div
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: "auto" }}
      exit={{ opacity: 0, height: 0 }}
      className="overflow-hidden"
    >
      <div className="flex items-center gap-2.5 rounded-xl px-4 py-2.5 text-sm font-medium" style={{ backgroundColor: tint(color, 12), color }}>
        {icon}
        {text}
      </div>
    </motion.div>
  );
}

interface DemoControlsProps {
  listening: boolean;
  setListening: (v: boolean) => void;
  partner: string | null;
  setPartner: (v: string | null) => void;
  demoActive: boolean;
  setDemoActive: (v: boolean) => void;
  screenLocked: boolean;
  setScreenLocked: (v: boolean) => void;
  micLocked: boolean;
  setMicLocked: (v: boolean) => void;
  hasAssignment: boolean;
  setHasAssignment: (v: boolean) => void;
}

/** Not a real feature — a stand-in for the teacher-driven events this screen
 * will eventually receive over the network (step 7+), so every state above
 * (lock overlay, banners, demo placeholder) can actually be seen and
 * reviewed today. Floats above everything, including the lock overlay, so
 * it's never possible to get stuck. */
function DemoControls(props: DemoControlsProps) {
  const [open, setOpen] = useState(false);
  return (
    <div className="absolute bottom-4 right-4 z-50">
      {open ? (
        <div className="w-72 rounded-2xl border border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] p-4 shadow-2xl">
          <div className="mb-3 flex items-center justify-between">
            <span className="text-xs font-medium text-[var(--color-text-muted)]">Демо-переключатели (без сети)</span>
            <button type="button" onClick={() => setOpen(false)} className="text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]">
              <X size={16} />
            </button>
          </div>
          <div className="flex flex-col gap-2 text-sm">
            <DemoToggle label="Учитель слушает" checked={props.listening} onChange={props.setListening} />
            <DemoToggle label="Экран заблокирован" checked={props.screenLocked} onChange={props.setScreenLocked} />
            <DemoToggle label="Микрофон заблокирован" checked={props.micLocked} onChange={props.setMicLocked} />
            <DemoToggle label="Демонстрация экрана" checked={props.demoActive} onChange={props.setDemoActive} />
            <DemoToggle label="Есть демо-задание" checked={props.hasAssignment} onChange={props.setHasAssignment} />
            <div>
              <label className="mb-1 block text-xs text-[var(--color-text-muted)]">В группе с</label>
              <select
                value={props.partner ?? ""}
                onChange={(e) => props.setPartner(e.target.value || null)}
                className="w-full rounded-lg border border-[var(--color-border-subtle)] bg-field px-2 py-1.5 text-sm outline-none focus:border-violet-400"
              >
                <option value="">Никого</option>
                {partnerOptions.map((name) => (
                  <option key={name}>{name}</option>
                ))}
              </select>
            </div>
          </div>
        </div>
      ) : (
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="inline-flex items-center gap-1.5 rounded-full border border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] px-4 py-2 text-xs font-medium text-[var(--color-text-muted)] shadow-lg hover:text-[var(--color-text-primary)]"
        >
          <FlaskConical size={14} />
          Демо-переключатели
        </button>
      )}
    </div>
  );
}

function DemoToggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex cursor-pointer items-center justify-between gap-2">
      <span className="text-[var(--color-text-muted)]">{label}</span>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} className="accent-violet-400" />
    </label>
  );
}

/** One real received assignment: kind badge, title, and an expand/collapse toggle to see it. Answering is
 * a later step (per-kind display only, for now — see this file's own module doc comment): reading the
 * question is real, submitting an answer isn't wired up yet. */
function AssignmentCard({ assignment }: { assignment: AssignmentOfferDto }) {
  const [open, setOpen] = useState(false);
  const meta = KIND_META[assignment.content.kind];
  return (
    <Panel>
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <span
            className="shrink-0 rounded-md px-2 py-0.5 text-xs font-medium"
            style={{ backgroundColor: tint(meta.color), color: meta.color }}
          >
            {meta.label}
          </span>
          <span className="min-w-0 truncate font-medium">{assignment.title}</span>
        </div>
        <Button variant="secondary" className="shrink-0 px-3 py-1.5 text-sm" onClick={() => setOpen((v) => !v)}>
          {open ? "Свернуть" : "Начать"}
        </Button>
      </div>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            className="overflow-hidden"
          >
            <div className="mt-4 rounded-lg bg-overlay px-3 py-3 text-sm">
              <AssignmentBody content={assignment.content} />
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </Panel>
  );
}

function AssignmentBody({ content }: { content: AssignmentContent }) {
  if (content.kind === "test") {
    return (
      <ol className="flex flex-col gap-3">
        {content.questions.map((q, qi) => (
          <li key={qi}>
            <div className="mb-1.5 font-medium">
              {qi + 1}. {q.text}
            </div>
            <ul className="flex flex-col gap-1 pl-4 text-[var(--color-text-muted)]">
              {q.options.map((o, oi) => (
                <li key={oi} className="flex items-center gap-2">
                  <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-current" />
                  {o}
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ol>
    );
  }
  if (content.kind === "listening") {
    return (
      <div>
        <div className="mb-2 font-medium">{content.materialTitle}</div>
        {content.questions.length > 0 && (
          <ol className="flex flex-col gap-1 pl-4 text-[var(--color-text-muted)]">
            {content.questions.map((q, qi) => (
              <li key={qi}>
                {qi + 1}. {q}
              </li>
            ))}
          </ol>
        )}
      </div>
    );
  }
  return <p className="whitespace-pre-wrap">{content.text}</p>;
}
