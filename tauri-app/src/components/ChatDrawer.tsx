import { useState, type FormEvent } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { ArrowRight, Send, X } from "lucide-react";
import { ROSTER } from "../lib/mockClassroom";

const WHOLE_CLASS = "Весь класс";

interface Message {
  id: number;
  from: string;
  to: string;
  text: string;
}

const INITIAL_MESSAGES: Message[] = [{ id: 1, from: "Система", to: WHOLE_CLASS, text: "Урок начался." }];

/** Step 5 of the Tauri migration (vocalis_roadmap.md, section 8): teacher↔
 * class/student chat. Local state only — there's no network session to send
 * these over yet, that's step 7 (or whenever real data starts flowing).
 * Overlays as a right-hand drawer over whichever teacher screen is active,
 * rather than being its own nav tab, so it's reachable from anywhere. */
export function ChatDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [messages, setMessages] = useState<Message[]>(INITIAL_MESSAGES);
  const [target, setTarget] = useState(WHOLE_CLASS);
  const [text, setText] = useState("");

  function send(e: FormEvent) {
    e.preventDefault();
    const trimmed = text.trim();
    if (!trimmed) return;
    setMessages((prev) => [...prev, { id: Date.now(), from: "Вы", to: target, text: trimmed }]);
    setText("");
  }

  const visible = messages.filter((m) => m.to === WHOLE_CLASS || m.to === target);

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
              <option>{WHOLE_CLASS}</option>
              {ROSTER.map((name) => (
                <option key={name}>{name}</option>
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
                        {m.to}
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
                placeholder={`Сообщение (${target})`}
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
