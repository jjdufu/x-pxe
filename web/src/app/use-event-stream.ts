// SSE → 查询缓存：推送只标记相关查询失效（后台重取，不重建表单）；断开时退化为 30 s 定时重取（step-12 第 4 节约定的基座部分）。
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { subscribeEvents, type Pushed, type StreamStatus } from "../lib/sse";

const INVALIDATES: Record<string, readonly (readonly string[])[]> = {
  "node.state": [["nodes"]],
  "node.event": [["nodes"]],
  "node.alert": [["nodes"]],
  "task.progress": [["nodes"], ["assets"]],
  "config.version": [["configs"], ["assets"]],
  "bootnet.health": [["bootnet"]],
  "bootnet.observations": [["bootnet"]],
};

export function useEventStream(): StreamStatus {
  const client = useQueryClient();
  const [status, setStatus] = useState<StreamStatus>("connecting");

  useEffect(() => {
    const onEvent = (event: Pushed) => {
      for (const key of INVALIDATES[event.kind] ?? []) void client.invalidateQueries({ queryKey: [...key] });
    };
    return subscribeEvents(onEvent, setStatus);
  }, [client]);

  useEffect(() => {
    if (status === "open") return undefined;
    const timer = window.setInterval(() => {
      void client.invalidateQueries();
    }, 30_000);
    return () => {
      window.clearInterval(timer);
    };
  }, [status, client]);

  return status;
}
