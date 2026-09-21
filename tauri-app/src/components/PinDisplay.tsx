import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { copyText } from "../lib/copyText";

type CopyState = "idle" | "copied" | "failed";

/** "Скопировать" button for the lesson PIN — one click puts it on the clipboard and the label flips to a
 * short "Скопировано" confirmation, then back. */
function CopyPinButton({ pin, className = "" }: { pin: string; className?: string }) {
  const [state, setState] = useState<CopyState>("idle");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  async function copy() {
    const ok = await copyText(pin);
    setState(ok ? "copied" : "failed");
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setState("idle"), 1800);
  }

  const label = state === "copied" ? "✓ Скопировано" : state === "failed" ? "Не удалось скопировать" : "📋 Скопировать";
  const tone =
    state === "copied"
      ? "bg-emerald-400/15 text-emerald-300"
      : state === "failed"
        ? "bg-rose-400/15 text-rose-300"
        : "bg-white/5 text-[var(--color-text-muted)] hover:bg-white/10 hover:text-[var(--color-text-primary)]";

  return (
    <button
      type="button"
      onClick={copy}
      aria-live="polite"
      title="Скопировать PIN в буфер обмена"
      className={
        "inline-flex items-center justify-center whitespace-nowrap rounded-lg px-2.5 py-1 text-xs font-medium outline-none " +
        "transition-colors focus-visible:ring-2 focus-visible:ring-violet-400 active:scale-95 " +
        tone +
        " " +
        className
      }
    >
      <AnimatePresence mode="wait" initial={false}>
        <motion.span
          key={state}
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -4 }}
          transition={{ duration: 0.12 }}
        >
          {label}
        </motion.span>
      </AnimatePresence>
    </button>
  );
}

/** Compact PIN chip for the class header: `PIN 123456 [Скопировать]`. */
export function PinChip({ pin }: { pin: string }) {
  return (
    <span className="inline-flex items-center gap-2">
      <span className="text-[var(--color-text-muted)]">PIN</span>
      <span className="font-mono text-base font-semibold tracking-[0.2em] text-[var(--color-text-primary)]">{pin}</span>
      <CopyPinButton pin={pin} />
    </span>
  );
}

/** Big, hard-to-miss PIN for the waiting-for-students state. */
export function PinHero({ pin }: { pin: string }) {
  return (
    <div className="flex flex-col items-center gap-3">
      <div className="text-xs font-medium uppercase tracking-widest text-[var(--color-text-muted)]">PIN урока</div>
      {/* `pl` balances the trailing letter-spacing so the digits look centred. */}
      <div className="pl-[0.3em] font-mono text-6xl font-bold tracking-[0.3em] text-violet-300 select-all">{pin}</div>
      <CopyPinButton pin={pin} className="px-3.5 py-1.5 text-sm" />
    </div>
  );
}
