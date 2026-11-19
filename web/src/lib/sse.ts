// 服务端推送：原生 EventSource 订阅 /api/v1/events/stream。返回清理函数（调用方在 useEffect 中返回它）。
export const EVENT_KINDS = [
  "node.state",
  "node.event",
  "node.alert",
  "task.progress",
  "config.version",
  "bootnet.health",
  "bootnet.observations",
] as const;

export type EventKind = (typeof EVENT_KINDS)[number];

export interface Pushed {
  kind: string;
  data: unknown;
}

export type StreamStatus = "connecting" | "open" | "closed";

/**
 * 订阅推送。EventSource 只对网络错误自动重连；HTTP 错误（如强制改密期间的 403、登录前的 401）会永久关闭，
 * 这里在关闭后按 retryMs 重新建立连接，直到调用清理函数。
 */
export function subscribeEvents(
  onEvent: (event: Pushed) => void,
  onStatus: (status: StreamStatus) => void,
  url = "/api/v1/events/stream",
  retryMs = 5000,
): () => void {
  if (typeof EventSource === "undefined") {
    onStatus("closed");
    return () => undefined;
  }
  let source: EventSource | null = null;
  let timer: number | null = null;
  let stopped = false;
  const handle = (message: MessageEvent<string>) => {
    try {
      const parsed = JSON.parse(message.data) as Pushed;
      onEvent(parsed);
    } catch {
      // 忽略无法解析的推送
    }
  };
  const connect = () => {
    timer = null;
    if (stopped) return;
    const current = new EventSource(url, { withCredentials: true });
    source = current;
    onStatus("connecting");
    for (const kind of EVENT_KINDS) current.addEventListener(kind, handle);
    current.onopen = () => {
      onStatus("open");
    };
    current.onerror = () => {
      if (current.readyState !== EventSource.CLOSED) {
        onStatus("connecting");
        return;
      }
      onStatus("closed");
      for (const kind of EVENT_KINDS) current.removeEventListener(kind, handle);
      if (!stopped && timer === null) timer = window.setTimeout(connect, retryMs);
    };
  };
  connect();
  return () => {
    stopped = true;
    if (timer !== null) window.clearTimeout(timer);
    if (source !== null) {
      for (const kind of EVENT_KINDS) source.removeEventListener(kind, handle);
      source.close();
    }
  };
}
