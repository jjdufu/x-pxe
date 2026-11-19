// 总览（step-12 §3.2）：状态计数、引导网络健康、资产就绪、近 30 天装机结果、最近事件。
import { useNavigate } from "react-router";
import { Alert, Badge, Timeline, Tracker, type TrackerDay } from "../../components/ui/feedback";
import { Card, CardLabel, Section } from "../../components/ui/layout";
import { fmtTime, shortSha, stateLabel } from "../../lib/format";
import type { AssetsView, BootnetStatus, Overview } from "../../lib/types";
import { AsyncView } from "../AsyncView";
import { useAssets, useBootnetStatus, useOverview } from "../data";

const ASSET_KINDS = [
  { kind: "ipxe_undionly", label: "iPXE BIOS（undionly.kpxe）" },
  { kind: "ipxe_efi_x64", label: "iPXE UEFI（ipxe.efi）" },
  { kind: "ipxe_snponly_x64", label: "iPXE UEFI（snponly.efi）" },
  { kind: "debian_netboot_amd64", label: "Debian netboot" },
];

const COUNTERS: { key: string; label: string; tone: "outline" | "neutral" | "warn" | "success" | "danger"; states: string[] }[] = [
  { key: "idle", label: "Idle", tone: "outline", states: ["idle"] },
  { key: "armed", label: "Armed", tone: "neutral", states: ["armed"] },
  { key: "active", label: "进行中", tone: "warn", states: ["installing", "installed", "configuring"] },
  { key: "ready", label: "Ready", tone: "success", states: ["ready"] },
  { key: "failed", label: "Failed", tone: "danger", states: ["failed"] },
];

/** 近 30 天：没有装机的日子记为正常。 */
export function trackerDays(outcomes: Overview["outcomes"], now: number): TrackerDay[] {
  const byDay = new Map(outcomes.map((item) => [item.day, item]));
  return Array.from({ length: 30 }, (_, index) => {
    const date = new Date((now - (29 - index) * 86_400) * 1000).toISOString().slice(0, 10);
    const day = byDay.get(date);
    const total = (day?.ready ?? 0) + (day?.failed ?? 0);
    const ratio = total === 0 ? 1 : (day?.ready ?? 0) / total;
    return { date, ratio, status: ratio === 1 ? "ok" : ratio === 0 ? "down" : "degraded" };
  });
}

function BootnetCard({ status }: { status: BootnetStatus | null | undefined }) {
  const beat = status?.heartbeat?.heartbeat;
  const conflicts = beat?.probe?.conflicts ?? [];
  return (
    <Card>
      <CardLabel>引导网络</CardLabel>
      {conflicts.length > 0 && (
        <Alert tone="danger" title="现网存在冲突的 DHCP 服务器">
          {conflicts.join("、")}，<a href="/boot-network">前往引导网络页处理</a>
        </Alert>
      )}
      <dl className="kv">
        <dt>健康</dt>
        <dd>
          <Badge tone={status === undefined || status === null ? "outline" : status.healthy ? "success" : "danger"} dot>
            {status === undefined || status === null ? "未上报" : status.healthy ? "健康" : "异常"}
          </Badge>
        </dd>
        <dt>模式</dt>
        <dd>{beat?.mode ?? "—"}</dd>
        <dt>指令版本</dt>
        <dd className="tabular">{status?.rev ?? "—"}</dd>
        <dt>最近心跳</dt>
        <dd className="tabular">{fmtTime(status?.heartbeat?.received_at)}</dd>
      </dl>
    </Card>
  );
}

function AssetsCard({ view }: { view: AssetsView | null | undefined }) {
  return (
    <Card>
      <CardLabel>资产就绪</CardLabel>
      <ul className="list-plain">
        {ASSET_KINDS.map(({ kind, label }) => {
          const active = view?.channels.channels[kind];
          const record = active === undefined ? undefined : view?.assets.find((item) => item.id === active.asset_id);
          return (
            <li key={kind} className="row-between">
              <span>{label}</span>
              <span className="row-gap">
                {record?.kernel_abi !== undefined && record.kernel_abi !== null && <span className="mono muted-text">{record.kernel_abi}</span>}
                {active === undefined ? <Badge tone="outline">未激活</Badge> : <Badge tone="success">激活 {shortSha(active.sha256)}</Badge>}
              </span>
            </li>
          );
        })}
      </ul>
    </Card>
  );
}

function OverviewBody({ data }: { data: Overview }) {
  const navigate = useNavigate();
  const bootnet = useBootnetStatus();
  const assets = useAssets();
  const count = (states: string[]) =>
    states.reduce((sum, state) => sum + (data.states.by_state[state as keyof Overview["states"]["by_state"]] ?? 0), 0);
  return (
    <div className="page-stack">
      <div className="grid-4">
        {COUNTERS.map((counter) => (
          <button
            key={counter.key}
            type="button"
            className="ui-card stat-card"
            onClick={() => {
              void navigate(`/nodes?state=${counter.key}`);
            }}
          >
            <Badge tone={counter.tone} dot>
              {counter.label}
            </Badge>
            <span className="stat-value tabular">{count(counter.states)}</span>
          </button>
        ))}
      </div>
      <div className="grid-2">
        <BootnetCard status={bootnet.data} />
        <AssetsCard view={assets.data} />
      </div>
      <Card>
        <CardLabel>近 30 天装机结果</CardLabel>
        <Tracker days={trackerDays(data.outcomes, data.now)} label="近 30 天装机结果" />
      </Card>
      <Card>
        <CardLabel>最近事件</CardLabel>
        {data.events.length === 0 ? (
          <p className="muted-text">暂无事件</p>
        ) : (
          <Timeline
            entries={data.events.map((event) => ({
              id: String(event.id),
              title: (
                <span>
                  <strong>{event.hostname}</strong> {event.kind}
                  {event.to_state !== null && event.from_state !== event.to_state && ` → ${stateLabel(event.to_state)}`}
                </span>
              ),
              meta: fmtTime(event.at),
              status: event.kind.includes("fail") ? "danger" : event.to_state === "ready" ? "success" : "info",
            }))}
          />
        )}
      </Card>
    </div>
  );
}

export function OverviewPage() {
  const query = useOverview();
  return (
    <Section title="总览" description="节点状态、引导网络与资产就绪情况。">
      <AsyncView query={query} isEmpty={(data) => data === null} empty={{ title: "暂无概况", description: "控制面尚未返回运行概况。" }}>
        {(data) => (data === null ? null : <OverviewBody data={data} />)}
      </AsyncView>
    </Section>
  );
}
