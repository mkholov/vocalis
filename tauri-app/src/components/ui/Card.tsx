import { motion } from "framer-motion";
import type { ReactNode } from "react";

/** The one card shell every screen in this step uses — soft shadow, subtle
 * border, translucent background over the app's ambient glow. Consistent
 * entrance animation so every screen transition feels the same. */
export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 16, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: -12, scale: 0.98 }}
      transition={{ type: "spring", stiffness: 300, damping: 30 }}
      className={
        "w-full max-w-md rounded-2xl border border-[var(--color-border-subtle)] " +
        "bg-[var(--color-card)] p-8 shadow-2xl shadow-black/40 backdrop-blur-xl " +
        className
      }
    >
      {children}
    </motion.div>
  );
}
