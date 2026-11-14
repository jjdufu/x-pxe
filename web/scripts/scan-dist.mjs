// 产物扫描骨架：dist 中不得出现外链与禁用 API（Step 11 补全全部规则）。
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const root = new URL("../dist/", import.meta.url).pathname;
const banned = [
  // React 生产包的错误码解码地址只是字符串、不会发起请求；SVG 命名空间同理
  [/https?:\/\/(?!www\.w3\.org\/|react\.dev\/errors\/)/u, "外链 URL"],
  [/<select[\s>]/u, "原生 <select>"],
  [/\balert\(/u, "alert()"],
  [/\bconfirm\(/u, "confirm()"],
  [/\bprompt\(/u, "prompt()"],
];
const files = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path);
    else files.push(path);
  }
};
walk(root);
let failed = false;
for (const file of files.filter((f) => /\.(html|js|css)$/u.test(f))) {
  const text = readFileSync(file, "utf8");
  for (const [pattern, label] of banned) {
    if (pattern.test(text)) {
      console.error(`scan-dist: ${file} 含有 ${label}`);
      failed = true;
    }
  }
}
if (failed) process.exit(1);
console.log(`scan-dist: ${String(files.length)} 个文件通过`);
