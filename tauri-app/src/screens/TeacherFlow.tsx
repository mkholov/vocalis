import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { ArrowLeft, Plus } from "lucide-react";
import { Card } from "../components/ui/Card";
import { Button } from "../components/ui/Button";
import { TextField } from "../components/ui/TextField";
import { createClass, deleteClass, listClasses, renameClass, type ClassDto } from "../lib/commands";
import { ClassRow } from "../components/ClassRow";

interface Props {
  onBack: () => void;
  /** Called after the mocked "start lesson" step, with the chosen class's
   * name — lets the parent move on to the step-4 class grid. Real
   * network/session start is still a later roadmap step, so this is only
   * ever a local screen transition, never a network call. */
  onStart?: (className: string) => void;
}

type Step = "password" | "classes";

/** Teacher flow: password → class picker, matching the egui console's own
 * order (`teacher::auth::AuthScreen` then `teacher::class_picker`) — same
 * two gates, new look. Real login/session-start is still a later roadmap
 * step; the `console.log` below is a deliberate stand-in, not a stub for
 * something that was supposed to work already. */
export function TeacherFlow({ onBack, onStart }: Props) {
  const [step, setStep] = useState<Step>("password");
  const [password, setPassword] = useState("");
  const [passwordError, setPasswordError] = useState<string | undefined>();

  const [classes, setClasses] = useState<ClassDto[] | null>(null);
  const [classesError, setClassesError] = useState<string | undefined>();
  const [selectedClassId, setSelectedClassId] = useState<number | null>(null);

  const [newClassName, setNewClassName] = useState("");
  const [newClassError, setNewClassError] = useState<string | undefined>();
  const [creatingClass, setCreatingClass] = useState(false);

  useEffect(() => {
    if (step !== "classes" || classes !== null) return;
    listClasses()
      .then((rows) => {
        setClasses(rows);
        if (rows.length > 0) setSelectedClassId(rows[0].id);
      })
      .catch((err) => setClassesError(String(err)));
  }, [step, classes]);

  function submitPassword(e: React.FormEvent) {
    e.preventDefault();
    if (password.trim().length === 0) {
      setPasswordError("Введите пароль");
      return;
    }
    setPasswordError(undefined);
    setStep("classes");
  }

  /** Real `create_class` (the same `db::insert_class` the egui class picker uses); the new class is
   * appended to the list and selected right away, no reload of the list needed. */
  function submitNewClass(e: React.FormEvent) {
    e.preventDefault();
    if (creatingClass) return;
    if (newClassName.trim().length === 0) {
      setNewClassError("Введите название класса");
      return;
    }
    setCreatingClass(true);
    createClass(newClassName)
      .then((created) => {
        setClasses((prev) => [...(prev ?? []), created]);
        setSelectedClassId(created.id);
        setNewClassName("");
        setNewClassError(undefined);
      })
      .catch((err) => setNewClassError(String(err)))
      .finally(() => setCreatingClass(false));
  }

  /** Rejections carry a ready-to-show message; the row shows it, so it is passed on as a plain string. */
  async function handleRename(id: number, name: string) {
    try {
      const updated = await renameClass(id, name);
      setClasses((prev) => prev?.map((c) => (c.id === id ? updated : c)) ?? prev);
    } catch (err) {
      throw String(err);
    }
  }

  async function handleDelete(id: number) {
    try {
      await deleteClass(id);
    } catch (err) {
      throw String(err);
    }
    const remaining = (classes ?? []).filter((c) => c.id !== id);
    setClasses(remaining);
    // Deleting the chosen class must not leave "Начать урок" pointing at nothing.
    if (selectedClassId === id) setSelectedClassId(remaining[0]?.id ?? null);
  }

  function startLesson() {
    const chosen = classes?.find((c) => c.id === selectedClassId);
    // Real network/session start is a later roadmap step — this is still just
    // the visual shell, so a console.log stands in for "start the lesson".
    console.log("[mock] начать урок для класса:", chosen);
    if (chosen) onStart?.(chosen.name);
  }

  return (
    <Card>
      <AnimatePresence mode="wait">
        {step === "password" ? (
          <motion.form
            key="password"
            initial={{ opacity: 0, x: 24 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -24 }}
            transition={{ type: "spring", stiffness: 300, damping: 30 }}
            onSubmit={submitPassword}
          >
            <h2 className="mb-1 text-xl font-semibold">Профиль преподавателя</h2>
            <p className="mb-6 text-sm text-[var(--color-text-muted)]">Введите пароль, чтобы войти в консоль.</p>

            <TextField
              label="Пароль"
              type="password"
              autoFocus
              value={password}
              onChange={(e) => {
                setPassword(e.target.value);
                if (passwordError) setPasswordError(undefined);
              }}
              error={passwordError}
              placeholder="••••••••"
            />

            <div className="mt-6 flex gap-3">
              <Button type="button" variant="ghost" onClick={onBack}>
                <ArrowLeft size={16} />
                Назад
              </Button>
              <Button type="submit" className="flex-1">
                Войти
              </Button>
            </div>
          </motion.form>
        ) : (
          <motion.div
            key="classes"
            initial={{ opacity: 0, x: 24 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -24 }}
            transition={{ type: "spring", stiffness: 300, damping: 30 }}
          >
            <h2 className="mb-1 text-xl font-semibold">Выберите класс</h2>
            <p className="mb-6 text-sm text-[var(--color-text-muted)]">Для какого класса этот урок?</p>

            {classesError && (
              <p className="mb-4 rounded-lg bg-rose-400/10 px-3 py-2 text-sm text-danger-text">
                Не удалось загрузить классы: {classesError}
              </p>
            )}

            {classes === null && !classesError && (
              <div className="flex items-center gap-2 py-4 text-sm text-[var(--color-text-muted)]">
                <motion.span
                  className="h-3.5 w-3.5 rounded-full border-2 border-violet-400 border-t-transparent"
                  animate={{ rotate: 360 }}
                  transition={{ repeat: Infinity, duration: 0.7, ease: "linear" }}
                />
                Загрузка классов из базы данных…
              </div>
            )}

            {classes?.length === 0 && (
              <p className="py-4 text-sm text-[var(--color-text-muted)]">
                Классов пока нет — создайте первый ниже.
              </p>
            )}

            {classes && classes.length > 0 && (
              <motion.ul
                className="mb-2 flex max-h-[38vh] flex-col gap-2 overflow-y-auto"
                initial="hidden"
                animate="visible"
                variants={{ visible: { transition: { staggerChildren: 0.05 } } }}
              >
                {classes.map((c) => (
                  <motion.li key={c.id} variants={{ hidden: { opacity: 0, y: 8 }, visible: { opacity: 1, y: 0 } }}>
                    <ClassRow
                      cls={c}
                      selected={selectedClassId === c.id}
                      onSelect={() => setSelectedClassId(c.id)}
                      onRename={(name) => handleRename(c.id, name)}
                      onDelete={() => handleDelete(c.id)}
                    />
                  </motion.li>
                ))}
              </motion.ul>
            )}

            {classes && (
              <form className="mt-4 flex items-start gap-2" onSubmit={submitNewClass}>
                <div className="flex-1">
                  <TextField
                    label="Новый класс"
                    value={newClassName}
                    onChange={(e) => {
                      setNewClassName(e.target.value);
                      if (newClassError) setNewClassError(undefined);
                    }}
                    error={newClassError}
                    placeholder="например, 9А английский"
                  />
                </div>
                <Button type="submit" variant="secondary" className="mt-[1.625rem] py-2.5" disabled={creatingClass}>
                  <Plus size={16} />
                  Создать
                </Button>
              </form>
            )}

            <div className="mt-6 flex gap-3">
              <Button type="button" variant="ghost" onClick={() => setStep("password")}>
                <ArrowLeft size={16} />
                Назад
              </Button>
              <Button
                type="button"
                className="flex-1"
                disabled={!classes || classes.length === 0 || selectedClassId === null}
                onClick={startLesson}
              >
                Начать урок
              </Button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </Card>
  );
}
