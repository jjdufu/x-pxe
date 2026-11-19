// 节点详情抽屉（step-12 §3.3）：概览 / 事件 / 日志 / 规格 / BMC / 会话。SSE 使时间轴查询失效后自动增长。
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "../../../components/ui/button";
import { CodeBlock, CopyButton } from "../../../components/ui/code";
import { AlertDialog, Drawer } from "../../../components/ui/dialog";
import { Alert, Badge, Steps, Timeline } from "../../../components/ui/feedback";
import { Tabs } from "../../../components/ui/tabs";
import { api } from "../../../lib/api";
import { fmtTime, stateLabel } from "../../../lib/format";
import type { EventDetail, NodeRow, SessionDetail } from "../../../lib/types";
import { useAction, useTimeline } from "../../data";
import { Countdown, StateBadge } from "./common";
import { draftOf, fieldErrors, SpecFields, specOf, type SpecDraft } from "./NodeDialogs";

const FLOW = ["armed", "installing", "installed", "configuring", "ready"];
const FLOW_LABELS = FLOW.map(stateLabel);

function OverviewTab({ node }: { node: NodeRow }) {
  const index = FLOW.indexOf(node.state);
  return (
    <div className="page-stack">
      <Steps labels={FLOW_LABELS} current={node.state === "ready" ? FLOW.length : Math.max(index, 0)} label="装机进度" />
      {node.alert !== null && (
        <Alert tone="warn" title="会话告警">
          {node.alert}
        </Alert>
      )}
      <dl className="kv">
        <dt>状态</dt>
        <dd>
          <StateBadge state={node.state} />
        </dd>
        <dt>状态起始</dt>
        <dd className="tabular">{fmtTime(node.state_since)}</dd>
        <dt>截止</dt>
        <dd>
          <Countdown deadline={node.deadline_at} />
        </dd>
        <dt>MAC</dt>
        <dd className="mono tabular">{node.mac}</dd>
        <dt>通道 / 固件</dt>
        <dd>
          {node.channel.type === "redfish" ? "Redfish" : "LAN"} / {node.firmware.toUpperCase()}
        </dd>
        <dt>会话</dt>
        <dd className="mono">{node.active_session_id ?? "—"}</dd>
      </dl>
    </div>
  );
}

function eventTitle(event: EventDetail) {
  const change = event.to_state !== null && event.from_state !== event.to_state ? ` → ${stateLabel(event.to_state)}` : "";
  return `${event.kind}${change}`;
}

function EventsTab({ events }: { events: EventDetail[] }) {
  if (events.length === 0) return <p className="muted-text">暂无事件</p>;
  return (
    <div data-testid="node-timeline">
      <Timeline
        entries={events.map((event) => ({
          id: String(event.id),
          title: eventTitle(event),
          meta: `${fmtTime(event.at)} · ${event.source}`,
          status: event.kind.includes("fail") ? "danger" : event.to_state === "ready" ? "success" : "info",
        }))}
      />
    </div>
  );
}

function LogTab({ events }: { events: EventDetail[] }) {
  const lines = events
    .filter((event) => event.kind.endsWith("failed") || (typeof event.payload === "object" && event.payload !== null && "log_tail" in event.payload))
    .map((event) => {
      const payload = event.payload as { log_tail?: unknown };
      const body = typeof payload.log_tail === "string" ? payload.log_tail : JSON.stringify(event.payload, null, 2);
      return `# ${event.kind} ${fmtTime(event.at)}\n${body}`;
    });
  if (lines.length === 0) return <p className="muted-text">没有失败上报或 agent 日志</p>;
  return <CodeBlock label="装机日志" code={lines.join("\n\n")} />;
}

function SpecTab({ node }: { node: NodeRow }) {
  const [draft, setDraft] = useState<SpecDraft>(() => draftOf(node));
  const [errors, setErrors] = useState<Record<string, string>>({});
  const spec = useQuery({ queryKey: ["nodes", node.id, "spec"], queryFn: () => api<{ version: { version: number } }>(`/nodes/${String(node.id)}/spec`) });
  const { perform, busy } = useAction();
  const editable = ["idle", "ready", "failed"].includes(node.state);
  const save = async () => {
    let failure: unknown = null;
    const saved = await perform({
      success: "规格已保存为新版本",
      failure: "保存失败",
      invalidate: [["nodes"], ["history"]],
      run: () =>
        api(`/nodes/${String(node.id)}`, {
          method: "PUT",
          body: { expected_head: spec.data?.version.version ?? 1, spec: specOf(draft), profile_id: Number(draft.profileId), init_plan_id: Number(draft.planId) },
        }).catch((error: unknown) => {
          failure = error;
          throw error;
        }),
    });
    setErrors(saved === undefined ? fieldErrors(failure) : {});
  };
  return (
    <div className="page-stack">
      {!editable && (
        <Alert tone="info" title="装机进行中">
          装机结束后才能修改规格。
        </Alert>
      )}
      <SpecFields draft={draft} onChange={setDraft} locked errors={errors} />
      <div className="row-gap">
        <span className="muted-text tabular">规格版本 v{spec.data?.version.version ?? "—"}</span>
        <Button
          variant="primary"
          disabled={!editable || busy}
          onClick={() => {
            void save();
          }}
        >
          保存规格
        </Button>
      </div>
    </div>
  );
}

interface BmcItem {
  id: number;
  name: string;
  document: { endpoint: string; username: string; pin?: string | null; fingerprint?: string | null };
}

