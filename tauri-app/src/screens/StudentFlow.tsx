import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { ArrowLeft, Check, Loader2, UserRound } from "lucide-react";
import { Card } from "../components/ui/Card";
import { Button } from "../components/ui/Button";
import { TextField } from "../components/ui/TextField";
import { discoverTeachers, fetchClassRoster, type DiscoveredTeacherDto } from "../lib/commands";

interface Props {
  onBack: () => void;
  /** Called once a real teacher is picked and the student has actually identified themselves (either by
   * tapping their own name in the real class roster, or — if they're not on it — typing one by hand):
   * hands back everything `useStudentSession` needs to actually dial that teacher for real (step 7 part
   * B, `vocalis_roadmap.md` section 8). `discoverTeachers` is already a real UDP listen, and the roster
   * itself (see `RosterStep` below) is a real, if disposable, probe connection — this just moves on once
   * a name is settled; the actual `connect_student_session` dial happens once `StudentConsole` mounts. */
  onConnect?: (studentName: string, teacher: DiscoveredTeacherDto, pin: string) => void;
}

const DISCOVERY_TIMEOUT_MS = 2500;

/** Student flow: PIN + live "who's on the LAN" list first, then — once a specific teacher accepts that
 * PIN — a real class roster to pick yourself from (step 8.x of the Tauri migration: a typo, a different
 * capitalization or a shortened name used to fragment one real student's history across several unrelated
 * rows; picking a name from the teacher's own roster instead of typing it fixes that at the source).
 * Matches the egui student screen's own layout and its already-fixed validation placement (errors sit
 * under the field they describe). `discover_teachers` re-polls itself on a timer since discovery is a
 * point-in-time UDP listen, not a subscription. */
export function StudentFlow({ onBack, onConnect }: Props) {
  const [pin, setPin] = useState("");
  const [touched, setTouched] = useState(false);

  const [teachers, setTeachers] = useState<DiscoveredTeacherDto[]>([]);
  const [scanning, setScanning] = useState(true);
  const [scanError, setScanError] = useState<string | undefined>();
  const cancelled = useRef(false);

  // Set once a teacher has actually accepted this PIN (the real roster probe succeeded) — switches the
  // card over to `RosterStep`. `null` means "still on the PIN/teacher-list step".
  const [rosterTarget, setRosterTarget] = useState<{ teacher: DiscoveredTeacherDto; pin: string; roster: string[] } | null>(null);
  const [pendingTeacherKey, setPendingTeacherKey] = useState<string | null>(null);
  const [rosterError, setRosterError] = useState<string | undefined>();

  useEffect(() => {
    cancelled.current = false;
    let timer: ReturnType<typeof setTimeout>;

    async function scan() {
      setScanning(true);
      try {
        const found = await discoverTeachers(DISCOVERY_TIMEOUT_MS);
        if (!cancelled.current) {
          setTeachers(found);
          setScanError(undefined);
        }
      } catch (err) {
        if (!cancelled.current) setScanError(String(err));
      } finally {
        if (!cancelled.current) {
          setScanning(false);
          timer = setTimeout(scan, 1500);
        }
      }
    }
    scan();

    return () => {
      cancelled.current = true;
      clearTimeout(timer);
    };
  }, []);

  const pinTrimmed = pin.trim();
  const pinValid = /^\d{4,6}$/.test(pinTrimmed);
  const pinError = touched && !pinValid ? "Введите PIN-код урока (4-6 цифр)" : undefined;

  async function pickTeacher(teacher: DiscoveredTeacherDto) {
    setTouched(true);
    if (!pinValid || pendingTeacherKey) return;
    const key = `${teacher.ip}:${teacher.controlPort}`;
    setPendingTeacherKey(key);
    setRosterError(undefined);
    try {
      const roster = await fetchClassRoster(teacher.ip, teacher.controlPort, pinTrimmed);
      setRosterTarget({ teacher, pin: pinTrimmed, roster });
    } catch (err) {
      setRosterError(String(err));
    } finally {
      setPendingTeacherKey(null);
    }
  }

  if (rosterTarget) {
    return (
      <RosterStep
        teacherName={rosterTarget.teacher.teacherName}
        roster={rosterTarget.roster}
        onBack={() => setRosterTarget(null)}
        onPick={(name) => onConnect?.(name, rosterTarget.teacher, rosterTarget.pin)}
      />
    );
  }

  return (
    <Card>
      <motion.div initial={{ opacity: 0, x: 24 }} animate={{ opacity: 1, x: 0 }} transition={{ type: "spring", stiffness: 300, damping: 30 }}>
        <h2 className="mb-1 text-xl font-semibold">Подключение к уроку</h2>
        <p className="mb-6 text-sm text-[var(--color-text-muted)]">Введите PIN-код от преподавателя, затем выберите его из списка.</p>

        <TextField
          label="PIN-код урока"
          autoFocus
          inputMode="numeric"
          value={pin}
          onChange={(e) => {
            setPin(e.target.value.replace(/[^\d]/g, "").slice(0, 6));
            setRosterError(undefined);
          }}
          onBlur={() => setTouched(true)}
          error={pinError}
          placeholder="123456"
          className="font-mono"
        />

        {rosterError && <p className="mt-3 rounded-lg bg-rose-400/10 px-3 py-2 text-sm text-danger-text">{rosterError}</p>}

        <div className="mt-7 flex items-center justify-between">
          <h3 className="text-sm font-medium text-[var(--color-text-muted)]">Преподаватели в сети</h3>
          {scanning && (
            <motion.span
              className="h-3 w-3 rounded-full border-2 border-violet-400 border-t-transparent"
              animate={{ rotate: 360 }}
              transition={{ repeat: Infinity, duration: 0.7, ease: "linear" }}
            />
          )}
        </div>

        {scanError && <p className="mt-2 rounded-lg bg-rose-400/10 px-3 py-2 text-sm text-danger-text">Ошибка поиска: {scanError}</p>}

        <div className="mt-3 min-h-[4rem]">
          <AnimatePresence mode="popLayout">
            {teachers.length === 0 && !scanError ? (
              <motion.p key="empty" exit={{ opacity: 0 }} className="text-sm text-[var(--color-text-muted)]">
                Пока никого не найдено. Убедитесь, что находитесь в одной сети с преподавателем.
              </motion.p>
            ) : (
              <motion.ul layout className="flex max-h-[30vh] flex-col gap-2 overflow-y-auto">
                {teachers.map((t) => {
                  const key = `${t.ip}:${t.controlPort}`;
                  const isPending = pendingTeacherKey === key;
                  return (
                    <motion.li
                      key={key}
                      layout
                      initial={{ opacity: 0, y: 8 }}
                      animate={{ opacity: 1, y: 0 }}
                      exit={{ opacity: 0 }}
                      className="flex items-center justify-between rounded-xl border border-[var(--color-border-subtle)] px-4 py-3"
                    >
                      <div>
                        <div className="font-medium">{t.teacherName}</div>
                        <div className="text-xs text-[var(--color-text-muted)]">{t.ip}</div>
                      </div>
                      <Button
                        variant="secondary"
                        className="px-3 py-1.5 text-sm"
                        disabled={!pinValid || Boolean(pendingTeacherKey)}
                        onClick={() => pickTeacher(t)}
                      >
                        {isPending ? <Loader2 size={15} className="animate-spin" /> : "Подключиться"}
                      </Button>
                    </motion.li>
                  );
                })}
              </motion.ul>
            )}
          </AnimatePresence>
        </div>

        <div className="mt-6">
          <Button type="button" variant="ghost" onClick={onBack}>
            <ArrowLeft size={16} />
                Назад
          </Button>
        </div>
      </motion.div>
    </Card>
  );
}

