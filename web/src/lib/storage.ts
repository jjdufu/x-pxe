// 唯一允许访问 localStorage 的文件（ESLint 强制）。所有读写都包 try/catch，
// 不可用（隐私模式、配额满、被禁用）时退化为内存，同参考实现 app.js loadSaved/savePref。
const memory = new Map<string, string>();

function backend(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** 读取字符串；存储不可用时读内存副本。 */
export function readItem(key: string): string | null {
  try {
    const value = backend()?.getItem(key) ?? null;
    return value ?? memory.get(key) ?? null;
  } catch {
    return memory.get(key) ?? null;
  }
}

/** 写入字符串；总是先写内存副本，持久化失败不抛出。 */
export function writeItem(key: string, value: string): void {
  memory.set(key, value);
  try {
    backend()?.setItem(key, value);
  } catch {
    // 退化为内存
  }
}

/** 删除。 */
export function removeItem(key: string): void {
  memory.delete(key);
  try {
    backend()?.removeItem(key);
  } catch {
    // 退化为内存
  }
}

/** 读取 JSON，并用 guard 校验形状；解析失败或形状不符返回 null。 */
export function readJson<T>(key: string, guard: (value: unknown) => value is T): T | null {
  const raw = readItem(key);
  if (raw === null) return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    return guard(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

/** 写入 JSON。 */
export function writeJson(key: string, value: unknown): void {
  writeItem(key, JSON.stringify(value));
}
