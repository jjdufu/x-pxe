// 对比度门禁（step-11 第 3.5 节、第 10 节第 7 项）：直接解析 tokens.css 与 presets.ts，
// 对每个灰阶 × 模式的全部组合做数值断言，任一低于门槛即失败，并打印实测表。
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../src/styles/tokens.css", import.meta.url), "utf8");
const presets = readFileSync(new URL("../src/theme/presets.ts", import.meta.url), "utf8");

const GRAYS = ["zinc", "neutral", "slate", "sand"];
const MODES = ["light", "dark"];

function parseBlocks(text) {
  const blocks = [];
  for (const match of text.matchAll(/([^{}]+)\{([^}]*)\}/gu)) {
    const selectors = match[1].split(",").map((item) => item.replace(/\/\*[\s\S]*?\*\//gu, "").trim());
    const vars = Object.fromEntries([...match[2].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/gu)].map((m) => [m[1], m[2].trim()]));
    blocks.push({ selectors, vars });
  }
  return blocks;
}

const blocks = parseBlocks(css);
const selectorFor = (gray, mode) => (mode === "light" ? `:root[data-gray="${gray}"]` : `:root[data-gray="${gray}"][data-theme="dark"]`);

function tokensFor(gray, mode) {
  const out = {};
  const apply = (selector) => {
    for (const block of blocks) if (block.selectors.includes(selector)) Object.assign(out, block.vars);
  };
  apply(":root");
  if (mode === "dark") apply(':root[data-theme="dark"]');
  apply(selectorFor(gray, "light"));
  if (mode === "dark") apply(selectorFor(gray, "dark"));
  return out;
}

function rgb(hex) {
  const value = /^#([0-9a-f]{6})$/iu.exec(hex);
  if (value === null) throw new Error(`无法解析颜色 ${hex}`);
  const n = Number.parseInt(value[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}
const lin = (c) => (c / 255 <= 0.04045 ? c / 255 / 12.92 : ((c / 255 + 0.055) / 1.055) ** 2.4);
const lum = (hex) => {
  const [r, g, b] = rgb(hex).map(lin);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
export const contrast = (a, b) => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

const chromatic = Object.fromEntries([...presets.matchAll(/^\s+(blue|emerald|violet|amber|rose): "(#[0-9a-f]{6})",$/gmu)].map((m) => [m[1], m[2]]));
if (Object.keys(chromatic).length !== 5) throw new Error("presets.ts 中未找到 5 个彩色 Accent");

let failed = 0;
const check = (label, value, min) => {
  if (value < min) {
    console.error(`contrast: ${label} = ${value.toFixed(2)} < ${String(min)}`);
    failed += 1;
  }
  return value.toFixed(2);
};

const rows = [];
for (const gray of GRAYS) {
  for (const mode of MODES) {
    const t = tokensFor(gray, mode);
    const id = `${gray}/${mode}`;
    const bgs = ["--bg", "--surface", "--surface-2"];
    const muted = bgs.map((bg) => check(`${id} --text-muted/${bg}`, contrast(t["--text-muted"], t[bg]), 4.5));
    for (const bg of bgs) check(`${id} --text/${bg}`, contrast(t["--text"], t[bg]), 4.5);
    const input = ["--bg", "--surface"].map((bg) => check(`${id} --border-input/${bg}`, contrast(t["--border-input"], t[bg]), 3));
    for (const tone of ["--danger", "--success", "--warning", "--info"]) {
      for (const bg of bgs) check(`${id} ${tone}/${bg}`, contrast(t[tone], t[bg]), 4.5);
    }
    check(`${id} --danger-fg/--danger`, contrast(t["--danger-fg"], t["--danger"]), 4.5);
    // Accent：Neutral 跟随灰阶，Slate 固定，彩色两种模式同值且前景白色
    const accents = {
      neutral: mode === "dark" ? [t["--text"], t["--bg"]] : [t["--text"], "#ffffff"],
      slate: mode === "dark" ? ["#f8fafc", "#020617"] : ["#0f172a", "#ffffff"],
      ...Object.fromEntries(Object.entries(chromatic).map(([name, hex]) => [name, [hex, "#ffffff"]])),
    };
    for (const [name, [accent, fg]] of Object.entries(accents)) {
      check(`${id} accent-fg/accent(${name})`, contrast(fg, accent), 4.5);
      check(`${id} accent(${name})/--bg`, contrast(accent, t["--bg"]), 3);
    }
    rows.push(`| ${gray} | ${mode} | ${muted.join(" / ")} | ${input.join(" / ")} |`);
  }
}

console.log("| 灰阶 | 模式 | --text-muted 对 bg / surface / surface-2 | --border-input 对 bg / surface |");
console.log(rows.join("\n"));
if (failed > 0) {
  console.error(`contrast: ${String(failed)} 项低于门槛`);
  process.exit(1);
}
console.log("contrast: 全部组合达标");
