import { useEffect, useState } from "react";
import { Moon, Presentation, Sun } from "lucide-react";
import { Button } from "../components/ui/Button";
import { MicTest } from "../components/MicTest";
import { Panel } from "../components/ui/Panel";
import { listAudioDevices, type AudioDevicesDto } from "../lib/commands";
import { setTheme, useTheme, type Theme } from "../lib/theme";

type Quality = "high" | "medium" | "low";

const THEME_OPTIONS: { value: Theme; label: string; icon: typeof Sun }[] = [
  { value: "dark", label: "Тёмная", icon: Moon },
  { value: "light", label: "Светлая", icon: Sun },
];

const QUALITY_LABEL: Record<Quality, string> = {
  high: "Высокое (1280 px, 15 fps)",
  medium: "Среднее (1280 px, 10 fps)",
  low: "Низкое (960 px, 10 fps)",
};

const selectClasses =
  "w-full rounded-xl border border-[var(--color-border-subtle)] bg-field px-4 py-2.5 text-[var(--color-text-primary)] " +
  "outline-none transition-all focus:border-violet-400 focus:bg-field-focus focus:ring-4 focus:ring-violet-400/15";

/** Step 5 of the Tauri migration (vocalis_roadmap.md, section 8): settings
 * screen. The device lists are real — `list_audio_devices` (step 2) hitting
 * the same `cpal` enumeration as the egui apps' own Settings tab, not a mock.
 * Theme/video-quality/language are working local UI state with no
 * persistence yet, per the roadmap ("остальное можно как рабочий UI без
 * сохранения") — no `settings.json`-equivalent exists for this stack. */
export function SettingsPanel({ onShowOnboarding }: { onShowOnboarding: () => void }) {
  const [devices, setDevices] = useState<AudioDevicesDto | null>(null);
  const [devicesError, setDevicesError] = useState<string | undefined>();
  const [micDevice, setMicDevice] = useState("system");
  const [outputDevice, setOutputDevice] = useState("system");
  const [quality, setQuality] = useState<Quality>("high");
  const [language, setLanguage] = useState("ru");
  const theme = useTheme();

  useEffect(() => {
    listAudioDevices()
      .then(setDevices)
      .catch((err) => setDevicesError(String(err)));
  }, []);

  return (
    <div className="mx-auto flex h-full max-w-3xl flex-col gap-6 overflow-y-auto p-8">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Настройки</h1>
        <p className="text-sm text-[var(--color-text-muted)]">
          Список устройств — настоящий, с этого компьютера. Тема запоминается между запусками; остальные настройки пока нет.
        </p>
      </div>

      <Panel>
        <h2 className="mb-4 text-sm font-medium text-[var(--color-text-muted)]">Тема оформления</h2>
        <div role="radiogroup" aria-label="Тема оформления" className="inline-flex gap-1 rounded-xl bg-overlay p-1">
          {THEME_OPTIONS.map(({ value, label, icon: Icon }) => {
            const active = theme === value;
            return (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={active}
                onClick={() => setTheme(value)}
                className={
                  "inline-flex items-center gap-2 rounded-lg px-4 py-2 text-sm font-medium outline-none transition-colors " +
                  "focus-visible:ring-2 focus-visible:ring-violet-400 " +
                  (active
                    ? "bg-[var(--color-card-solid)] text-[var(--color-text-primary)] shadow-sm"
                    : "text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]")
                }
              >
                <Icon size={16} />
                {label}
              </button>
            );
          })}
        </div>
      </Panel>

      <Panel>
        <h2 className="mb-1 text-sm font-medium text-[var(--color-text-muted)]">Знакомство с Vocalis</h2>
        <p className="mb-4 text-sm text-[var(--color-text-muted)]">Короткий обзор из 5 шагов — тот же, что показывается при первом запуске.</p>
        <Button type="button" variant="secondary" className="px-3.5 py-2 text-sm" onClick={onShowOnboarding}>
          <Presentation size={16} />
          Показать введение ещё раз
        </Button>
      </Panel>

      <Panel>
        <h2 className="mb-4 text-sm font-medium text-[var(--color-text-muted)]">Аудиоустройства</h2>

        {devicesError && (
          <p className="rounded-lg bg-rose-400/10 px-3 py-2 text-sm text-danger-text">Не удалось получить список устройств: {devicesError}</p>
        )}
        {!devices && !devicesError && (
          <div className="flex items-center gap-2 text-sm text-[var(--color-text-muted)]">
            <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-violet-400 border-t-transparent" />
            Опрашиваю устройства…
          </div>
        )}

        {devices && (
          <div className="flex flex-col gap-4">
            <div>
              <label className="mb-1.5 block text-sm font-medium text-[var(--color-text-muted)]">Микрофон</label>
              <select value={micDevice} onChange={(e) => setMicDevice(e.target.value)} className={selectClasses}>
                <option value="system">Системное по умолчанию</option>
                {devices.inputDevices.map((d) => (
                  <option key={d} value={d}>
                    {d}
                  </option>
                ))}
              </select>
              <MicTest deviceName={micDevice === "system" ? undefined : micDevice} />
              <p className="mt-2 text-xs text-[var(--color-text-muted)]">
                Проверка идёт на выбранном микрофоне. В самом уроке пока всегда работает микрофон по умолчанию — выбор здесь ещё не подключён к трансляции.
              </p>
            </div>
            <div>
              <label className="mb-1.5 block text-sm font-medium text-[var(--color-text-muted)]">Устройство вывода</label>
              <select value={outputDevice} onChange={(e) => setOutputDevice(e.target.value)} className={selectClasses}>
                <option value="system">Системное по умолчанию</option>
                {devices.outputDevices.map((d) => (
                  <option key={d} value={d}>
                    {d}
                  </option>
                ))}
              </select>
            </div>
          </div>
        )}
      </Panel>

      <Panel>
        <h2 className="mb-4 text-sm font-medium text-[var(--color-text-muted)]">Качество видео-трансляции</h2>
        <div className="flex flex-col gap-2.5">
          {(Object.keys(QUALITY_LABEL) as Quality[]).map((q) => (
            <label key={q} className="flex cursor-pointer items-center gap-2.5 text-sm">
              <input type="radio" name="quality" checked={quality === q} onChange={() => setQuality(q)} className="accent-violet-400" />
              {QUALITY_LABEL[q]}
            </label>
          ))}
        </div>
      </Panel>

      <Panel>
        <h2 className="mb-4 text-sm font-medium text-[var(--color-text-muted)]">Язык интерфейса</h2>
        <select value={language} onChange={(e) => setLanguage(e.target.value)} className={`${selectClasses} max-w-xs`}>
          <option value="ru">Русский</option>
        </select>
      </Panel>
    </div>
  );
}
