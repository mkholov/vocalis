import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Check, Pencil, Trash2, X } from "lucide-react";
import { Button } from "./ui/Button";
import { inputClasses, iconButtonClasses } from "./ui/fieldStyles";
import { plural } from "../lib/plural";
import type { ClassDto } from "../lib/commands";

interface Props {
  cls: ClassDto;
  selected: boolean;
  onSelect: () => void;
  /** Resolves when saved; rejects with a message to show under the field. */
  onRename: (name: string) => Promise<void>;
  /** Resolves when deleted; rejects with a message to show in the confirmation. */
  onDelete: () => Promise<void>;
}

type Mode = "view" | "rename" | "delete";

const neutralIconButton =
  "inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-[var(--color-text-muted)] outline-none " +
  "transition-colors hover:bg-overlay-hover hover:text-[var(--color-text-primary)] focus-visible:ring-2 focus-visible:ring-violet-400";

/** One class in the picker: select it, rename it in place, or delete it. Deleting is two-step — the row
 * turns into a confirmation that says what goes with the class (its lesson history and roster) and only
 * "Удалить" there deletes; the same idea as the egui console's "click, then confirm" for roster names. */
export function ClassRow({ cls, selected, onSelect, onRename, onDelete }: Props) {
  const [mode, setMode] = useState<Mode>("view");
  const [draft, setDraft] = useState(cls.name);
  const [error, setError] = useState<string | undefined>();
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (mode === "rename") inputRef.current?.select();
  }, [mode]);

  function enter(next: Mode) {
    setMode(next);
    setError(undefined);
    setDraft(cls.name);
  }

  async function run(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    try {
      await action();
      setMode("view");
      setError(undefined);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  const history =
    cls.lessons === 0 && cls.roster === 0
      ? "У класса нет истории — удаляется только он сам."
      : `Вместе с ним удалится: ${[
          cls.lessons > 0 ? `история — ${plural(cls.lessons, "урок", "урока", "уроков")}` : "",
          cls.roster > 0 ? `список — ${plural(cls.roster, "ученик", "ученика", "учеников")}` : "",
        ]
          .filter(Boolean)
          .join(", ")}. Это нельзя отменить.`;

  const shell =
    "rounded-xl border transition-colors " +
    (mode === "delete"
      ? "border-rose-400/50 bg-rose-400/5"
      : selected
        ? "border-violet-400 bg-violet-400/10"
        : "border-[var(--color-border-subtle)] hover:bg-overlay");

  return (
    <div className={shell} onKeyDown={(e) => e.key === "Escape" && mode !== "view" && enter("view")}>
      <AnimatePresence mode="wait" initial={false}>
        {mode === "view" && (
          <motion.div key="view" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.12 }} className="flex items-center gap-1 pr-1.5">
            <button
              type="button"
              onClick={onSelect}
              className={
                "min-w-0 flex-1 rounded-xl px-4 py-3 text-left outline-none focus-visible:ring-2 focus-visible:ring-violet-400 " +
                (selected ? "text-[var(--color-text-primary)]" : "text-[var(--color-text-muted)]")
              }
            >
              <div className="truncate">{cls.name}</div>
              {(cls.lessons > 0 || cls.roster > 0) && (
                <div className="truncate text-xs text-[var(--color-text-muted)]">
                  {[
                    cls.lessons > 0 ? plural(cls.lessons, "урок", "урока", "уроков") : "",
                    cls.roster > 0 ? plural(cls.roster, "ученик", "ученика", "учеников") : "",
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </div>
              )}
            </button>
            <button type="button" className={neutralIconButton} title="Переименовать" aria-label={`Переименовать класс ${cls.name}`} onClick={() => enter("rename")}>
              <Pencil size={15} />
            </button>
            <button type="button" className={iconButtonClasses} title="Удалить" aria-label={`Удалить класс ${cls.name}`} onClick={() => enter("delete")}>
              <Trash2 size={15} />
            </button>
          </motion.div>
        )}

        {mode === "rename" && (
          <motion.form
            key="rename"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.12 }}
            className="p-2"
            onSubmit={(e) => {
              e.preventDefault();
              run(() => onRename(draft));
            }}
          >
            <div className="flex items-center gap-1.5">
              <input
                ref={inputRef}
                autoFocus
                value={draft}
                onChange={(e) => {
                  setDraft(e.target.value);
                  if (error) setError(undefined);
                }}
                aria-label="Новое название класса"
                className={inputClasses + " py-2"}
              />
              <button type="submit" disabled={busy} className={neutralIconButton + " text-ok-text"} title="Сохранить" aria-label="Сохранить название">
                <Check size={16} />
              </button>
              <button type="button" className={neutralIconButton} title="Отмена" aria-label="Отменить переименование" onClick={() => enter("view")}>
                <X size={16} />
              </button>
            </div>
            {error && <p className="mt-1.5 px-1 text-sm text-danger-text">{error}</p>}
          </motion.form>
        )}

        {mode === "delete" && (
          <motion.div key="delete" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.12 }} className="p-3" role="alertdialog" aria-label={`Удалить класс ${cls.name}?`}>
            <p className="text-sm font-medium">Удалить класс «{cls.name}»?</p>
            <p className="mt-0.5 text-sm text-[var(--color-text-muted)]">{history}</p>
            {error && <p className="mt-1.5 text-sm text-danger-text">{error}</p>}
            <div className="mt-3 flex gap-2">
              <Button
                type="button"
                variant="secondary"
                className="flex-1 !border-rose-400/60 !bg-rose-400/15 py-2 text-sm !text-danger-text"
                disabled={busy}
                onClick={() => run(onDelete)}
              >
                <Trash2 size={15} />
                Удалить
              </Button>
              <Button type="button" variant="ghost" className="py-2 text-sm" autoFocus onClick={() => enter("view")}>
                Отмена
              </Button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
