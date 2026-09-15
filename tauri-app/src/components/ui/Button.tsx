import { motion, type HTMLMotionProps } from "framer-motion";
import type { ReactNode } from "react";

type Variant = "primary" | "secondary" | "ghost";

interface ButtonProps extends Omit<HTMLMotionProps<"button">, "children"> {
  variant?: Variant;
  children: ReactNode;
}

const variantClasses: Record<Variant, string> = {
  primary:
    "bg-violet-500 text-white shadow-lg shadow-violet-500/25 hover:bg-violet-400 " +
    "disabled:bg-violet-500/40 disabled:shadow-none disabled:cursor-not-allowed",
  secondary:
    "border border-[var(--color-border-subtle)] bg-white/5 text-[var(--color-text-primary)] " +
    "hover:bg-white/10 disabled:opacity-40 disabled:cursor-not-allowed",
  ghost:
    "text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)] hover:bg-white/5 " +
    "disabled:opacity-40 disabled:cursor-not-allowed",
};

/** Shared button: consistent hover/tap motion + focus ring across the app,
 * three variants for the visual hierarchy the roadmap asked for (a primary
 * "Начать урок"-style action shouldn't look like a "Назад" link). */
export function Button({ variant = "primary", className = "", disabled, ...props }: ButtonProps) {
  return (
    <motion.button
      whileHover={disabled ? undefined : { scale: 1.02 }}
      whileTap={disabled ? undefined : { scale: 0.97 }}
      transition={{ type: "spring", stiffness: 500, damping: 30 }}
      disabled={disabled}
      className={
        "rounded-xl px-5 py-3 font-medium outline-none transition-colors " +
        "focus-visible:ring-2 focus-visible:ring-violet-400 focus-visible:ring-offset-2 " +
        "focus-visible:ring-offset-[var(--color-app)] " +
        variantClasses[variant] +
        " " +
        className
      }
      {...props}
    />
  );
}
