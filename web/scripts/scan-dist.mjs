// 产物扫描（step-11 第 10 节第 5 项）：dist 中不得出现外链与禁用 API。
// 允许的 URL 字面量只有：XML/SVG/MathML 命名空间（w3.org），以及 React / React Router 生产包里
// 错误提示文案中的文档地址、React Router 解析相对路径用的基址 http://localhost（只是字符串，不会发起请求；Playwright 离线用例另行断言外部请求数为 0）。
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const root = new URL(`../${process.argv[2] ?? "dist"}/`, import.meta.url).pathname;
const ALLOWED_URL = /^(https?:\/\/(www\.w3\.org\/|react\.dev\/errors\/|reactrouter\.com\/)|http:\/\/localhost$)/u;

export const BANNED = [
  [/<select[\s>]/u, "原生 <select>"],
  [/\balert\(/u, "alert()"],
  [/\bconfirm\(/u, "confirm()"],
  [/\bprompt\(/u, "prompt()"],
  [/@import\s+url\(/u, "@import url("],
  [/@font-face\s*\{[^}]*url\(\s*['"]?(https?:)?\/\//u, "@font-face 外链"],
  [/--park-/u, "--park- 前缀"],
  [/--colors-/u, "--colors- 前缀"],
];

/** 返回文本中的违规项（标签列表）。 */
export function violations(text, kind) {
  const found = [];
  for (const match of text.matchAll(/https?:\/\/[^\s"'`)<>]*/gu)) {
    if (!ALLOWED_URL.test(match[0])) found.push(`外链 URL ${match[0].slice(0, 80)}`);
  }
  for (const [pattern, label] of BANNED) if (pattern.test(text)) found.push(label);
  if (kind === "css" && !/\[hidden\]\{display:none!important\}/u.test(text.replace(/\s+/gu, ""))) {
    // 只要求入口样式表包含；调用方按需判断
    found.push("@@no-hidden-rule");
  }
  return found;
}

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else out.push(path);
  }
  return out;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const files = walk(root).filter((file) => /\.(html|js|css)$/u.test(file));
  let failed = false;
  let hiddenRule = false;
  for (const file of files) {
    const kind = file.endsWith(".css") ? "css" : "other";
    const found = violations(readFileSync(file, "utf8"), kind);
    if (kind === "css" && !found.includes("@@no-hidden-rule")) hiddenRule = true;
    for (const label of found.filter((item) => item !== "@@no-hidden-rule")) {
      console.error(`scan-dist: ${file} 含有 ${label}`);
      failed = true;
    }
  }
  if (!hiddenRule) {
    console.error("scan-dist: 样式表中缺少 [hidden]{display:none!important}");
    failed = true;
  }
  if (failed) process.exit(1);
  console.log(`scan-dist: ${String(files.length)} 个文件通过`);
}
