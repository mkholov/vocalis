/** Class list for a bare text input/textarea (no label wrapper) — the same look `TextField` gives its
 * input, for rows where a label per field would be noise (editor rows). */
export const inputClasses =
  "w-full rounded-xl border border-[var(--color-border-subtle)] bg-black/20 px-4 py-2.5 text-[var(--color-text-primary)] " +
  "placeholder:text-[var(--color-text-muted)]/60 outline-none transition-all duration-150 " +
  "focus:border-violet-400 focus:bg-black/30 focus:ring-4 focus:ring-violet-400/15";

/** Small icon-only button (delete row, etc.). */
export const iconButtonClasses =
  "inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-[var(--color-text-muted)] outline-none " +
  "transition-colors hover:bg-rose-400/15 hover:text-rose-300 focus-visible:ring-2 focus-visible:ring-violet-400 " +
  "disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-[var(--color-text-muted)]";
