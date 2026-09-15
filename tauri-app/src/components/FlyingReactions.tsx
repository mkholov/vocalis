import { AnimatePresence, motion } from "framer-motion";

export interface FlyingReaction {
  id: number;
  emoji: string;
  offsetX: number;
}

/** Renders a transient "pop and float up" animation per active reaction —
 * callers own the list (add on click, remove after the animation's own
 * duration via a timeout) so this stays a pure presentational piece. */
export function FlyingReactions({ reactions }: { reactions: FlyingReaction[] }) {
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-28 flex justify-center">
      <AnimatePresence>
        {reactions.map((r) => (
          <motion.div
            key={r.id}
            initial={{ opacity: 0, y: 0, scale: 0.4 }}
            animate={{ opacity: [0, 1, 1, 0], y: -130, scale: 1.4 }}
            transition={{ duration: 1.3, ease: "easeOut", times: [0, 0.15, 0.7, 1] }}
            style={{ position: "absolute", left: `calc(50% + ${r.offsetX}px)` }}
            className="text-4xl"
          >
            {r.emoji}
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}
