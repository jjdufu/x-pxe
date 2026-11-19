// 显示格式：时间（Unix 秒或 RFC 3339）、倒计时、大小、摘要前缀。
export function toDate(at: number | string | null | undefined): Date | null {
  if (at === null || at === undefined || at === "") return null;
  const date = typeof at === "number" ? new Date(at * 1000) : new Date(at);
  return Number.isNaN(date.getTime()) ? null : date;
}

const pad = (value: number) => String(value).padStart(2, "0");

/** 本地时间 `YYYY-MM-DD HH:MM:SS`。 */
export function fmtTime(at: number | string | null | undefined): string {
  const date = toDate(at);
  if (date === null) return "—";
  return `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

/** 剩余秒数 → `HH:MM:SS`（负数视为 0）。 */
export function fmtCountdown(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  return `${pad(hours)}:${pad(minutes)}:${pad(total % 60)}`;
}

export function fmtBytes(size: number): string {
  if (size < 1024) return `${String(size)} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KiB`;
  return `${(size / 1024 / 1024).toFixed(1)} MiB`;
}

export function shortSha(sha: string): string {
  return sha.slice(0, 12);
}

/** 节点状态显示名。 */
export const STATE_LABEL: Record<string, string> = {
  idle: "Idle",
  armed: "Armed",
  installing: "Installing",
  installed: "Installed",
  configuring: "Configuring",
  ready: "Ready",
  failed: "Failed",
};

export function stateLabel(state: string): string {
  return STATE_LABEL[state.toLowerCase()] ?? state;
}
