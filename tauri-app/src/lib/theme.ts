import { useSyncExternalStore } from "react";

export type Theme = "dark" | "light";

const STORAGE_KEY = "vocalis-theme";
const listeners = new Set<() => void>();

/** The saved choice, or dark (the default look). `localStorage` can be missing or throw (private mode,
 * blocked storage), so failures just mean "not remembered", never a broken start. */
export function readStoredTheme(): Theme {
  try {
    return localStorage.getItem(STORAGE_KEY) === "light" ? "light" : "dark";
  } catch {
    return "dark";
  }
}

/** Switches the whole app: the palette lives in CSS variables keyed on `<html data-theme>` (see
 * `index.css`), so this one attribute is all a theme is. */
export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
  listeners.forEach((l) => l());
}

/** Apply and remember for the next launch. */
export function setTheme(theme: Theme) {
  applyTheme(theme);
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // not remembered — the switch itself still worked
  }
}

const currentTheme = (): Theme => (document.documentElement.dataset.theme === "light" ? "light" : "dark");
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

/** Current theme, re-rendering when it changes. */
export function useTheme(): Theme {
  return useSyncExternalStore(subscribe, currentTheme);
}
