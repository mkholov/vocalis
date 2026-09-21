import { useEffect, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { ArrowLeft, ArrowRight, Cast, ChartColumn, Check, ClipboardList, KeyRound, LayoutGrid, Mic, Presentation, Settings, type LucideIcon } from "lucide-react";
import { Button } from "./ui/Button";

interface Step {
  icon: LucideIcon;
  title: string;
  body: ReactNode;
}

const SECTIONS: { icon: LucideIcon; name: string; text: string }[] = [
  { icon: LayoutGrid, name: "Класс", text: "сетка учеников, таймер урока, трансляция голоса и экрана" },
  { icon: ClipboardList, name: "Задания", text: "библиотека и редактор тестов, аудирования и чтения" },
  { icon: ChartColumn, name: "Статистика", text: "сводка по урокам и ученикам (пока в виде образца)" },
  { icon: Settings, name: "Настройки", text: "тема оформления, проверка микрофона и это введение" },
];

const STEPS: Step[] = [
  {
    icon: Presentation,
    title: "Добро пожаловать в Vocalis",
    body: "Vocalis — лингафонный кабинет: вы ведёте урок с одного экрана, слышите и говорите с учениками, показываете свой экран и аудио, видите, кто из класса активен. Короткий обзор из 5 шагов покажет, с чего начать.",
  },
  {
    icon: KeyRound,
    title: "Начните урок и позовите учеников",
    body: "После выбора класса откроется консоль урока, и в её шапке сразу виден PIN — его можно скопировать одним кликом. Ученики открывают Vocalis, выбирают вас в списке преподавателей, вводят своё имя и этот PIN — и появляются в сетке класса.",
  },
  {
    icon: LayoutGrid,
    title: "Четыре раздела",
    body: (
      <ul className="flex flex-col gap-2.5">
        {SECTIONS.map(({ icon: Icon, name, text }) => (
          <li key={name} className="flex items-start gap-3">
            <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-violet-400/15 text-accent-text">
              <Icon size={15} />
            </span>
            <span>
              <span className="font-medium text-[var(--color-text-primary)]">{name}</span> — {text}
            </span>
          </li>
        ))}
      </ul>
    ),
  },
  {
    icon: Mic,
    title: "Голос и звук",
    body: "«Говорить с классом» — ваш микрофон слышит весь класс. На карточке ученика «Слушать» включает его микрофон только вам, а «Интерком» — приватный разговор с ним, не мешая остальным. В «Материалах» — аудиофайлы, которые можно проиграть всему классу или выбранным ученикам.",
  },
  {
    icon: Cast,
    title: "Экран и группы",
    body: "«Показать классу» транслирует ваш экран на компьютеры всех учеников, а «Превью экрана» показывает, как это выглядит у вас. Кнопка «Группы» собирает учеников в пары и группы для работы друг с другом. Это введение всегда можно открыть снова в «Настройках».",
  },
];

/** First-run walkthrough as a modal over the teacher console — five short cards, skippable at any point,
 * reopenable from Settings. Keyboard: ← → to move, Esc to skip, Tab stays inside the card. */
export function Onboarding({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [step, setStep] = useState(0);
  const [direction, setDirection] = useState(1);
  const dialogRef = useRef<HTMLDivElement>(null);
  const last = step === STEPS.length - 1;

  // Reopening starts from the first card again.
  useEffect(() => {
    if (open) {
      setStep(0);
      setDirection(1);
    }
  }, [open]);

  function go(delta: number) {
    const next = step + delta;
    if (next < 0 || next >= STEPS.length) return;
    setDirection(delta);
    setStep(next);
  }

  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
      else if (e.key === "ArrowRight") go(1);
      else if (e.key === "ArrowLeft") go(-1);
      else if (e.key === "Tab" && dialogRef.current) {
        const focusable = Array.from(dialogRef.current.querySelectorAll<HTMLElement>("button:not([disabled])"));
        if (focusable.length === 0) return;
        const first = focusable[0];
        const lastEl = focusable[focusable.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          lastEl.focus();
        } else if (!e.shiftKey && document.activeElement === lastEl) {
          e.preventDefault();
          first.focus();
        }
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const current = STEPS[step];
  const Icon = current.icon;

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          key="onboarding"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="fixed inset-0 z-[55] flex items-center justify-center bg-black/55 p-6 backdrop-blur-sm"
        >
          <motion.div
            ref={dialogRef}
            role="dialog"
            aria-modal="true"
            aria-label="Знакомство с Vocalis"
            initial={{ opacity: 0, y: 16, scale: 0.97 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 8, scale: 0.98 }}
            transition={{ type: "spring", stiffness: 300, damping: 30 }}
            className="w-full max-w-lg overflow-hidden rounded-2xl border border-[var(--color-border-subtle)] bg-[var(--color-card-solid)] p-8 shadow-2xl shadow-black/40"
          >
            <div className="relative min-h-[17rem]">
              <AnimatePresence mode="wait" initial={false} custom={direction}>
                <motion.div
                  key={step}
                  custom={direction}
                  variants={{
                    enter: (d: number) => ({ opacity: 0, x: 32 * d }),
                    center: { opacity: 1, x: 0 },
                    exit: (d: number) => ({ opacity: 0, x: -32 * d }),
                  }}
                  initial="enter"
                  animate="center"
                  exit="exit"
                  transition={{ duration: 0.18 }}
                >
                  <div className="mb-5 flex h-12 w-12 items-center justify-center rounded-2xl bg-violet-400/15 text-accent-text">
                    <Icon size={24} />
                  </div>
                  <h2 className="mb-3 text-xl font-semibold tracking-tight">{current.title}</h2>
                  <div className="text-sm leading-relaxed text-[var(--color-text-muted)]">{current.body}</div>
                </motion.div>
              </AnimatePresence>
            </div>

            <div className="mt-6 flex items-center justify-center gap-2" aria-label={`Шаг ${step + 1} из ${STEPS.length}`}>
              {STEPS.map((_, i) => (
                <motion.span
                  key={i}
                  animate={{ width: i === step ? 22 : 8, opacity: i === step ? 1 : 0.35 }}
                  transition={{ type: "spring", stiffness: 400, damping: 30 }}
                  className="h-2 rounded-full bg-violet-400"
                />
              ))}
            </div>

            <div className="mt-6 flex items-center gap-3">
              <Button type="button" variant="ghost" className={step === 0 ? "invisible" : ""} onClick={() => go(-1)} tabIndex={step === 0 ? -1 : 0}>
                <ArrowLeft size={16} />
                Назад
              </Button>
              <span className="flex-1" />
              {!last && (
                <Button type="button" variant="ghost" onClick={onClose}>
                  Пропустить
                </Button>
              )}
              <Button key={step} type="button" autoFocus onClick={() => (last ? onClose() : go(1))}>
                {last ? (
                  <>
                    <Check size={16} />
                    Готово
                  </>
                ) : (
                  <>
                    Далее
                    <ArrowRight size={16} />
                  </>
                )}
              </Button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
