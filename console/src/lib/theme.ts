import { useSyncExternalStore } from "react";

export type Theme = "dark" | "light";

const KEY = "gapless-theme";
const listeners = new Set<() => void>();

function read(): Theme {
  try {
    const saved = localStorage.getItem(KEY);
    if (saved === "dark" || saved === "light") return saved;
  } catch {
    // Storage can be unavailable (private mode); fall through to the default.
  }
  return "dark";
}

let current: Theme = read();

export function applyTheme(theme: Theme = current) {
  current = theme;
  document.documentElement.classList.toggle("dark", theme === "dark");
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // Not persisting is fine.
  }
  for (const listener of listeners) listener();
}

export function toggleTheme() {
  applyTheme(current === "dark" ? "light" : "dark");
}

export function useTheme(): Theme {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
  );
}
