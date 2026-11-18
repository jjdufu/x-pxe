import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { contrast } from "./color";
import { ACCENT_PRESETS, GRAYS, GRAY_TOKENS, type Mode } from "./presets";
import { FG_MIN, RING_MIN, resolveAccent } from "./resolve-accent";

const MODES: Mode[] = ["light", "dark"];
const tokensCss = readFileSync(`${process.cwd()}/src/styles/tokens.css`, "utf8");

describe("resolveAccent 全矩阵", () => {
  for (const gray of GRAYS) {
    for (const mode of MODES) {
      for (const name of ACCENT_PRESETS) {
        it(`${gray}/${mode}/${name}：主按钮 ≥ 4.5、焦点环 ≥ 3`, () => {
          const bg = GRAY_TOKENS[gray][mode].bg;
          const resolved = resolveAccent({ kind: "preset", name }, mode, gray);
          expect(contrast(resolved.accentFg, resolved.accent)).toBeGreaterThanOrEqual(FG_MIN);
          expect(contrast(resolved.accent, bg)).toBeGreaterThanOrEqual(RING_MIN);
        });
      }
    }
  }

  it("深色模式下中性 Accent 反转为浅色 Accent + 深色前景（规范 §4.2）", () => {
    expect(resolveAccent({ kind: "preset", name: "neutral" }, "dark", "zinc")).toEqual({ accent: "#fafafa", accentFg: "#09090b", adjusted: false });
    expect(resolveAccent({ kind: "preset", name: "neutral" }, "light", "zinc")).toEqual({ accent: "#18181b", accentFg: "#ffffff", adjusted: false });
    expect(resolveAccent({ kind: "preset", name: "slate" }, "dark", "zinc")).toEqual({ accent: "#f8fafc", accentFg: "#020617", adjusted: false });
  });

  it("彩色预设前景固定白色且无需调整（Emerald 用 #047857）", () => {
    for (const mode of MODES) {
      const emerald = resolveAccent({ kind: "preset", name: "emerald" }, mode, "zinc");
      expect(emerald).toEqual({ accent: "#047857", accentFg: "#ffffff", adjusted: false });
    }
    expect(contrast("#ffffff", "#059669")).toBeLessThan(FG_MIN);
  });

  it("对比度不足的自定义彩色被加深并标记 adjusted", () => {
    const resolved = resolveAccent({ kind: "custom", hex: "#22d3ee" }, "light", "zinc");
    expect(resolved.adjusted).toBe(true);
    expect(resolved.accentFg).toBe("#ffffff");
    expect(contrast(resolved.accentFg, resolved.accent)).toBeGreaterThanOrEqual(FG_MIN);
  });

  it("深色模式下过暗的自定义色改用深色前景并提亮", () => {
    for (const hex of ["#1e1b4b", "#18181b", "#0f172a", "#3f0d12"]) {
      const resolved = resolveAccent({ kind: "custom", hex }, "dark", "zinc");
      expect(contrast(resolved.accentFg, resolved.accent)).toBeGreaterThanOrEqual(FG_MIN);
      expect(contrast(resolved.accent, GRAY_TOKENS.zinc.dark.bg)).toBeGreaterThanOrEqual(RING_MIN);
    }
  });

  it("任意自定义色在所有灰阶与模式下都满足门槛", () => {
    const samples = ["#ff0000", "#00ff00", "#0000ff", "#ffff00", "#ff00ff", "#00ffff", "#ffffff", "#000000", "#808080", "#f59e0b", "#7c3aed"];
    for (const hex of samples) {
      for (const gray of GRAYS) {
        for (const mode of MODES) {
          const resolved = resolveAccent({ kind: "custom", hex }, mode, gray);
          expect(contrast(resolved.accentFg, resolved.accent), `${hex} ${gray} ${mode}`).toBeGreaterThanOrEqual(FG_MIN);
          expect(contrast(resolved.accent, GRAY_TOKENS[gray][mode].bg), `${hex} ${gray} ${mode}`).toBeGreaterThanOrEqual(RING_MIN);
        }
      }
    }
  });
});

describe("presets.ts 与 tokens.css 一致", () => {
  const block = (gray: string, mode: Mode): string => {
    const selector = mode === "light" ? `:root[data-gray="${gray}"]` : `:root[data-gray="${gray}"][data-theme="dark"]`;
    const start = tokensCss.indexOf(`${selector} {`);
    const fallback = tokensCss.indexOf(`${selector},`) >= 0 ? tokensCss.indexOf(`${selector},`) : tokensCss.indexOf(`, ${selector} {`);
    const at = start >= 0 ? start : fallback;
    expect(at, selector).toBeGreaterThanOrEqual(0);
    return tokensCss.slice(at, tokensCss.indexOf("}", at));
  };
  for (const gray of GRAYS) {
    for (const mode of MODES) {
      it(`${gray}/${mode}`, () => {
        const text = block(gray, mode);
        const tokens = GRAY_TOKENS[gray][mode];
        expect(text).toContain(`--bg: ${tokens.bg};`);
        expect(text).toContain(`--surface: ${tokens.surface};`);
        expect(text).toContain(`--surface-2: ${tokens.surface2};`);
        expect(text).toContain(`--border-input: ${tokens.borderInput};`);
        expect(text).toContain(`--text: ${tokens.text};`);
        expect(text).toContain(`--text-muted: ${tokens.textMuted};`);
      });
    }
  }
});
