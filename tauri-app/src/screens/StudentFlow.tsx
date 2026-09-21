import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Card } from "../components/ui/Card";
import { Button } from "../components/ui/Button";
import { TextField } from "../components/ui/TextField";
import { discoverTeachers, type DiscoveredTeacherDto } from "../lib/commands";

interface Props {
  onBack: () => void;
  /** Called once a real teacher is picked from the discovered list — hands
   * back everything `useStudentSession` needs to actually dial that teacher
   * for real (step 7 part B, `vocalis_roadmap.md` section 8): the chosen
   * `DiscoveredTeacherDto` (ip/controlPort/teacherName) and the entered PIN,
   * alongside the student's name. `discoverTeachers` is already a real UDP
   * listen — this just moves on to the next screen, the actual
   * `connect_student_session` call happens once `StudentConsole` mounts. */
  onConnect?: (studentName: string, teacher: DiscoveredTeacherDto, pin: string) => void;
}

const DISCOVERY_TIMEOUT_MS = 2500;

/** Student flow: name + lesson PIN, with a live "who's on the LAN" list —
 * matches the egui student screen's own layout and its already-fixed
 * validation placement (errors sit under the field they describe, not
 * batched at the bottom — see `app.rs`'s student connect-screen polish
 * pass). `discover_teachers` re-polls itself on a timer since discovery is
 * a point-in-time UDP listen, not a subscription. */
export function StudentFlow({ onBack, onConnect }: Props) {
  const [name, setName] = useState("");
  const [pin, setPin] = useState("");
  const [touched, setTouched] = useState(false);

  const [teachers, setTeachers] = useState<DiscoveredTeacherDto[]>([]);
  const [scanning, setScanning] = useState(true);
  const [scanError, setScanError] = useState<string | undefined>();
  const cancelled = useRef(false);

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

  const nameError = touched && name.trim().length === 0 ? "Введите имя, чтобы можно было подключиться" : undefined;
  const pinTrimmed = pin.trim();
  const pinValid = /^\d{4,6}$/.test(pinTrimmed);
  const pinError = touched && !pinValid ? "Введите PIN-код урока (4-6 цифр)" : undefined;
  const canConnect = name.trim().length > 0 && pinValid;

  function connect(teacher: DiscoveredTeacherDto) {
    setTouched(true);
    if (!canConnect) return;
    onConnect?.(name.trim(), teacher, pinTrimmed);
  }

  return (
    <Card>
      <motion.div initial={{ opacity: 0, x: 24 }} animate={{ opacity: 1, x: 0 }} transition={{ type: "spring", stiffness: 300, damping: 30 }}>
        <h2 className="mb-1 text-xl font-semibold">Подключение к уроку</h2>
        <p className="mb-6 text-sm text-[var(--color-text-muted)]">Введите имя и PIN-код от преподавателя.</p>

        <div className="flex flex-col gap-4">
          <TextField
            label="Ваше имя и фамилия"
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onBlur={() => setTouched(true)}
            error={nameError}
            placeholder="Иванов Пётр"
          />
          <TextField
            label="PIN-код урока"
            inputMode="numeric"
            value={pin}
            onChange={(e) => setPin(e.target.value.replace(/[^\d]/g, "").slice(0, 6))}
            onBlur={() => setTouched(true)}
            error={pinError}
            placeholder="123456"
            className="font-mono"
          />
        </div>

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

        {scanError && <p className="mt-2 rounded-lg bg-rose-400/10 px-3 py-2 text-sm text-rose-400">Ошибка поиска: {scanError}</p>}

        <div className="mt-3 min-h-[4rem]">
          <AnimatePresence mode="popLayout">
            {teachers.length === 0 && !scanError ? (
              <motion.p key="empty" exit={{ opacity: 0 }} className="text-sm text-[var(--color-text-muted)]">
                Пока никого не найдено. Убедитесь, что находитесь в одной сети с преподавателем.
              </motion.p>
            ) : (
              <motion.ul layout className="flex max-h-[30vh] flex-col gap-2 overflow-y-auto">
                {teachers.map((t) => (
                  <motion.li
                    key={`${t.ip}:${t.controlPort}`}
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
                    <Button variant="secondary" className="px-3 py-1.5 text-sm" disabled={!canConnect} onClick={() => connect(t)}>
                      Подключиться
                    </Button>
                  </motion.li>
                ))}
              </motion.ul>
            )}
          </AnimatePresence>
        </div>

        <div className="mt-6">
          <Button type="button" variant="ghost" onClick={onBack}>
            ← Назад
          </Button>
        </div>
      </motion.div>
    </Card>
  );
}
