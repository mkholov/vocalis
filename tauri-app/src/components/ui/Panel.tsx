import type { ReactNode } from "react";

/** Same surface treatment as `Card` (border, blur, soft shadow) but without
 * `Card`'s fixed max-width or its own entrance/exit animation — for
 * full-width page sections (Задания/Статистика/Настройки) where the parent
 * screen already handles the transition. */
export function Panel({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div
      className={
        "rounded-2xl border border-[var(--color-border-subtle)] bg-[var(--color-card)] p-6 " +
        "shadow-lg shadow-black/20 backdrop-blur-xl " +
        className
      }
    >
      {children}
    </div>
  );
}
