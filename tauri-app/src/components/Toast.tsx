import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

export type ToastKind = "help" | "info";

export interface ToastItem {
  id: number;
  kind: ToastKind;
  text: string;
}

const MAX_VISIBLE = 3;
// A raised hand stays up longer than a plain "joined" note — it is the one the teacher must not miss.
const LIFETIME_MS: Record<ToastKind, number> = { help: 8000, info: 4000 };

/** Toast queue: `push` shows a toast (auto-dismissed after a few seconds, newest last, at most
 * `MAX_VISIBLE` on screen), `dismiss` closes one early. Render the result with `<ToastStack>`. */
export function useToasts() {
  const [items, setItems] = useState<ToastItem[]>([]);
  const nextId = useRef(1);
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());

  const dismiss = useCallback((id: number) => {
    clearTimeout(timers.current.get(id));
    timers.current.delete(id);
    setItems((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const push = useCallback(
    (kind: ToastKind, text: string) => {
      const id = nextId.current++;
      setItems((prev) => [...prev, { id, kind, text }].slice(-MAX_VISIBLE));
      timers.current.set(id, setTimeout(() => dismiss(id), LIFETIME_MS[kind]));
    },
    [dismiss],
  );

  useEffect(() => {
    const active = timers.current;
    return () => active.forEach(clearTimeout);
  }, []);

  return { items, push, dismiss };
}

const STYLE: Record<ToastKind, { icon: string; ring: string; iconBg: string }> = {
  help: { icon: "✋", ring: "border-amber-400/40", iconBg: "bg-amber-400/20 text-amber-300" },
  info: { icon: "👋", ring: "border-violet-400/30", iconBg: "bg-violet-400/20 text-violet-300" },
};

/** Top-right toast stack — above the chat drawer (`z-50`) so a raised hand is never hidden behind it. */
export function ToastStack({ items, onDismiss }: { items: ToastItem[]; onDismiss: (id: number) => void }) {
  return (
    <div className="pointer-events-none fixed right-4 top-4 z-[60] flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2">
      <AnimatePresence initial={false}>
        {items.map((t) => (
          <motion.div
            key={t.id}
            layout
            role="status"
            initial={{ opacity: 0, x: 48, scale: 0.96 }}
            animate={{ opacity: 1, x: 0, scale: 1 }}
            exit={{ opacity: 0, x: 48, scale: 0.96 }}
            transition={{ type: "spring", stiffness: 380, damping: 30 }}
            className={
              "pointer-events-auto flex items-center gap-3 rounded-xl border bg-[var(--color-card-solid)] px-3.5 py-3 shadow-2xl shadow-black/40 " +
              STYLE[t.kind].ring
            }
          >
            <span className={"flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-base " + STYLE[t.kind].iconBg}>
              {STYLE[t.kind].icon}
            </span>
            <span className="min-w-0 flex-1 text-sm leading-snug">{t.text}</span>
            <button
              type="button"
              onClick={() => onDismiss(t.id)}
              aria-label="Закрыть уведомление"
              className="shrink-0 rounded-md px-1.5 py-0.5 text-[var(--color-text-muted)] outline-none transition-colors hover:bg-white/10 hover:text-[var(--color-text-primary)] focus-visible:ring-2 focus-visible:ring-violet-400"
            >
              ✕
            </button>
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}
