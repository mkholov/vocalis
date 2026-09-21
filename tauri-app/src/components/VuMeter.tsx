import { motion } from "framer-motion";

// Same center-peak silhouette + ramp-depth coloring as the egui app's
// `theme::wave_meter` (center brightest, edges darkest) — a deliberate shared
// visual language between the two stacks, not a coincidence.
const SHAPE = [0.35, 0.55, 0.8, 1.0, 0.8, 0.55, 0.35];
const MAX_HEIGHT = 20;

function barColor(index: number, active: boolean) {
  if (!active) return "transparent";
  if (index === 3) return "var(--color-accent-text)"; // peak — theme::ACCENT_300
  if (index === 2 || index === 4) return "var(--color-accent)"; // theme::ACCENT
  return "color-mix(in srgb, var(--color-accent) 82%, black)"; // theme::ACCENT_500
}

/** A small VU meter: `level` is 0-100, `active` gates whether anything is
 * lit at all (an idle/empty seat shows the bare silhouette). Each bar
 * animates its fill height with a spring, so a jump in `level` reads as a
 * bounce rather than a snap. */
export function VuMeter({ level, active }: { level: number; active: boolean }) {
  const fraction = Math.max(0, Math.min(100, level)) / 100;
  return (
    <div className="flex h-5 items-end gap-[3px]" aria-hidden>
      {SHAPE.map((shape, i) => {
        const trackHeight = MAX_HEIGHT * shape;
        const fillHeight = active ? Math.max(fraction * trackHeight, fraction > 0.02 ? 2 : 0) : 0;
        return (
          <div key={i} className="relative w-[6px] overflow-hidden rounded-sm bg-overlay-hover" style={{ height: trackHeight }}>
            <motion.div
              className="absolute bottom-0 w-full rounded-sm"
              style={{ backgroundColor: barColor(i, active) }}
              animate={{ height: fillHeight }}
              transition={{ type: "spring", stiffness: 320, damping: 22 }}
            />
          </div>
        );
      })}
    </div>
  );
}
