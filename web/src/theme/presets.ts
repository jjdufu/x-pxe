// 灰阶与 Accent 预设（与 tokens.css 一致，单元测试逐值核对）。
export type Gray = "zinc" | "neutral" | "slate" | "sand";
export type Mode = "light" | "dark";
export type ModeChoice = Mode | "system";
export type AccentPreset = "neutral" | "slate" | "blue" | "emerald" | "violet" | "amber" | "rose";

export interface GrayTokens {
  bg: string;
  surface: string;
  surface2: string;
  border: string;
  borderInput: string;
  text: string;
  textMuted: string;
}

export const GRAYS: readonly Gray[] = ["zinc", "neutral", "slate", "sand"];
export const GRAY_LABELS: Record<Gray, string> = { zinc: "Zinc", neutral: "Neutral", slate: "Slate", sand: "Sand" };

export const GRAY_TOKENS: Record<Gray, Record<Mode, GrayTokens>> = {
  zinc: {
    light: { bg: "#fcfcfc", surface: "#f4f4f5", surface2: "#e4e4e7", border: "#e4e4e7", borderInput: "#8c8c8f", text: "#18181b", textMuted: "#63636b" },
    dark: { bg: "#09090b", surface: "#121215", surface2: "#18181b", border: "#27272a", borderInput: "#626264", text: "#fafafa", textMuted: "#a1a1aa" },
  },
  neutral: {
    light: { bg: "#fcfcfc", surface: "#f5f5f5", surface2: "#e5e5e5", border: "#e5e5e5", borderInput: "#8c8c8c", text: "#171717", textMuted: "#656565" },
    dark: { bg: "#0a0a0a", surface: "#141414", surface2: "#1f1f1f", border: "#262626", borderInput: "#636363", text: "#fafafa", textMuted: "#a3a3a3" },
  },
  slate: {
    light: { bg: "#fcfcfd", surface: "#f1f5f9", surface2: "#e2e8f0", border: "#e2e8f0", borderInput: "#868d99", text: "#0f172a", textMuted: "#59677e" },
    dark: { bg: "#020617", surface: "#0f172a", surface2: "#1e293b", border: "#1e293b", borderInput: "#5d6673", text: "#f8fafc", textMuted: "#94a3b8" },
  },
  sand: {
    light: { bg: "#fdfdfc", surface: "#f5f5f4", surface2: "#e7e5e4", border: "#e7e5e4", borderInput: "#8f8c8b", text: "#1c1917", textMuted: "#6b6460" },
    dark: { bg: "#0c0a09", surface: "#1c1917", surface2: "#292524", border: "#292524", borderInput: "#686665", text: "#fafaf9", textMuted: "#a8a29e" },
  },
};

export const ACCENT_PRESETS: readonly AccentPreset[] = ["neutral", "slate", "blue", "emerald", "violet", "amber", "rose"];
export const ACCENT_LABELS: Record<AccentPreset, string> = {
  neutral: "Neutral",
  slate: "Slate",
  blue: "Blue",
  emerald: "Emerald",
  violet: "Violet",
  amber: "Amber",
  rose: "Rose",
};

/** 彩色预设（两种模式同值，前景 #ffffff）。Emerald 取 #047857：参考实现的 #059669 与白色只有 3.77:1。 */
export const CHROMATIC: Record<Exclude<AccentPreset, "neutral" | "slate">, string> = {
  blue: "#2563eb",
  emerald: "#047857",
  violet: "#7c3aed",
  amber: "#b45309",
  rose: "#e11d48",
};

export const RADIUS_STEPS: readonly { label: string; value: number }[] = [
  { label: "None", value: 0 },
  { label: "Sm", value: 4 },
  { label: "Md", value: 6 },
  { label: "Base", value: 8 },
  { label: "Lg", value: 12 },
  { label: "Full", value: 99 },
];
