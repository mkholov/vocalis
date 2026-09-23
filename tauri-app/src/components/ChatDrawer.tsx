import { useEffect, useState, type FormEvent } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { ArrowRight, Send, X } from "lucide-react";

const WHOLE_CLASS = "Весь класс";

interface Message {
  id: number;
  from: string;
  /** `WHOLE_CLASS`, or a student's real UUID (`LiveStudent.realId`) — never a bare name, so two students who
   * happen to share a name can't be confused. */
  toId: string;
  /** The recipient's display name at the time this message was sent — kept alongside `toId` so history
   * still reads correctly after that student disconnects and drops out of `students`. */
  toName: string;
  text: string;
}

/** The little a `ChatDrawer` needs to know about a real connected student — the same shape
 * `LiveStudent` already has, kept minimal here so this component doesn't depend on the whole
 * `useLiveClassroom` module for two fields. */
export interface ChatStudent {
  realId: string;
  name: string;
}

const INITIAL_MESSAGES: Message[] = [{ id: 1, from: "Система", toId: WHOLE_CLASS, toName: WHOLE_CLASS, text: "Урок начался." }];

/** Step 5 of the Tauri migration (vocalis_roadmap.md, section 8): teacher↔
 * class/student chat. `students` is the real connected roster (`TeacherConsole`'s own
 * `useLiveClassroom`, the same source the class grid uses) — no more invented names. Sending itself is
 * still local state only: there's no `ChatMessage`-over-the-network wiring into this Tauri command bridge
 * yet (the protocol already has `ClientToServer::ChatMessage`/receiving it is a separate, real network
 * step for later — this UI is ready for it, but doesn't send anything over the wire today).
 * Overlays as a right-hand drawer over whichever teacher screen is active, rather than being its own nav
 * tab, so it's reachable from anywhere. */
export function ChatDrawer({ open, onClose, students }: { open: boolean; onClose: () => void; students: ChatStudent[] }) {
  const [messages, setMessages] = useState<Message[]>(INITIAL_MESSAGES);
  const [target, setTarget] = useState(WHOLE_CLASS);
  const [text, setText] = useState("");

  // A student who disconnects while selected as the target must not leave the picker pointing at someone
  // who's no longer there.
  useEffect(() => {
    if (target !== WHOLE_CLASS && !students.some((s) => s.realId === target)) setTarget(WHOLE_CLASS);
  }, [students, target]);

  function send(e: FormEvent) {
    e.preventDefault();
    const trimmed = text.trim();
    if (!trimmed) return;
    const toName = target === WHOLE_CLASS ? WHOLE_CLASS : (students.find((s) => s.realId === target)?.name ?? WHOLE_CLASS);
    setMessages((prev) => [...prev, { id: Date.now(), from: "Вы", toId: target, toName, text: trimmed }]);
    setText("");
  }

  const targetName = target === WHOLE_CLASS ? WHOLE_CLASS : (students.find((s) => s.realId === target)?.name ?? WHOLE_CLASS);
  const visible = messages.filter((m) => m.toId === WHOLE_CLASS || m.toId === target);

  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={onClose}
            className="fixed inset-0 z-40 bg-black/50"
          />
          <motion.div
            initial={{ x: "100%" }}
            animate={{ x: 0 }}
            exit={{ x: "100%" }}
            transition={{ type: "spring", stiffness: 320, damping: 34 }}
            className="fixed right-0 top-0 z-50 flex h-full w-full max-w-sm flex-col border-l border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] p-5"
          >
            <div className="mb-4 flex items-center justify-between">
              <h2 className="text-lg font-semibold">Чат</h2>
              <button
                type="button"
                onClick={onClose}
                className="inline-flex items-center justify-center rounded-lg p-1.5 text-[var(--color-text-muted)] hover:bg-overlay hover:text-[var(--color-text-primary)]"
              >
                <X size={18} />
              </button>
            </div>

            <select
              value={target}
              onChange={(e) => setTarget(e.target.value)}
              className="mb-4 rounded-xl border border-[var(--color-border-subtle)] bg-field px-3 py-2 text-sm outline-none focus:border-violet-400"
            >
              <option value={WHOLE_CLASS}>{WHOLE_CLASS}</option>
              {students.length === 0 && (
                <option value="" disabled>
                  Никто не подключён
                </option>
              )}
              {students.map((s) => (
                <option key={s.realId} value={s.realId}>
                  {s.name}
                </option>
              ))}
            </select>

            <div className="flex-1 overflow-y-auto pr-1">
              <AnimatePresence initial={false}>
                {visible.map((m) => (
                  <motion.div
                    key={m.id}
                    layout
                    initial={{ opacity: 0, y: 8 }}
                    animate={{ opacity: 1, y: 0 }}
                    className="mb-2 rounded-xl bg-overlay px-3 py-2 text-sm"
                  >
                    <div className="mb-0.5 flex items-center gap-1.5 text-xs text-[var(--color-text-muted)]">
                      <span className="font-medium text-accent-text">{m.from}</span>
                      <span className="inline-flex items-center gap-1">
                        <ArrowRight size={12} />
                        {m.toName}
                      </span>
                    </div>
                    {m.text}
                  </motion.div>
                ))}
              </AnimatePresence>
            </div>

            <form onSubmit={send} className="mt-3 flex gap-2">
              <input
                value={text}
                onChange={(e) => setText(e.target.value)}
                placeholder={`Сообщение (${targetName})`}
                className="flex-1 rounded-xl border border-[var(--color-border-subtle)] bg-field px-3 py-2 text-sm outline-none focus:border-violet-400"
              />
              <button type="submit" className="inline-flex items-center justify-center rounded-xl bg-violet-500 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-violet-400">
                <Send size={16} />
              </button>
            </form>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
