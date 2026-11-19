// 节点页共用：状态徽章、倒计时、规格表单字段。
import { useEffect, useState } from "react";
import { Badge, type Tone } from "../../../components/ui/feedback";
import { fmtCountdown, stateLabel } from "../../../lib/format";
import type { NodeRow } from "../../../lib/types";

export const STATE_TONE: Record<string, Tone> = {
  idle: "outline",
  armed: "neutral",
  installing: "warn",
  installed: "warn",
  configuring: "warn",
  ready: "success",
  failed: "danger",
};

export const STATES = ["idle", "armed", "installing", "installed", "configuring", "ready", "failed"] as const;
export const ACTIVE_STATES: readonly string[] = ["installing", "installed", "configuring"];

export function StateBadge({ state }: { state: string }) {
  return (
    <Badge tone={STATE_TONE[state] ?? "neutral"} dot>
      {stateLabel(state)}
    </Badge>
  );
}

/** 截止倒计时：每秒本地递减，不触发重取。 */
export function Countdown({ deadline }: { deadline: number | null }) {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    if (deadline === null) return undefined;
    const timer = window.setInterval(() => {
      setNow(Date.now() / 1000);
    }, 1000);
    return () => {
      window.clearInterval(timer);
    };
  }, [deadline]);
  if (deadline === null) return <span className="muted-text">—</span>;
  return (
    <span className="tabular" data-testid="countdown">
      {fmtCountdown(deadline - now)}
    </span>
  );
}

export function canArm(node: NodeRow): boolean {
  return node.state === "idle" || node.state === "failed";
}

export function canDisarm(node: NodeRow): boolean {
  return node.state === "armed";
}
