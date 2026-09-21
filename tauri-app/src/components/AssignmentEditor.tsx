import { useState, type Dispatch, type FormEvent, type SetStateAction } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Check, Plus, Save, Trash2, X } from "lucide-react";
import { Button } from "./ui/Button";
import { TextField } from "./ui/TextField";
import { inputClasses, iconButtonClasses } from "./ui/fieldStyles";
import {
  KIND_META,
  MIN_OPTIONS,
  draftToContent,
  emptyDraft,
  isValid,
  newOption,
  newPrompt,
  newQuestion,
  removeOption,
  validateDraft,
  type AssignmentContent,
  type AssignmentDraft,
  type AssignmentKind,
  type DraftTestQuestion,
} from "../lib/assignments";

const rowMotion = {
  layout: true,
  initial: { opacity: 0, height: 0 },
  animate: { opacity: 1, height: "auto" },
  exit: { opacity: 0, height: 0 },
  transition: { duration: 0.18 },
} as const;

interface Props {
  /** The form's state lives in the parent (`TeacherConsole`) — this screen unmounts on every tab switch,
   * and a half-written five-question test must not vanish because the teacher peeked at the class. */
  draft: AssignmentDraft;
  setDraft: Dispatch<SetStateAction<AssignmentDraft>>;
  /** Called with a validated, protocol-shaped assignment. Local for now — wire a real save here later. */
  onSave: (title: string, content: AssignmentContent) => void;
}

/** Assignment editor: title + kind-specific body. Test: any number of questions, each with its own
 * options, a marked right answer and a delete button (the egui editor had no way to remove a question).
 * State is one `AssignmentDraft` (see `lib/assignments.ts`), so it saves as-is once there's a real command. */
export function AssignmentEditor({ draft, setDraft, onSave }: Props) {
  // Errors only appear after the first save attempt, then track the draft live as the teacher fixes things.
  const [attempted, setAttempted] = useState(false);
  const [justSaved, setJustSaved] = useState(false);
  const errors = attempted ? validateDraft(draft) : undefined;

  const patch = (p: Partial<AssignmentDraft>) => setDraft((d) => ({ ...d, ...p }));
  const patchQuestion = (id: number, f: (q: DraftTestQuestion) => DraftTestQuestion) =>
    setDraft((d) => ({ ...d, testQuestions: d.testQuestions.map((q) => (q.id === id ? f(q) : q)) }));

  function submit(e: FormEvent) {
    e.preventDefault();
    setAttempted(true);
    if (!isValid(validateDraft(draft))) return;
    onSave(draft.title.trim(), draftToContent(draft));
    setDraft(emptyDraft(draft.kind));
    setAttempted(false);
    setJustSaved(true);
    setTimeout(() => setJustSaved(false), 2200);
  }

  return (
    <>
      <div className="mb-4 flex flex-wrap gap-2">
        {(Object.keys(KIND_META) as AssignmentKind[]).map((k) => (
          <button
            key={k}
            type="button"
            onClick={() => patch({ kind: k })}
            className={
              "rounded-lg px-3 py-1.5 text-sm font-medium outline-none transition-colors focus-visible:ring-2 focus-visible:ring-violet-400 " +
              (draft.kind === k ? "bg-violet-400/20 text-accent-text" : "text-[var(--color-text-muted)] hover:bg-overlay")
            }
          >
            {KIND_META[k].label}
          </button>
        ))}
      </div>

      <form onSubmit={submit} className="flex flex-col gap-5" noValidate>
        <TextField
          label="Название"
          value={draft.title}
          onChange={(e) => patch({ title: e.target.value })}
          error={errors?.title}
          placeholder="Например, Времена группы Present"
        />

        {draft.kind === "test" && (
          <div className="flex flex-col gap-3">
            <AnimatePresence initial={false}>
              {draft.testQuestions.map((q, qi) => (
                <motion.div key={q.id} {...rowMotion} className="overflow-hidden">
                  <QuestionCard
                    index={qi}
                    question={q}
                    error={errors?.questions.get(q.id)}
                    onChange={(f) => patchQuestion(q.id, f)}
                    onDelete={() => setDraft((d) => ({ ...d, testQuestions: d.testQuestions.filter((x) => x.id !== q.id) }))}
                  />
                </motion.div>
              ))}
            </AnimatePresence>
            {draft.testQuestions.length === 0 && (
              <p className="rounded-xl border border-dashed border-[var(--color-border-subtle)] px-4 py-5 text-center text-sm text-[var(--color-text-muted)]">
                Вопросов пока нет.
              </p>
            )}
            {errors?.content && <p className="text-sm text-danger-text">{errors.content}</p>}
            <Button type="button" variant="secondary" className="self-start px-4 py-2 text-sm" onClick={() => patch({ testQuestions: [...draft.testQuestions, newQuestion()] })}>
              <Plus size={15} />
              Добавить вопрос
            </Button>
          </div>
        )}

        {draft.kind === "listening" && (
          <div className="flex flex-col gap-3">
            <TextField
              label="Материал (название из библиотеки)"
              value={draft.listeningMaterial}
              onChange={(e) => patch({ listeningMaterial: e.target.value })}
              error={errors?.content}
              placeholder="Например, Dialogue at the airport"
            />
            <div>
              <div className="mb-1.5 text-sm font-medium text-[var(--color-text-muted)]">
                Вопросы по материалу <span className="font-normal">(без автопроверки — ученик просто отмечает задание выполненным)</span>
              </div>
              <div className="flex flex-col gap-2">
                <AnimatePresence initial={false}>
                  {draft.listeningPrompts.map((p, pi) => (
                    <motion.div key={p.id} {...rowMotion} className="overflow-hidden">
                      <div className="flex items-center gap-2">
                        <span className="w-6 shrink-0 text-right text-sm tabular-nums text-[var(--color-text-muted)]">{pi + 1}.</span>
                        <input
                          value={p.text}
                          onChange={(e) => patch({ listeningPrompts: draft.listeningPrompts.map((x) => (x.id === p.id ? { ...x, text: e.target.value } : x)) })}
                          placeholder="Вопрос"
                          aria-label={`Вопрос ${pi + 1}`}
                          className={inputClasses}
                        />
                        <button
                          type="button"
                          className={iconButtonClasses}
                          title="Удалить вопрос"
                          aria-label={`Удалить вопрос ${pi + 1}`}
                          onClick={() => patch({ listeningPrompts: draft.listeningPrompts.filter((x) => x.id !== p.id) })}
                        >
                          <Trash2 size={16} />
                        </button>
                      </div>
                    </motion.div>
                  ))}
                </AnimatePresence>
              </div>
              <Button type="button" variant="secondary" className="mt-3 px-4 py-2 text-sm" onClick={() => patch({ listeningPrompts: [...draft.listeningPrompts, newPrompt()] })}>
                <Plus size={15} />
                Добавить вопрос
              </Button>
            </div>
          </div>
        )}

        {draft.kind === "reading" && (
          <div>
            <label htmlFor="reading-text" className="mb-1.5 block text-sm font-medium text-[var(--color-text-muted)]">
              Текст для чтения
            </label>
            <textarea
              id="reading-text"
              value={draft.readingText}
              onChange={(e) => patch({ readingText: e.target.value })}
              rows={6}
              className={inputClasses + (errors?.content ? " !border-rose-400/60" : "")}
            />
            {errors?.content && <p className="mt-1.5 text-sm text-danger-text">{errors.content}</p>}
          </div>
        )}

        <div className="flex items-center gap-3">
          <Button type="submit">
            <Save size={16} />
            Сохранить задание
          </Button>
          <AnimatePresence>
            {justSaved && (
              <motion.span initial={{ opacity: 0, x: -6 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0 }} className="text-sm text-ok-text" role="status">
                <span className="inline-flex items-center gap-1.5">
                  <Check size={14} />
                  Задание добавлено в библиотеку
                </span>
              </motion.span>
            )}
          </AnimatePresence>
        </div>
      </form>
    </>
  );
}

