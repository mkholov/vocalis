import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Button } from "../components/ui/Button";
import { Panel } from "../components/ui/Panel";
import { VuMeter } from "../components/VuMeter";
import { FlyingReactions, type FlyingReaction } from "../components/FlyingReactions";
import { ROSTER } from "../lib/mockClassroom";
import { useMicMeter } from "../lib/useMicMeter";
import { useStudentSession } from "../lib/useStudentSession";

interface Props {
  studentName: string;
  teacherIp: string;
  controlPort: number;
  pin: string;
  onDisconnect: () => void;
}

type AssignmentKind = "test" | "listening" | "reading";

const KIND_META: Record<AssignmentKind, { label: string; color: string }> = {
  test: { label: "Тест", color: "#e6a84a" },
  listening: { label: "Аудирование", color: "#a78bfa" },
  reading: { label: "Чтение", color: "#5ec980" },
};

const partnerOptions = ROSTER.filter((_, i) => i % 2 === 1);

/** Step 6 (screen) + step 7 parts A/B (live mic level, live screen-demo
 * video) of the Tauri migration (vocalis_roadmap.md, section 8) — takes cues
 * from `student::app` in the egui app (lock overlay, listening/group
 * banners, assignment card, raise-hand) but isn't a port: quick reactions
 * (👍/❓) are new, added directly on this stack per the roadmap's step-8
 * intro. Listening/grouped-with/lock/assignment are still local mock
 * state — there's no session delivering *those* yet — so the
 * "Демо-переключатели" panel at the bottom exists to make every such visual
 * state reachable for review (its "Демонстрация экрана" toggle is now purely
 * a manual placeholder-preview trigger, since real demos start themselves —
 * see below). `useStudentSession` connects for real on mount
 * (`commands/student_session.rs`, the same Hello/Welcome handshake and
 * session key the egui student app uses) and its `frame` is the teacher's
 * *actual* screen, broadcast to the whole class over the real network — not
 * a local capture, unlike the self-preview `useScreenDemo` hook this used
 * before that path existed. The mic meter next to this student's own name is
 * likewise a real `cpal` capture (`useMicMeter`), not a mock, exactly like
 * the egui app's own top-bar VU meter. */
