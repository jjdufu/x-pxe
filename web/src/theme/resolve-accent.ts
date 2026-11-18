// resolveAccent：按选择、模式与灰阶算出 --accent / --accent-fg，并保证对比度门槛
// （accent-fg/accent ≥ 4.5，accent/bg ≥ 3）。见 step-11 第 3.4 节。
import { contrast, fromOklch, saturation, toOklch } from "./color";
import { CHROMATIC, GRAY_TOKENS, type AccentPreset, type Gray, type Mode } from "./presets";

export type AccentChoice = { kind: "preset"; name: AccentPreset } | { kind: "custom"; hex: string };

export interface ResolvedAccent {
  accent: string;
  accentFg: string;
  adjusted: boolean;
}

export const FG_MIN = 4.5;
export const RING_MIN = 3;

const WHITE = "#ffffff";

function ok(accent: string, fg: string, bg: string): boolean {
  return contrast(fg, accent) >= FG_MIN && contrast(accent, bg) >= RING_MIN;
}

/** 在 OKLCH 中按方向逐步改亮度，直到满足门槛；找不到返回 null。 */
function walk(start: string, fg: string, bg: string, direction: -1 | 1): string | null {
  const base = toOklch(start);
  if (base === null) return null;
  for (let step = 0; step <= 100; step += 1) {
    const l = base.l + direction * step * 0.01;
    if (l < 0 || l > 1) break;
    const candidate = fromOklch({ l, c: base.c, h: base.h });
    if (ok(candidate, fg, bg)) return candidate;
  }
  return null;
}

function settle(accent: string, fg: string, bg: string, direction: -1 | 1): ResolvedAccent {
  if (ok(accent, fg, bg)) return { accent, accentFg: fg, adjusted: false };
  const walked = walk(accent, fg, bg, direction);
  if (walked !== null) return { accent: walked, accentFg: fg, adjusted: true };
  // 该前景无解（如深色模式下过暗的自定义色）：换用另一端前景（白 ↔ 背景色）并反向调整亮度
  const other = fg === WHITE ? bg : WHITE;
  const flipped = walk(accent, other, bg, direction === -1 ? 1 : -1);
  if (flipped !== null) return { accent: flipped, accentFg: other, adjusted: true };
  return { accent, accentFg: fg, adjusted: false };
}

export function resolveAccent(choice: AccentChoice, mode: Mode, gray: Gray): ResolvedAccent {
  const tokens = GRAY_TOKENS[gray][mode];
  if (choice.kind === "preset") {
    switch (choice.name) {
      case "neutral":
        return { accent: tokens.text, accentFg: mode === "dark" ? tokens.bg : WHITE, adjusted: false };
      case "slate":
        return mode === "dark"
          ? { accent: "#f8fafc", accentFg: "#020617", adjusted: false }
          : { accent: "#0f172a", accentFg: WHITE, adjusted: false };
      default:
        return settle(CHROMATIC[choice.name], WHITE, tokens.bg, -1);
    }
  }
  const hex = choice.hex.toLowerCase();
  if (saturation(hex) < 0.15) {
    // 无彩色：深色模式反转为浅色 Accent + 深色前景（规范 §4.2）
    if (mode === "dark") return settle(hex, tokens.bg, tokens.bg, 1);
    return settle(hex, WHITE, tokens.bg, -1);
  }
  return settle(hex, WHITE, tokens.bg, -1);
}
