import { AnimatePresence, motion } from "framer-motion";
import type { LucideIcon } from "lucide-react";

export interface FlyingReaction {
  id: number;
  icon: LucideIcon;
  /** Tailwind text colour for the floating icon. */
  tone: string;
  offsetX: number;
}

/** Renders a transient "pop and float up" animation per active reaction —
 * callers own the list (add on click, remove after the animation's own
 * duration via a timeout) so this stays a pure presentational piece. */
export function FlyingReactions({ reactions }: { reactions: FlyingReaction[] }) {
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-28 flex justify-center">
      <AnimatePresence>
        {reactions.map(({ icon: Icon, ...r }) => (
          <motion.div
            key={r.id}
            initial={{ opacity: 0, y: 0, scale: 0.4 }}
            animate={{ opacity: [0, 1, 1, 0], y: -130, scale: 1.4 }}
            transition={{ duration: 1.3, ease: "easeOut", times: [0, 0.15, 0.7, 1] }}
            style={{ position: "absolute", left: `calc(50% + ${r.offsetX}px)` }}
            className={r.tone}
          >
            <Icon size={40} strokeWidth={1.75} fill="currentColor" fillOpacity={0.15} />
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}