export function StudentConsole({ studentName, teacherIp, controlPort, pin, onDisconnect }: Props) {
  const mic = useMicMeter();
  const session = useStudentSession({ studentName, teacherIp, controlPort, pin });
  const teacherLabel = session.teacherName ?? "подключение…";
  const [listening, setListening] = useState(false);
  const [partner, setPartner] = useState<string | null>(null);
  const [demoActive, setDemoActive] = useState(false);
  const showingDemo = demoActive || Boolean(session.frame);
  const [screenLocked, setScreenLocked] = useState(false);
  const [micLocked, setMicLocked] = useState(false);
  const [hasAssignment, setHasAssignment] = useState(true);
  const [assignmentOpen, setAssignmentOpen] = useState(false);
  const assignment = hasAssignment ? { title: "Времена группы Present", kind: "test" as AssignmentKind } : null;

  const [handRaised, setHandRaised] = useState(false);
  const [reactions, setReactions] = useState<FlyingReaction[]>([]);

  function fireReaction(emoji: string) {
    const id = Date.now() + Math.random();
    setReactions((prev) => [...prev, { id, emoji, offsetX: (Math.random() - 0.5) * 90 }]);
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
              <h1 className="text-xl font-semibold tracking-tight text-violet-400">Vocalis — ученик</h1>
              <p className="text-sm text-[var(--color-text-muted)]">
                {studentName} · подключено к {teacherLabel}
              </p>
              {mic.error && <p className="text-xs text-rose-400">Микрофон недоступен: {mic.error}</p>}
              {session.error && <p className="text-xs text-rose-400">Не удалось подключиться: {session.error}</p>}
            </div>
            <div className="flex items-center gap-4">
              <div className="flex items-center gap-2" title="Ваш микрофон (реальный уровень)">
                <span>🎙</span>
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
              {micLocked && <StatusBanner key="mic" color="#e05252" text="🔇 Микрофон заблокирован преподавателем" />}
              {listening && <StatusBanner key="listen" color="#e6a84a" text="🔴 Учитель слушает ваш микрофон в реальном времени" />}
              {partner && <StatusBanner key="partner" color="#a78bfa" text={`🔗 В группе с: ${partner}`} />}
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
                    <motion.div
                      className="absolute inset-0"
                      style={{ background: "linear-gradient(120deg, #2a2140, #1a1a2e, #241a38)", backgroundSize: "200% 200%" }}
                      animate={{ backgroundPosition: ["0% 50%", "100% 50%", "0% 50%"] }}
                      transition={{ duration: 6, repeat: Infinity, ease: "linear" }}
                    />
                  )}
                  <span className="absolute left-3 top-3 rounded-md bg-black/50 px-2 py-1 text-xs font-medium text-violet-300 backdrop-blur">
                    🖥 Демонстрация экрана: {teacherLabel}
                  </span>
                  {!session.frame && <span className="relative text-sm text-white/60">Подключение…</span>}
                </>
              ) : (
                <motion.div
                  className="flex flex-col items-center gap-2 text-[var(--color-text-muted)]"
                  animate={{ opacity: [0.4, 0.8, 0.4] }}
                  transition={{ duration: 2.2, repeat: Infinity, ease: "easeInOut" }}
                >
                  <span className="text-3xl">🖥</span>
                  <span className="text-sm">Нет активной трансляции</span>
                </motion.div>
              )}
            </Panel>

            {assignment && (
              <Panel>
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-3">
                    <span
                      className="rounded-md px-2 py-0.5 text-xs font-medium"
                      style={{ backgroundColor: `${KIND_META[assignment.kind].color}26`, color: KIND_META[assignment.kind].color }}
                    >
                      {KIND_META[assignment.kind].label}
                    </span>
                    <span className="font-medium">{assignment.title}</span>
                  </div>
                  <Button variant="secondary" className="px-3 py-1.5 text-sm" onClick={() => setAssignmentOpen((v) => !v)}>
                    {assignmentOpen ? "Свернуть" : "Начать"}
                  </Button>
                </div>
                <AnimatePresence>
                  {assignmentOpen && (
                    <motion.div
                      initial={{ opacity: 0, height: 0 }}
                      animate={{ opacity: 1, height: "auto" }}
                      exit={{ opacity: 0, height: 0 }}
                      className="overflow-hidden"
                    >
                      <p className="mt-4 rounded-lg bg-white/5 px-3 py-2 text-sm text-[var(--color-text-muted)]">
                        Прохождение задания появится на следующем шаге — здесь будет сам вопрос и вариант ответа.
                      </p>
                    </motion.div>
                  )}
                </AnimatePresence>
              </Panel>
            )}

            <div className="mt-auto flex items-center justify-center gap-3 pt-4">
              <motion.div whileTap={{ scale: 0.95 }}>
                <button
                  type="button"
                  onClick={() => setHandRaised((v) => !v)}
                  className={
                    "rounded-xl px-5 py-3 font-medium transition-colors " +
                    (handRaised ? "bg-amber-400/20 text-amber-300" : "bg-white/5 text-[var(--color-text-muted)] hover:bg-white/10")
                  }
                >
                  <motion.span
                    className="inline-block"
                    animate={handRaised ? { rotate: [0, -12, 12, -8, 0] } : { rotate: 0 }}
                    transition={{ duration: 0.5 }}
                  >
                    ✋
                  </motion.span>{" "}
                  {handRaised ? "Рука поднята" : "Поднять руку"}
                </button>
              </motion.div>

              <motion.button
                type="button"
                whileTap={{ scale: 0.85 }}
                onClick={() => fireReaction("👍")}
                className="rounded-xl bg-white/5 px-4 py-3 text-xl hover:bg-white/10"
                title="Понял"
              >
                👍
              </motion.button>
              <motion.button
                type="button"
                whileTap={{ scale: 0.85 }}
                onClick={() => fireReaction("❓")}
                className="rounded-xl bg-white/5 px-4 py-3 text-xl hover:bg-white/10"
                title="Не понял"
              >
                ❓
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
            <motion.p initial={{ scale: 0.9 }} animate={{ scale: 1 }} className="text-3xl font-semibold text-white">
              🔒 Экран заблокирован преподавателем
            </motion.p>
          </motion.div>
        )}
      </AnimatePresence>

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
        hasAssignment={hasAssignment}
        setHasAssignment={setHasAssignment}
      />
    </div>
  );
}

function StatusBanner({ color, text }: { color: string; text: string }) {
  return (
    <motion.div
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: "auto" }}
      exit={{ opacity: 0, height: 0 }}
      className="overflow-hidden"
    >
      <div className="rounded-xl px-4 py-2.5 text-sm font-medium" style={{ backgroundColor: `${color}1f`, color }}>
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
              ✕
            </button>
          </div>
          <div className="flex flex-col gap-2 text-sm">
            <DemoToggle label="Учитель слушает" checked={props.listening} onChange={props.setListening} />
            <DemoToggle label="Экран заблокирован" checked={props.screenLocked} onChange={props.setScreenLocked} />
            <DemoToggle label="Микрофон заблокирован" checked={props.micLocked} onChange={props.setMicLocked} />
            <DemoToggle label="Демонстрация экрана" checked={props.demoActive} onChange={props.setDemoActive} />
            <DemoToggle label="Есть задание" checked={props.hasAssignment} onChange={props.setHasAssignment} />
            <div>
              <label className="mb-1 block text-xs text-[var(--color-text-muted)]">В группе с</label>
              <select
                value={props.partner ?? ""}
                onChange={(e) => props.setPartner(e.target.value || null)}
                className="w-full rounded-lg border border-[var(--color-border-subtle)] bg-black/20 px-2 py-1.5 text-sm outline-none focus:border-violet-400"
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
          className="rounded-full border border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] px-4 py-2 text-xs font-medium text-[var(--color-text-muted)] shadow-lg hover:text-[var(--color-text-primary)]"
        >
          🧪 Демо-переключатели
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
