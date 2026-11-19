// 手动引导菜单文档的纯函数：默认值、排序、条目校验（与 xpxe-domain::manual 的白名单一致）。
import type { BootMenuDoc, BootMenuEntry, EntryKind, IsoDetection } from "../../../lib/types";

export const EMPTY_MENU: BootMenuDoc = { enabled: false, show_unknown: false, allow_macs: [], default_entry: null, timeout_s: 10, chain_prefixes: [], entries: [] };

const CMDLINE_RE = /^[A-Za-z0-9 ._,:=/+@%~-]*$/u;
const PATH_RE = /^\/[A-Za-z0-9._/+~-]*$/u;
const ID_RE = /^[a-z0-9_-]{1,32}$/u;
export const MAX_CMDLINE = 1024;

/** 把任意已保存文档补齐为完整的 BootMenuDoc。 */
export function normalize(doc: Record<string, unknown> | null): BootMenuDoc {
  return { ...EMPTY_MENU, ...(doc ?? {}) };
}

/** 把第 index 个条目移动 delta 位（越界时原样返回）。 */
export function move<T>(items: readonly T[], index: number, delta: number): T[] {
  const target = index + delta;
  if (index < 0 || index >= items.length || target < 0 || target >= items.length) return [...items];
  const next = [...items];
  const [item] = next.splice(index, 1);
  if (item !== undefined) next.splice(target, 0, item);
  return next;
}

export function cmdlineError(cmdline: string): string | undefined {
  if (cmdline.length > MAX_CMDLINE) return `不超过 ${String(MAX_CMDLINE)} 个字符`;
  return CMDLINE_RE.test(cmdline) ? undefined : "只允许字母、数字、空格与 . _ , : = / + @ % ~ -";
}

export function pathError(path: string): string | undefined {
  if (path === "") return "必填";
  return PATH_RE.test(path) && !path.split("/").includes("..") ? undefined : "以 / 开头，不含 ..，只允许字母、数字与 . _ / + ~ -";
}

/** 条目的字段错误（空对象表示可保存）。 */
export function entryErrors(entry: BootMenuEntry, others: readonly BootMenuEntry[]): Record<string, string> {
  const errors: Record<string, string> = {};
  if (!ID_RE.test(entry.id)) errors["id"] = "1–32 位小写字母、数字、_ 或 -";
  else if (others.some((item) => item.id === entry.id)) errors["id"] = "ID 已存在";
  if (entry.title.trim() === "" || entry.title.length > 64) errors["title"] = "1–64 个字符";
  const kind = entry.kind;
  if ((kind.type === "sanboot" || kind.type === "kernel_initrd") && kind.iso === "") errors["iso"] = "请选择 ISO";
  if (kind.type === "kernel_initrd") {
    const kernel = pathError(kind.kernel_path);
    const initrd = pathError(kind.initrd_path);
    const cmdline = cmdlineError(kind.cmdline);
    if (kernel !== undefined) errors["kernel_path"] = kernel;
    if (initrd !== undefined) errors["initrd_path"] = initrd;
    if (cmdline !== undefined) errors["cmdline"] = cmdline;
  }
  if (kind.type === "chain" && !kind.url.startsWith("/") && !/^https?:\/\//u.test(kind.url)) errors["url"] = "同源路径（/ 开头）或白名单前缀下的 http(s) URL";
  return errors;
}

/** 按类型切换条目定义，保留能沿用的 ISO。 */
export function switchKind(kind: EntryKind, type: EntryKind["type"]): EntryKind {
  const iso = kind.type === "sanboot" || kind.type === "kernel_initrd" ? kind.iso : "";
  switch (type) {
    case "sanboot":
      return { type, iso };
    case "kernel_initrd":
      return { type, iso, kernel_path: "", initrd_path: "", cmdline: "", embed_iso: false, iso_name: null };
    case "chain":
      return { type, url: "/" };
    case "exit":
      return { type };
  }
}

/** 选中 ISO 后按识别结果预填推荐类型与路径。 */
export function prefill(iso: string, detection: IsoDetection | undefined): EntryKind {
  if (detection?.recommended === "kernel_initrd" && detection.kernel_path && detection.initrd_path) {
    return { type: "kernel_initrd", iso, kernel_path: detection.kernel_path, initrd_path: detection.initrd_path, cmdline: "", embed_iso: detection.embed_iso === true, iso_name: detection.iso_name ?? null };
  }
  return { type: "sanboot", iso };
}

export function newEntry(existing: readonly BootMenuEntry[]): BootMenuEntry {
  let index = existing.length + 1;
  while (existing.some((item) => item.id === `entry-${String(index)}`)) index += 1;
  return { id: `entry-${String(index)}`, title: "新条目", enabled: true, arch: "any", kind: { type: "exit" } };
}

/** 删除或停用条目后修正默认项。 */
export function fixDefault(doc: BootMenuDoc): BootMenuDoc {
  const ok = doc.entries.some((entry) => entry.enabled && entry.id === doc.default_entry);
  return ok ? doc : { ...doc, default_entry: null };
}
