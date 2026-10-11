import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Check, Copy, TriangleAlert } from "lucide-react";
import { copyText } from "../lib/copyText";

type CopyState = "idle" | "copied" | "failed";

/** "Скопировать" button for the lesson PIN — one click puts it on the clipboard and the label flips to a
 * short "Скопировано" confirmation, then back. */
function CopyPinButton({ pin, className = "" }: { pin: string; className?: string }) {
  const [state, setState] = useState<CopyState>("idle");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  async function copy() {
    const ok = await copyText(pin);
    setState(ok ? "copied" : "failed");
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setState("idle"), 1800);
  }

  const label = state === "copied" ? "Скопировано" : state === "failed" ? "Не удалось скопировать" : "Скопировать";
  const Icon = state === "copied" ? Check : state === "failed" ? TriangleAlert : Copy;
  const tone =
    state === "copied"
      ? "bg-emerald-400/15 text-ok-text"
      : state === "failed"
        ? "bg-rose-400/15 text-danger-text"
        : "bg-overlay text-[var(--color-text-muted)] hover:bg-overlay-hover hover:text-[var(--color-text-primary)]";

  return (
    <button
      type="button"
      onClick={copy}
      aria-live="polite"
      title="Скопировать PIN в буфер обмена"
      className={
        "inline-flex items-center justify-center whitespace-nowrap rounded-lg px-2.5 py-1 text-xs font-medium outline-none " +
        "transition-colors focus-visible:ring-2 focus-visible:ring-violet-400 active:scale-95 " +
        tone +
        " " +
        className
      }
    >
      <AnimatePresence mode="wait" initial={false}>
        <motion.span
          key={state}
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -4 }}
          transition={{ duration: 0.12 }}
          className="inline-flex items-center gap-1.5"
        >
          <Icon size={13} />
          {label}
        </motion.span>
      </AnimatePresence>
    </button>
  );
}

/** "Скопировать" for an IP address — same idea as `CopyPinButton`, smaller (an IP is read out far less
 * often than a PIN, this is a convenience for the rare case, not a primary action). */
function CopyIpButton({ ip }: { ip: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  async function copy() {
    if (await copyText(ip)) {
      setCopied(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 1400);
    }
  }
  return (
    <button
      type="button"
      onClick={copy}
      title="Скопировать IP в буфер обмена"
      className="inline-flex items-center justify-center rounded-md p-0.5 text-[var(--color-text-muted)] outline-none transition-colors hover:bg-overlay-hover hover:text-[var(--color-text-primary)] focus-visible:ring-2 focus-visible:ring-violet-400"
    >
      {copied ? <Check size={11} /> : <Copy size={11} />}
    </button>
  );
}

/** Compact PIN chip for the class header: `PIN 123456 [Скопировать]`. */
export function PinChip({ pin }: { pin: string }) {
  return (
    <span className="inline-flex items-center gap-2">
      <span className="text-[var(--color-text-muted)]">PIN</span>
      <span className="font-mono text-base font-semibold tracking-[0.2em] text-[var(--color-text-primary)]">{pin}</span>
      <CopyPinButton pin={pin} />
    </span>
  );
}

/** This machine's candidate LAN IPs, compact — for a teacher who needs to help a late-joining student
 * whose LAN discovery isn't finding anyone (see `IpHero`'s own doc comment for the full "why"). Only the
 * first (best-guess) address by default; `+N ещё` reveals the rest, for the case that guess is wrong. */
export function IpChip({ ips }: { ips: string[] }) {
  const [expanded, setExpanded] = useState(false);
  if (ips.length === 0) return null;
  const shown = expanded ? ips : ips.slice(0, 1);
  return (
    <span className="inline-flex flex-wrap items-center gap-1.5">
      <span className="text-[var(--color-text-muted)]">IP</span>
      {shown.map((ip) => (
        <span key={ip} className="inline-flex items-center gap-1 font-mono text-sm text-[var(--color-text-primary)]">
          {ip}
          <CopyIpButton ip={ip} />
        </span>
      ))}
      {!expanded && ips.length > 1 && (
        <button type="button" onClick={() => setExpanded(true)} className="text-xs text-[var(--color-text-muted)] underline-offset-2 hover:underline">
          +{ips.length - 1} ещё
        </button>
      )}
    </span>
  );
}

/** Big, hard-to-miss PIN for the waiting-for-students state. */
export function PinHero({ pin }: { pin: string }) {
  return (
    <div className="flex flex-col items-center gap-3">
      <div className="text-xs font-medium uppercase tracking-widest text-[var(--color-text-muted)]">PIN урока</div>
      {/* `pl` balances the trailing letter-spacing so the digits look centred. */}
      <div className="pl-[0.3em] font-mono text-6xl font-bold tracking-[0.3em] text-accent-text select-all">{pin}</div>
      <CopyPinButton pin={pin} className="px-3.5 py-1.5 text-sm" />
    </div>
  );
}

/** Shown under `PinHero` — "if the student list on their screen is empty, here's what to read out instead".
 * Real reason this exists: LAN discovery (a UDP broadcast) silently fails under Wi-Fi client/AP isolation,
 * which many school networks enable — this app has no way to detect that from inside the process, so the
 * mitigation is making manual IP+PIN entry (the student side's own "Ввести адрес учителя вручную") actually
 * usable, which means the teacher needs to be able to read the IP out in the first place. Not shown at all
 * if no usable network interface was found (`ips` empty) — nothing to read out then anyway. */
export function IpHero({ ips }: { ips: string[] }) {
  const [expanded, setExpanded] = useState(false);
  if (ips.length === 0) return null;
  const shown = expanded ? ips : ips.slice(0, 1);
  return (
    <div className="flex flex-col items-center gap-1.5">
      <div className="text-xs text-[var(--color-text-muted)]">
        Если ученик не видит вас в списке — пусть введёт адрес вручную:
      </div>
      <div className="flex flex-wrap items-center justify-center gap-x-3 gap-y-1">
        {shown.map((ip) => (
          <span key={ip} className="inline-flex items-center gap-1.5 rounded-lg bg-overlay px-2.5 py-1 font-mono text-sm text-[var(--color-text-primary)]">
            {ip}
            <CopyIpButton ip={ip} />
          </span>
        ))}
      </div>
      {!expanded && ips.length > 1 && (
        <button type="button" onClick={() => setExpanded(true)} className="text-xs text-[var(--color-text-muted)] underline-offset-2 hover:underline">
          Показать ещё {ips.length - 1}
        </button>
      )}
    </div>
  );
}
