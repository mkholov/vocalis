import { motion } from "framer-motion";
import { Hand, Headphones, Lock, LockOpen, Mic, MicOff, Phone, PhoneOff, Square } from "lucide-react";
import type { ReactNode } from "react";
import { VuMeter } from "./VuMeter";
import { avatarColor, initials } from "../lib/avatar";
import type { MockStudent, Presence } from "../lib/mockClassroom";

// Same three status colors as egui's theme.rs (WARN/OK, plus the accent for
// "speaking") — status colors there are deliberately theme-independent, so
// reusing the literal hex here (rather than inventing a new palette) keeps
// the two stacks reading the same way.
const Dot = () => <span className="h-2 w-2 rounded-full bg-current" />;
const PRESENCE: Record<Presence, { label: string; color: string; icon: ReactNode }> = {
  empty: { label: "Не подключен", color: "var(--color-text-muted)", icon: <Dot /> },
  needsHelp: { label: "Просит помощи", color: "var(--color-status-warn)", icon: <Hand size={12} /> },
  speaking: { label: "Говорит", color: "var(--color-status-accent)", icon: <Mic size={12} /> },
  connected: { label: "На связи", color: "var(--color-status-ok)", icon: <Dot /> },
};

/** Icon + label inside a card's small action buttons. */
const BTN = "flex flex-1 items-center justify-center gap-1.5 rounded-lg px-2 py-1.5 text-xs font-medium transition-colors active:scale-95 ";

interface Props {
  student: MockStudent;
  selected: boolean;
  onSelect: () => void;
  onToggleScreenLock: () => void;
  onToggleMicLock: () => void;
  /** Step 7.5: real-time listen-in (`teacher_session.rs`'s `start_listen`/
   * `stop_listen`) — `undefined` hides the button entirely (mock-mode
   * students have no real session to listen in on). */
  listening?: boolean;
  onToggleListen?: () => void;
  /** Step 7.5: private two-way intercom (`start_intercom`/`stop_intercom`) —
   * same `undefined`-hides-the-button convention as `listening` above. */
  intercomActive?: boolean;
  onToggleIntercom?: () => void;
}

export function StudentCard({
  student,
  selected,
  onSelect,
  onToggleScreenLock,
  onToggleMicLock,
  listening,
  onToggleListen,
  intercomActive,
  onToggleIntercom,
}: Props) {
  const empty = student.presence === "empty";
  const presence = PRESENCE[student.presence];

  return (
    <motion.div
      layout
      variants={{ hidden: { opacity: 0, y: 16, scale: 0.96 }, visible: { opacity: 1, y: 0, scale: 1 } }}
      whileTap={empty ? undefined : { scale: 0.98 }}
      onClick={empty ? undefined : onSelect}
      className={
        "flex min-h-[168px] flex-col rounded-2xl border p-4 transition-colors " +
        (empty
          ? "border-[var(--color-border-subtle)]/60 bg-subtle"
          : "cursor-pointer bg-[var(--color-card)] shadow-lg shadow-black/20 backdrop-blur-xl " +
            (selected ? "border-violet-400 ring-2 ring-violet-400/30" : "border-[var(--color-border-subtle)] hover:border-[var(--color-border-strong)]"))
      }
    >
      <div className="flex items-center justify-between text-xs text-[var(--color-text-muted)]">
        <span>#{student.seat}</span>
        {student.presence === "needsHelp" && (
          <span title="Просит помощи" className="text-warn-text">
            <Hand size={14} />
          </span>
        )}
      </div>

      {empty ? (
        <div className="flex flex-1 items-center justify-center text-sm text-[var(--color-text-muted)]">Свободно</div>
      ) : (
        <>
          <div className="mt-2 flex items-center gap-3">
            <div
              className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-sm font-semibold text-white"
              style={{ backgroundColor: avatarColor(student.name) }}
            >
              {initials(student.name)}
            </div>
            <div className="min-w-0">
              <div className="truncate font-medium">{student.name}</div>
              <div className="flex items-center gap-1 text-xs" style={{ color: presence.color }}>
                <span className="flex w-3 items-center justify-center">{presence.icon}</span>
                <span>{presence.label}</span>
              </div>
            </div>
          </div>

          <div className="mt-3">
            <VuMeter level={student.level} active={student.presence === "speaking"} />
          </div>

          <div className="mt-auto flex gap-2 pt-3">
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onToggleScreenLock();
              }}
              className={
                BTN +
                (student.screenLocked ? "bg-rose-400/20 text-danger-text" : "bg-overlay text-[var(--color-text-muted)] hover:bg-overlay-hover")
              }
            >
              {student.screenLocked ? <Lock size={13} /> : <LockOpen size={13} />}
              Экран
            </button>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onToggleMicLock();
              }}
              className={
                BTN +
                (student.micLocked ? "bg-rose-400/20 text-danger-text" : "bg-overlay text-[var(--color-text-muted)] hover:bg-overlay-hover")
              }
            >
              {student.micLocked ? <MicOff size={13} /> : <Mic size={13} />}
              Мик
            </button>
          </div>
          {(onToggleListen || onToggleIntercom) && (
            <div className="mt-2 flex gap-2">
              {onToggleListen && (
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggleListen();
                  }}
                  className={
                    BTN +
                    (listening ? "bg-violet-400/20 text-accent-text" : "bg-overlay text-[var(--color-text-muted)] hover:bg-overlay-hover")
                  }
                >
                  {listening ? <Square size={13} /> : <Headphones size={13} />}
                  {listening ? "Слушаю" : "Слушать"}
                </button>
              )}
              {onToggleIntercom && (
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggleIntercom();
                  }}
                  className={
                    BTN +
                    (intercomActive ? "bg-emerald-400/20 text-ok-text" : "bg-overlay text-[var(--color-text-muted)] hover:bg-overlay-hover")
                  }
                >
                  {intercomActive ? <PhoneOff size={13} /> : <Phone size={13} />}
                  {intercomActive ? "Завершить" : "Интерком"}
                </button>
              )}
            </div>
          )}
        </>
      )}
    </motion.div>
  );
}
