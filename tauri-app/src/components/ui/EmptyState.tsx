import type { ReactNode } from "react";
import { motion } from "framer-motion";

interface Props {
  /** A lucide icon element, e.g. `<Music />` — sized here. */
  icon: ReactNode;
  title: string;
  hint?: string;
  /** Optional call to action (a `<Button>`), shown under the text. */
  action?: ReactNode;
  /** Tighter padding/type for empty states inside a small floating panel. */
  compact?: boolean;
}

/** "Nothing here yet" block — the same idea as the class grid's waiting banner (say what's missing and
 * what to do about it, instead of leaving a blank area that reads as a bug). */
export function EmptyState({ icon, title, hint, action, compact = false }: Props) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.2 }}
      className={
        "flex flex-col items-center rounded-xl border border-dashed border-[var(--color-border-subtle)] text-center " +
        (compact ? "gap-1.5 px-4 py-5" : "gap-2 px-6 py-9")
      }
    >
      <div
        className={"flex items-center justify-center rounded-full bg-overlay text-[var(--color-text-muted)] " + (compact ? "h-10 w-10 [&>svg]:h-5 [&>svg]:w-5" : "h-14 w-14 [&>svg]:h-7 [&>svg]:w-7")}
        aria-hidden
      >
        {icon}
      </div>
      <div className={"font-medium " + (compact ? "text-sm" : "")}>{title}</div>
      {hint && <p className="max-w-sm text-sm text-[var(--color-text-muted)]">{hint}</p>}
      {action && <div className="mt-2">{action}</div>}
    </motion.div>
  );
}