interface ProbeResult {
  fingerprint: string;
  subject: string;
  issuer: string;
  not_before: string;
  not_after: string;
  vendor: string | null;
}

function BmcTab({ node }: { node: NodeRow }) {
  const bmcs = useQuery({ queryKey: ["bmcs"], queryFn: () => api<BmcItem[]>("/bmcs") });
  const [probe, setProbe] = useState<ProbeResult | null>(null);
  const [trustOpen, setTrustOpen] = useState(false);
  const { perform, busy } = useAction();
  const bmc = (bmcs.data ?? []).find((item) => item.name === node.channel.bmc);
  if (node.channel.type !== "redfish") return <p className="muted-text">LAN 通道节点没有 BMC。把通道改为 Redfish 并登记 BMC 后可在此探测与固定证书。</p>;
  if (bmc === undefined) return <p className="muted-text">未找到名为 {node.channel.bmc ?? "—"} 的 BMC 登记。</p>;
  return (
    <div className="page-stack">
      <dl className="kv">
        <dt>地址</dt>
        <dd className="mono">{bmc.document.endpoint}</dd>
        <dt>用户名</dt>
        <dd>{bmc.document.username}</dd>
        <dt>密码</dt>
        <dd>已设置（只写不读）</dd>
      </dl>
      <div className="row-gap">
        <Button
          variant="outline"
          disabled={busy}
          onClick={() => {
            void perform({ success: "探测完成", failure: "探测失败", run: () => api<ProbeResult>(`/bmcs/${String(bmc.id)}/probe`, { method: "POST" }) }).then((result) => {
              if (result !== undefined) setProbe(result);
            });
          }}
        >
          探测证书
        </Button>
        <Button
          variant="primary"
          disabled={probe === null}
          onClick={() => {
            setTrustOpen(true);
          }}
        >
          固定指纹
        </Button>
      </div>
      {probe !== null && (
        <dl className="kv">
          <dt>指纹</dt>
          <dd className="mono">{probe.fingerprint}</dd>
          <dt>主题</dt>
          <dd>{probe.subject}</dd>
          <dt>有效期</dt>
          <dd className="tabular">
            {probe.not_before} — {probe.not_after}
          </dd>
        </dl>
      )}
      <AlertDialog
        open={trustOpen}
        onOpenChange={setTrustOpen}
        title="固定 BMC 证书指纹"
        description="请与 BMC 管理界面显示的指纹逐位核对后再确认。"
        confirmLabel="固定"
        onConfirm={() => {
          void perform({
            success: "已固定指纹",
            failure: "固定失败",
            invalidate: [["bmcs"]],
            run: () => api(`/bmcs/${String(bmc.id)}/trust`, { method: "POST", body: { fingerprint: probe?.fingerprint ?? "" } }),
          }).then(() => {
            setTrustOpen(false);
          });
        }}
      >
        <p className="mono">{probe?.fingerprint}</p>
      </AlertDialog>
    </div>
  );
}

function SessionTab({ node, sessionId }: { node: NodeRow; sessionId: string | null }) {
  const detail = useQuery({
    queryKey: ["nodes", node.id, "session", sessionId],
    queryFn: () => api<SessionDetail>(`/nodes/${String(node.id)}/sessions/${sessionId ?? ""}`),
    enabled: sessionId !== null,
  });
  if (sessionId === null) return <p className="muted-text">节点还没有会话</p>;
  const data = detail.data;
  if (data === undefined) return <p className="muted-text">加载中…</p>;
  return (
    <div className="page-stack">
      <dl className="kv">
        <dt>会话</dt>
        <dd className="mono">
          {data.id} <CopyButton value={data.id} />
        </dd>
        <dt>结果</dt>
        <dd>{data.outcome ?? (data.closed_at === null ? "进行中" : "—")}</dd>
        <dt>过期时间</dt>
        <dd className="tabular">{fmtTime(data.expires_at)}</dd>
        <dt>令牌</dt>
        <dd className="mono">{data.token_tail === null ? "已销毁" : `••••${data.token_tail}`}</dd>
      </dl>
      <CodeBlock label="固定的版本" code={JSON.stringify(data.pinned, null, 2)} />
    </div>
  );
}

export function NodeDrawer({ node, onClose }: { node: NodeRow | undefined; onClose: () => void }) {
  const [tab, setTab] = useState("overview");
  const timeline = useTimeline(node?.id ?? null);
  const events = timeline.data ?? [];
  const sessionId = node?.active_session_id ?? events.find((event) => event.session_id !== null)?.session_id ?? null;
  return (
    <Drawer
      open={node !== undefined}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title="节点详情"
      description={node === undefined ? undefined : `${node.hostname} · ${node.mac}`}
    >
      {node !== undefined && (
        <Tabs
          label="节点详情"
          value={tab}
          onChange={setTab}
          items={[
            { value: "overview", label: "概览", content: <OverviewTab node={node} /> },
            {
              value: "events",
              label: "事件",
              content: (
                <div className="page-stack">
                  <Badge tone="outline">共 {events.length} 条</Badge>
                  <EventsTab events={events} />
                </div>
              ),
            },
            { value: "log", label: "日志", content: <LogTab events={events} /> },
            { value: "spec", label: "规格", content: <SpecTab key={node.id} node={node} /> },
            { value: "bmc", label: "BMC", content: <BmcTab node={node} /> },
            { value: "session", label: "会话", content: <SessionTab node={node} sessionId={sessionId} /> },
          ]}
        />
      )}
    </Drawer>
  );
}
