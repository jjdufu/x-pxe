// 主题偏好：读写（storage.ts）、应用到 <html>（属性 + CSSOM，同参考实现 app.js applyThemeAndTokens）。
import { readJson, writeJson } from "../lib/storage";
import { ACCENT_PRESETS, GRAYS, type Gray, type Mode, type ModeChoice } from "./presets";
import { resolveAccent, type AccentChoice, type ResolvedAccent } from "./resolve-accent";

export const THEME_KEY = "xpxe.theme.v1";

export interface ThemePrefs {
  mode: ModeChoice;
  gray: Gray;
  radius: number;
  accent: AccentChoice;
}

export interface StoredTheme extends ThemePrefs {
  v: 1;
  /** 两种模式下解析后的 Accent（theme-boot.js 在 React 挂载前直接应用）。 */
  applied: Record<Mode, { accent: string; accentFg: string }>;
}

export const DEFAULT_THEME: ThemePrefs = {
  mode: "system",
  gray: "zinc",
  radius: 6,
  accent: { kind: "preset", name: "neutral" },
};

function isAccent(value: unknown): value is AccentChoice {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  if (record.kind === "preset") return ACCENT_PRESETS.includes(record.name as never);
  return record.kind === "custom" && typeof record.hex === "string" && /^#[0-9a-f]{6}$/iu.test(record.hex);
}

export function isStoredTheme(value: unknown): value is StoredTheme {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return (
    record.v === 1 &&
    (record.mode === "light" || record.mode === "dark" || record.mode === "system") &&
    GRAYS.includes(record.gray as Gray) &&
    typeof record.radius === "number" &&
    record.radius >= 0 &&
    record.radius <= 99 &&
    isAccent(record.accent)
  );
}

export function loadTheme(): ThemePrefs {
  const stored = readJson(THEME_KEY, isStoredTheme);
  if (stored === null) return DEFAULT_THEME;
  return { mode: stored.mode, gray: stored.gray, radius: stored.radius, accent: stored.accent };
}

export function saveTheme(prefs: ThemePrefs): void {
  const light = resolveAccent(prefs.accent, "light", prefs.gray);
  const dark = resolveAccent(prefs.accent, "dark", prefs.gray);
  const stored: StoredTheme = {
    v: 1,
    ...prefs,
    applied: {
      light: { accent: light.accent, accentFg: light.accentFg },
      dark: { accent: dark.accent, accentFg: dark.accentFg },
    },
  };
  writeJson(THEME_KEY, stored);
}

export function effectiveMode(choice: ModeChoice, systemDark: boolean): Mode {
  if (choice === "system") return systemDark ? "dark" : "light";
  return choice;
}

/** 应用到 <html>；返回解析后的 Accent（Editor 用于显示“已自动加深”）。 */
export function applyTheme(prefs: ThemePrefs, mode: Mode, root: HTMLElement = document.documentElement): ResolvedAccent {
  const resolved = resolveAccent(prefs.accent, mode, prefs.gray);
  root.dataset.theme = mode;
  root.dataset.gray = prefs.gray;
  root.style.setProperty("--radius-md", `${String(prefs.radius)}px`);
  root.style.setProperty("--accent", resolved.accent);
  root.style.setProperty("--accent-fg", resolved.accentFg);
  return resolved;
}

/** 导出：只导出 Tailwind v4 预设（规范 §4.3）。 */
export function exportTheme(prefs: ThemePrefs, resolved: ResolvedAccent): { json: string; css: string } {
  const accent = prefs.accent.kind === "preset" ? prefs.accent.name : prefs.accent.hex;
  const json = JSON.stringify({ engine: "tailwind-v4", mode: prefs.mode, gray: prefs.gray, radius: prefs.radius, accent }, null, 2);
  const css = [
    `:root[data-gray="${prefs.gray}"] {`,
    `  --radius-md: ${String(prefs.radius)}px;`,
    `  --accent: ${resolved.accent};`,
    `  --accent-fg: ${resolved.accentFg};`,
    "}",
  ].join("\n");
  return { json, css };
}
