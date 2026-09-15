import { useState, type FormEvent } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Panel } from "../components/ui/Panel";
import { Button } from "../components/ui/Button";
import { TextField } from "../components/ui/TextField";

type Kind = "test" | "listening" | "reading";

interface Template {
  id: number;
  kind: Kind;
  title: string;
  content: string;
}

// Same three colors as StudentCard/egui's assignment_kind_color (Test=WARN,
// Listening=ACCENT, Reading≈OK) — kept consistent across every screen that
// shows an assignment kind badge.
const KIND_META: Record<Kind, { label: string; color: string; fieldLabel: string }> = {
  test: { label: "Тест", color: "#e6a84a", fieldLabel: "Вопрос" },
  listening: { label: "Аудирование", color: "#a78bfa", fieldLabel: "Материал (название)" },
  reading: { label: "Чтение", color: "#5ec980", fieldLabel: "Текст для чтения" },
};

const INITIAL_TEMPLATES: Template[] = [
  { id: 1, kind: "test", title: "Времена группы Present", content: "She ___ to school every day. (goes)" },
  { id: 2, kind: "reading", title: "Текст: My family", content: "My family is not big..." },
];

/** Step 5 of the Tauri migration (vocalis_roadmap.md, section 8): assignments
 * screen — library + create form. Deliberately simplified per the roadmap's
 * own allowance: one question/field per template rather than the egui app's
 * full multi-question/multi-option editor (`teacher::app::test_editor`) —
 * that's flagged as follow-up work, not silently dropped. Everything here is
 * local state; there's no `send_assignment` command yet, so "Отправить"
 * only shows a transient confirmation rather than actually notifying anyone. */
export function AssignmentsPanel() {
  const [templates, setTemplates] = useState<Template[]>(INITIAL_TEMPLATES);
  const [justSentId, setJustSentId] = useState<number | null>(null);

  const [kind, setKind] = useState<Kind>("test");
  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");
  const [titleError, setTitleError] = useState<string | undefined>();

  function markSent(id: number) {
    setJustSentId(id);
    setTimeout(() => setJustSentId((cur) => (cur === id ? null : cur)), 1600);
  }

  function saveTemplate(e: FormEvent) {
    e.preventDefault();
    if (title.trim().length === 0) {
      setTitleError("Введите название задания");
      return;
    }
    setTemplates((prev) => [...prev, { id: Date.now(), kind, title: title.trim(), content: content.trim() }]);
    setTitle("");
    setContent("");
    setTitleError(undefined);
  }

  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col gap-6 overflow-y-auto p-8">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Задания</h1>
        <p className="text-sm text-[var(--color-text-muted)]">
          Отправка — выберите учеников в разделе «Класс», иначе задание уйдёт всему классу.
        </p>
      </div>

      <Panel>
        <h2 className="mb-4 text-sm font-medium text-[var(--color-text-muted)]">Библиотека заданий</h2>
        {templates.length === 0 && (
          <p className="text-sm text-[var(--color-text-muted)]">Пока нет ни одного созданного задания — соберите его ниже.</p>
        )}
        <ul className="flex flex-col gap-2">
          <AnimatePresence initial={false}>
            {templates.map((t) => {
              const meta = KIND_META[t.kind];
              return (
                <motion.li
                  key={t.id}
                  layout
                  initial={{ opacity: 0, y: 8 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, height: 0 }}
                  className="flex items-center justify-between gap-3 rounded-xl border border-[var(--color-border-subtle)] px-4 py-3"
                >
                  <div className="flex min-w-0 items-center gap-3">
                    <span
                      className="shrink-0 rounded-md px-2 py-0.5 text-xs font-medium"
                      style={{ backgroundColor: `${meta.color}26`, color: meta.color }}
                    >
                      {meta.label}
                    </span>
                    <span className="truncate">{t.title}</span>
                  </div>
                  <Button variant="secondary" className="shrink-0 px-3 py-1.5 text-sm" onClick={() => markSent(t.id)}>
                    {justSentId === t.id ? "Отправлено ✓" : "📤 Отправить"}
                  </Button>
                </motion.li>
              );
            })}
          </AnimatePresence>
        </ul>
      </Panel>

      <Panel>
        <h2 className="mb-4 text-sm font-medium text-[var(--color-text-muted)]">Создать задание</h2>

        <div className="mb-4 flex gap-2">
          {(Object.keys(KIND_META) as Kind[]).map((k) => (
            <button
              key={k}
              type="button"
              onClick={() => setKind(k)}
              className={
                "rounded-lg px-3 py-1.5 text-sm font-medium transition-colors " +
                (kind === k ? "bg-violet-400/20 text-violet-300" : "text-[var(--color-text-muted)] hover:bg-white/5")
              }
            >
              {KIND_META[k].label}
            </button>
          ))}
        </div>

        <form onSubmit={saveTemplate} className="flex flex-col gap-4">
          <TextField
            label="Название"
            value={title}
            onChange={(e) => {
              setTitle(e.target.value);
              if (titleError) setTitleError(undefined);
            }}
            error={titleError}
            placeholder="Например, Времена группы Present"
          />
          <div>
            <label className="mb-1.5 block text-sm font-medium text-[var(--color-text-muted)]">{KIND_META[kind].fieldLabel}</label>
            {kind === "reading" ? (
              <textarea
                value={content}
                onChange={(e) => setContent(e.target.value)}
                rows={4}
                className="w-full rounded-xl border border-[var(--color-border-subtle)] bg-black/20 px-4 py-2.5 text-[var(--color-text-primary)] outline-none transition-all focus:border-violet-400 focus:bg-black/30 focus:ring-4 focus:ring-violet-400/15"
              />
            ) : (
              <input
                value={content}
                onChange={(e) => setContent(e.target.value)}
                className="w-full rounded-xl border border-[var(--color-border-subtle)] bg-black/20 px-4 py-2.5 text-[var(--color-text-primary)] outline-none transition-all focus:border-violet-400 focus:bg-black/30 focus:ring-4 focus:ring-violet-400/15"
              />
            )}
          </div>
          <Button type="submit" className="self-start">
            💾 Сохранить задание
          </Button>
        </form>
      </Panel>
    </div>
  );
}
