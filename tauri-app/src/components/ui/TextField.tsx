import { AnimatePresence, motion } from "framer-motion";
import type { InputHTMLAttributes } from "react";

interface TextFieldProps extends InputHTMLAttributes<HTMLInputElement> {
  label: string;
  /** Shown right under this field, not batched with other fields' errors —
   * see the egui app's own UI-polish pass (app.rs's student connect screen)
   * for why that matters: a validation message far from the field it
   * describes is easy to misread as belonging to the wrong one. */
  error?: string;
}

/** Text input with a focus glow, monospace-friendly for PIN-style fields via
 * `inputMode`, and an error message that animates in/out directly beneath
 * it rather than popping in instantly. */
export function TextField({ label, error, id, className = "", ...props }: TextFieldProps) {
  const inputId = id ?? `field-${label.replace(/\s+/g, "-").toLowerCase()}`;
  return (
    <div>
      <label htmlFor={inputId} className="mb-1.5 block text-sm font-medium text-[var(--color-text-muted)]">
        {label}
      </label>
      <input
        id={inputId}
        className={
          "w-full rounded-xl border bg-field px-4 py-2.5 text-[var(--color-text-primary)] " +
          "placeholder:text-[var(--color-text-muted)]/60 outline-none transition-all duration-150 " +
          "focus:bg-field-focus focus:ring-4 " +
          (error
            ? "border-rose-400/60 focus:border-rose-400 focus:ring-rose-400/15"
            : "border-[var(--color-border-subtle)] focus:border-violet-400 focus:ring-violet-400/15") +
          " " +
          className
        }
        {...props}
      />
      <AnimatePresence>
        {error && (
          <motion.p
            initial={{ opacity: 0, height: 0, marginTop: 0 }}
            animate={{ opacity: 1, height: "auto", marginTop: 6 }}
            exit={{ opacity: 0, height: 0, marginTop: 0 }}
            transition={{ duration: 0.15 }}
            className="overflow-hidden text-sm text-danger-text"
          >
            {error}
          </motion.p>
        )}
      </AnimatePresence>
    </div>
  );
}
