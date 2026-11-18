// 主题上下文：偏好状态、系统明暗跟随、应用到 <html>、持久化，以及 Theme Editor 的开关。
import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useState, type ReactNode } from "react";
import type { Mode } from "./presets";
import { resolveAccent, type ResolvedAccent } from "./resolve-accent";
import { applyTheme, effectiveMode, loadTheme, saveTheme, type ThemePrefs } from "./state";

export interface ThemeContextValue {
  prefs: ThemePrefs;
  mode: Mode;
  resolved: ResolvedAccent;
  setPrefs: (next: ThemePrefs) => void;
  toggleMode: () => void;
  editorOpen: boolean;
  setEditorOpen: (open: boolean) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

const DARK_QUERY = "(prefers-color-scheme: dark)";

function systemPrefersDark(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(DARK_QUERY).matches;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefsState] = useState<ThemePrefs>(loadTheme);
  const [systemDark, setSystemDark] = useState(systemPrefersDark);
  const [editorOpen, setEditorOpen] = useState(false);
  const mode = effectiveMode(prefs.mode, systemDark);
  const resolved = useMemo(() => resolveAccent(prefs.accent, mode, prefs.gray), [prefs.accent, prefs.gray, mode]);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const media = window.matchMedia(DARK_QUERY);
    const onChange = (event: MediaQueryListEvent) => {
      setSystemDark(event.matches);
    };
    media.addEventListener("change", onChange);
    return () => {
      media.removeEventListener("change", onChange);
    };
  }, []);

  useLayoutEffect(() => {
    applyTheme(prefs, mode);
  }, [prefs, mode]);

  const value = useMemo<ThemeContextValue>(
    () => ({
      prefs,
      mode,
      resolved,
      setPrefs: (next) => {
        setPrefsState(next);
        saveTheme(next);
      },
      toggleMode: () => {
        const next = { ...prefs, mode: mode === "dark" ? ("light" as const) : ("dark" as const) };
        setPrefsState(next);
        saveTheme(next);
      },
      editorOpen,
      setEditorOpen,
    }),
    [prefs, mode, resolved, editorOpen],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const value = useContext(ThemeContext);
  if (value === null) throw new Error("useTheme 必须在 ThemeProvider 内使用");
  return value;
}