/** Shown once a teacher has accepted the PIN and handed back its real class roster. Picking a name is one
 * tap; "Меня нет в списке" reveals the old free-text field as an explicit fallback for a student the
 * teacher hasn't entered yet — exactly the same "вне ростера" case `teacher::net::handle_student` already
 * flags server-side (`RosterStatus::UnrecognizedPending`), just reachable on purpose now instead of by typo. */
function RosterStep({
  teacherName,
  roster,
  onBack,
  onPick,
}: {
  teacherName: string;
  roster: string[];
  onBack: () => void;
  onPick: (name: string) => void;
}) {
  const [manualEntry, setManualEntry] = useState(roster.length === 0);
  const [manualName, setManualName] = useState("");
  const [touched, setTouched] = useState(false);

  const manualTrimmed = manualName.trim();
  const manualError = touched && manualTrimmed.length === 0 ? "Введите имя, чтобы можно было подключиться" : undefined;

  function confirmManual() {
    setTouched(true);
    if (manualTrimmed.length === 0) return;
    onPick(manualTrimmed);
  }

  return (
    <Card>
      <motion.div initial={{ opacity: 0, x: 24 }} animate={{ opacity: 1, x: 0 }} transition={{ type: "spring", stiffness: 300, damping: 30 }}>
        <h2 className="mb-1 text-xl font-semibold">Кто вы?</h2>
        <p className="mb-6 text-sm text-[var(--color-text-muted)]">
          Класс преподавателя «{teacherName}» — выберите своё имя в списке.
        </p>

        {!manualEntry && (
          <>
            <ul className="flex max-h-[40vh] flex-col gap-2 overflow-y-auto">
              {roster.map((name) => (
                <motion.li key={name} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }}>
                  <button
                    type="button"
                    onClick={() => onPick(name)}
                    className="flex w-full items-center gap-3 rounded-xl border border-[var(--color-border-subtle)] px-4 py-3 text-left transition-colors hover:border-violet-400/60 hover:bg-overlay"
                  >
                    <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-overlay text-[var(--color-text-muted)]">
                      <UserRound size={16} />
                    </span>
                    <span className="font-medium">{name}</span>
                  </button>
                </motion.li>
              ))}
            </ul>

            <button
              type="button"
              onClick={() => setManualEntry(true)}
              className="mt-4 text-sm text-[var(--color-text-muted)] underline-offset-2 hover:text-accent-text hover:underline"
            >
              Меня нет в списке — ввести имя вручную
            </button>
          </>
        )}

        {manualEntry && (
          <div className="flex flex-col gap-4">
            {roster.length > 0 && (
              <p className="text-sm text-[var(--color-text-muted)]">
                Вы не в списке класса — преподаватель увидит, что это имя не совпадает с ростером.
              </p>
            )}
            <TextField
              label="Ваше имя и фамилия"
              autoFocus
              value={manualName}
              onChange={(e) => setManualName(e.target.value)}
              onBlur={() => setTouched(true)}
              error={manualError}
              placeholder="Иванов Пётр"
            />
            <div className="flex items-center gap-3">
              <Button onClick={confirmManual} disabled={manualTrimmed.length === 0}>
                <Check size={16} />
                Подключиться
              </Button>
              {roster.length > 0 && (
                <Button variant="ghost" onClick={() => setManualEntry(false)}>
                  Назад к списку
                </Button>
              )}
            </div>
          </div>
        )}

        <div className="mt-6">
          <Button type="button" variant="ghost" onClick={onBack}>
            <ArrowLeft size={16} />
                Назад
          </Button>
        </div>
      </motion.div>
    </Card>
  );
}
