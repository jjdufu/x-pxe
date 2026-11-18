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

export function subscribeEvents(
  onEvent: (event: Pushed) => void,
  onStatus: (status: StreamStatus) => void,
  url = "/api/v1/events/stream",
): () => void {
  if (typeof EventSource === "undefined") {
    onStatus("closed");
    return () => undefined;
  }
  const source = new EventSource(url, { withCredentials: true });
  onStatus("connecting");
  const handle = (message: MessageEvent<string>) => {
    try {
      const parsed = JSON.parse(message.data) as Pushed;
      onEvent(parsed);
    } catch {
      // 忽略无法解析的推送
    }
  };
  for (const kind of EVENT_KINDS) source.addEventListener(kind, handle);
  source.onopen = () => {
    onStatus("open");
  };
  source.onerror = () => {
    onStatus(source.readyState === EventSource.CLOSED ? "closed" : "connecting");
  };
  return () => {
    for (const kind of EVENT_KINDS) source.removeEventListener(kind, handle);
    source.close();
  };
}
