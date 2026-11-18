// 参考实现：.progress-track/.progress-fill、.ui-badge(-success/-warn/-danger/-outline/-solid)/.badge-dot、
// .ui-alert(-info/-success/-warn/-danger)、.skeleton、.steps-bar/.step-item/.step-num、.timeline-list/.timeline-dot、.tracker-bar/.tracker-cell。
import { Progress as ArkProgress } from "@ark-ui/react/progress";
import { Check } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { cx } from "../../lib/cx";

export function Progress({ value, label }: { value: number | null; label: string }) {
  return (
    <ArkProgress.Root value={value} min={0} max={100} className="ui-progress">
      <div className="row-between">
        <ArkProgress.Label className="ui-label">{label}</ArkProgress.Label>
        <ArkProgress.ValueText className="muted-text tabular" />
      </div>
      <ArkProgress.Track className="progress-track">
        <ArkProgress.Range className="progress-fill" />
      </ArkProgress.Track>
    </ArkProgress.Root>
  );
}

export type Tone = "neutral" | "success" | "warn" | "danger" | "outline" | "solid";

export function Badge({ tone = "neutral", dot = false, children }: { tone?: Tone; dot?: boolean; children: ReactNode }) {
  return (
    <span className={cx("ui-badge", tone !== "neutral" && `ui-badge-${tone}`)}>
      {dot && <span className="badge-dot" aria-hidden="true" />}
      {children}
    </span>
  );
}

export function Alert({ tone = "info", title, children, action }: { tone?: "info" | "success" | "warn" | "danger"; title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className={cx("ui-alert", `ui-alert-${tone}`)} role={tone === "danger" ? "alert" : "status"}>
      <div className="ui-alert-title">{title}</div>
      {children !== undefined && <div className="ui-alert-desc">{children}</div>}
      {action !== undefined && <div className="ui-alert-action">{action}</div>}
    </div>
  );
}

export function Skeleton({ width, height = 14, className }: { width?: number | string; height?: number; className?: string }) {
  const style: CSSProperties = { height };
  if (width !== undefined) style.width = width;
  return <span className={cx("skeleton", className)} style={style} aria-hidden="true" />;
}

/** 步骤条（只读进度：资产激活向导、装机进度）。Ark Steps 面向带内容面板的向导（tablist），只读场景用有序列表更符合无障碍语义。 */
export function Steps({ labels, current, label = "进度" }: { labels: readonly string[]; current: number; label?: string }) {
  return (
    <ol className="steps-bar" aria-label={label}>
      {labels.map((text, index) => (
        <li key={text} className={cx("step-item", index < current && "done", index === current && "active")} aria-current={index === current ? "step" : undefined}>
          <span className="step-num" aria-hidden="true">
            {index < current ? <Check size={12} /> : index + 1}
          </span>
          <span>{text}</span>
          {index < current && <span className="sr-only">（已完成）</span>}
        </li>
      ))}
    </ol>
  );
}

export interface TimelineEntry {
  id: string;
  title: ReactNode;
  meta?: ReactNode;
  status?: "success" | "info" | "warn" | "danger";
}

export function Timeline({ entries }: { entries: readonly TimelineEntry[] }) {
  return (
    <ol className="timeline-list">
      {entries.map((entry) => (
        <li key={entry.id} className="timeline-item">
          <span className={cx("timeline-dot", entry.status)} aria-hidden="true" />
          <div>{entry.title}</div>
          {entry.meta !== undefined && <div className="muted-text tabular">{entry.meta}</div>}
        </li>
      ))}
    </ol>
  );
}

export interface TrackerDay {
  date: string;
  status: "ok" | "degraded" | "down";
  ratio: number;
}

export function Tracker({ days, label }: { days: readonly TrackerDay[]; label: string }) {
  return (
    <div className="tracker-bar" role="img" aria-label={label}>
      {days.map((day) => (
        <span key={day.date} className={cx("tracker-cell", day.status !== "ok" && day.status)} title={`${day.date} ${String(Math.round(day.ratio * 100))}%`} />
      ))}
    </div>
  );
}