function QuestionCard({
  index,
  question,
  error,
  onChange,
  onDelete,
}: {
  index: number;
  question: DraftTestQuestion;
  error?: string;
  onChange: (f: (q: DraftTestQuestion) => DraftTestQuestion) => void;
  onDelete: () => void;
}) {
  const canRemoveOption = question.options.length > MIN_OPTIONS;
  return (
    <div className={"rounded-xl border p-4 " + (error ? "border-rose-400/50" : "border-[var(--color-border-subtle)]")}>
      <div className="mb-3 flex items-center gap-2">
        <span className="shrink-0 text-sm font-medium text-[var(--color-text-muted)]">Вопрос {index + 1}</span>
        <input
          value={question.text}
          onChange={(e) => onChange((q) => ({ ...q, text: e.target.value }))}
          placeholder="Текст вопроса"
          aria-label={`Текст вопроса ${index + 1}`}
          className={inputClasses}
        />
        <button type="button" className={iconButtonClasses} title="Удалить вопрос" aria-label={`Удалить вопрос ${index + 1}`} onClick={onDelete}>
          <Trash2 size={16} />
        </button>
      </div>

      <div className="flex flex-col gap-2">
        <AnimatePresence initial={false}>
          {question.options.map((o, oi) => (
            <motion.div key={o.id} {...rowMotion} className="overflow-hidden">
              <div className="flex items-center gap-2">
                <input
                  type="radio"
                  name={`correct-${question.id}`}
                  checked={question.correctOptionId === o.id}
                  onChange={() => onChange((q) => ({ ...q, correctOptionId: o.id }))}
                  title="Правильный вариант"
                  aria-label={`Вариант ${oi + 1} — правильный`}
                  className="h-4 w-4 shrink-0 accent-violet-400"
                />
                <input
                  value={o.text}
                  onChange={(e) => onChange((q) => ({ ...q, options: q.options.map((x) => (x.id === o.id ? { ...x, text: e.target.value } : x)) }))}
                  placeholder={`Вариант ${oi + 1}`}
                  aria-label={`Вариант ${oi + 1}`}
                  className={inputClasses + " py-2"}
                />
                <button
                  type="button"
                  className={iconButtonClasses}
                  disabled={!canRemoveOption}
                  title={canRemoveOption ? "Удалить вариант" : `Нужно минимум ${MIN_OPTIONS} варианта`}
                  aria-label={`Удалить вариант ${oi + 1}`}
                  onClick={() => onChange((q) => removeOption(q, o.id))}
                >
                  <X size={16} />
                </button>
              </div>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>

      <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
        <button
          type="button"
          onClick={() => onChange((q) => ({ ...q, options: [...q.options, newOption()] }))}
          className="inline-flex items-center gap-1 rounded-lg px-2.5 py-1 text-xs font-medium text-[var(--color-text-muted)] outline-none transition-colors hover:bg-overlay hover:text-[var(--color-text-primary)] focus-visible:ring-2 focus-visible:ring-violet-400"
        >
          <Plus size={13} />
          Вариант ответа
        </button>
        <span className="text-xs text-[var(--color-text-muted)]">Кружок слева — правильный ответ</span>
      </div>
      {error && <p className="mt-2 text-sm text-danger-text">{error}</p>}
    </div>
  );
}
