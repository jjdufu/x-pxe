// 引导网络（step-12 §3.6）：模式 RadioCards、网卡、DHCP 规则、守护状态、现网扫描与冲突确认、外部 DHCP 片段、观测。
import { useState } from "react";
import { useNavigate } from "react-router";
import { Button } from "../../../components/ui/button";
import { CodeBlock } from "../../../components/ui/code";
import { RadioCards, Switch } from "../../../components/ui/controls";
import { DataTable } from "../../../components/ui/data-table";
import { AlertDialog } from "../../../components/ui/dialog";
import { Alert, Badge } from "../../../components/ui/feedback";
import { Card, CardLabel, Grid, Section } from "../../../components/ui/layout";
import { Select } from "../../../components/ui/select";
import { Tabs } from "../../../components/ui/tabs";
import { api } from "../../../lib/api";
import { fmtTime } from "../../../lib/format";
import type { BootnetStatus, Observation, ProbeReport } from "../../../lib/types";
import { AsyncView } from "../../AsyncView";
import { useAction, useBootnetStatus, useObservations, useSnippets, useSystemInfo } from "../../data";
import { SingletonSave, useSingleton } from "../configs/singleton";

const MODES = [
  { value: "proxy_dhcp", title: "ProxyDHCP", sub: "与现有 DHCP 共存，只补充引导信息" },
  { value: "external_dhcp", title: "复用现有 DHCP", sub: "在现有 DHCP 上配置片段，本机只提供 TFTP/HTTP" },
  { value: "redfish_only", title: "仅 Redfish", sub: "不应答 DHCP，经 BMC 虚拟介质引导" },
];

const EXTERNAL: Record<string, string> = {
  correct: "现有 DHCP 已正确配置",
  missing_next_server: "缺少 next-server",
  file_mismatch: "引导文件不一致",
  no_dhcp: "未发现 DHCP 服务器",
};

function StatusCard({ status }: { status: BootnetStatus | null }) {
  const beat = status?.heartbeat?.heartbeat ?? null;
  return (
    <Card>
      <CardLabel>守护状态</CardLabel>
      {beat === null ? (
        <p className="muted-text">尚未收到 xpxe-bootnet 心跳</p>
      ) : (
        <dl className="kv">
          <dt>健康</dt>
          <dd>
            <Badge tone={status?.healthy === true ? "success" : "danger"} dot>
              {status?.healthy === true ? "正常" : "异常"}
            </Badge>
            {beat.detail !== null && <span className="muted-text"> {beat.detail}</span>}
          </dd>
          <dt>模式</dt>
          <dd>{beat.mode}</dd>
          <dt>监听端口</dt>
          <dd className="mono">{beat.listening.join(", ") || "—"}</dd>
          <dt>心跳</dt>
          <dd className="tabular">{fmtTime(status?.heartbeat?.received_at ?? null)}</dd>
          <dt>计数</dt>
          <dd className="mono">
            {Object.entries(beat.counters)
              .filter(([, value]) => value > 0)
              .map(([name, value]) => `${name}=${String(value)}`)
              .join(" ") || "全部为 0"}
          </dd>
        </dl>
      )}
    </Card>
  );
}

function ProbeCard({ probe }: { probe: ProbeReport | null }) {
  const { perform, busy } = useAction();
  const [confirm, setConfirm] = useState(false);
  const conflicts = probe?.conflicts ?? [];
  return (
    <Card>
      <div className="row-between">
        <CardLabel>现网扫描</CardLabel>
        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => {
            void perform({ success: "已请求重新扫描", failure: "扫描失败", invalidate: [["bootnet"]], run: () => api("/bootnet/scan", { method: "POST" }) });
          }}
        >
          重新扫描
        </Button>
      </div>
      {probe === null ? (
        <p className="muted-text">尚无扫描结果</p>
      ) : (
        <div className="page-stack">
          <span className="muted-text tabular">扫描于 {fmtTime(probe.at)}</span>
          {probe.external !== null && <Badge tone={probe.external === "correct" ? "success" : "warn"}>{EXTERNAL[probe.external] ?? probe.external}</Badge>}
          {probe.responders.length === 0 ? (
            <p className="muted-text">未发现其他 DHCP 应答</p>
          ) : (
            <ul className="list-plain">
              {probe.responders.map((item) => (
                <li key={item.server} className="row-gap">
                  <span className="mono">{item.server}</span>
                  <Badge>{item.class}</Badge>
                  {item.file !== null && <span className="mono muted-text">{item.file}</span>}
                </li>
              ))}
            </ul>
          )}
          {conflicts.length > 0 && (
            <Alert
              tone="warn"
              title={`发现 ${String(conflicts.length)} 个冲突的引导应答者`}
              action={
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    setConfirm(true);
                  }}
                >
                  确认并继续应答
                </Button>
              }
            >
              <span className="mono">{conflicts.join(", ")}</span>
            </Alert>
          )}
        </div>
      )}
      <AlertDialog
        open={confirm}
        onOpenChange={setConfirm}
        title="确认现网冲突"
        description="确认后本机将继续应答 PXE 请求；同网段的其他引导服务器可能导致机器从错误来源引导。此操作写入审计。"
        confirmLabel="确认冲突"
        busy={busy}
        onConfirm={() => {
          void perform({ success: "已确认", failure: "确认失败", invalidate: [["bootnet"]], run: () => api("/bootnet/acknowledge", { method: "POST", body: { servers: conflicts } }) }).then(() => {
            setConfirm(false);
          });
        }}
      />
    </Card>
  );
}

function SnippetTabs() {
  const snippets = useSnippets(true);
  const [tab, setTab] = useState("");
  const items = snippets.data?.snippets ?? [];
  const current = items.some((item) => item.kind === tab) ? tab : (items[0]?.kind ?? "");
  if (items.length === 0) return null;
  return (
    <Card>
      <CardLabel>现有 DHCP 配置片段</CardLabel>
      <Tabs label="DHCP 片段" value={current} onChange={setTab} items={items.map((item) => ({ value: item.kind, label: item.kind, content: <CodeBlock code={item.text} label={`${item.kind} 片段`} /> }))} />
    </Card>
  );
}

function Observations() {
  const query = useObservations();
  const navigate = useNavigate();
  return (
    <Section title="DHCP 观测" description="最近收到的 PXE 请求；未登记的 MAC 可直接导入为节点。">
      <AsyncView query={query} isEmpty={(rows) => rows.length === 0} empty={{ title: "暂无观测", description: "守护进程收到 PXE 请求后显示在这里。" }}>
        {(rows) => (
          <DataTable<Observation>
            id="observations"
            label="DHCP 观测"
            rows={rows}
            rowKey={(row) => `${row.mac}-${String(row.at)}`}
            searchText={(row) => `${row.mac} ${row.kind} ${row.detail}`}
            columns={[
              { id: "at", header: "时间", cell: (row) => fmtTime(row.at), sortValue: (row) => row.at, data: true },
              { id: "mac", header: "MAC", cell: (row) => row.mac, data: true, hideable: false },
              { id: "arch", header: "架构", cell: (row) => row.arch },
              { id: "kind", header: "类型", cell: (row) => <Badge>{row.kind}</Badge> },
              { id: "detail", header: "说明", cell: (row) => row.detail },
            ]}
            rowMenu={() => [{ kind: "item", value: "import", label: "导入为节点" }]}
            onRowMenu={(row) => {
              void navigate(`/nodes?import=${encodeURIComponent(row.mac)}`);
            }}
          />
        )}
      </AsyncView>
    </Section>
  );
}

function Editor() {
  const net = useSingleton("boot_network");
  const rules = useSingleton("dhcp_rules");
  const system = useSystemInfo();
  const status = useBootnetStatus();
  const [creating, setCreating] = useState(false);
  const ifaces = system.data?.bootnet_allowed_ifaces ?? [];
  const doc = net.doc ?? (creating ? { mode: "proxy_dhcp", iface: ifaces[0] ?? "" } : null);
  const rulesDoc = rules.doc ?? { uefi_firmware: "ipxe_efi", http_boot_enabled: true, mirror_bootfile_options: false };
  const mode = typeof doc?.mode === "string" ? doc.mode : "proxy_dhcp";
  const iface = typeof doc?.iface === "string" ? doc.iface : "";
  const setNet = (next: Record<string, unknown>) => {
    net.setDoc(next);
  };
  return (
    <AsyncView
      query={net.list}
      isEmpty={(items) => items.length === 0 && !creating}
      empty={{
        title: "尚未配置引导网络",
        description: "选择 DHCP 协作模式与监听网卡后，xpxe-bootnet 才会开始应答。",
        action: (
          <Button
            variant="primary"
            onClick={() => {
              setCreating(true);
              net.setDoc({ mode: "proxy_dhcp", iface: ifaces[0] ?? "" });
            }}
          >
            开始配置
          </Button>
        ),
      }}
    >
      {() => (
        <div className="page-stack">
          <Grid cols={2}>
            <StatusCard status={status.data ?? null} />
            <ProbeCard probe={status.data?.heartbeat?.heartbeat.probe ?? null} />
          </Grid>
          <Card>
            <div className="page-stack">
              <RadioCards
                label="DHCP 协作模式"
                value={mode}
                onChange={(value) => {
                  setNet(value === "redfish_only" ? { mode: value } : { mode: value, iface: iface === "" ? (ifaces[0] ?? "") : iface });
                }}
                options={MODES}
              />
              {mode !== "redfish_only" && (
                <Select
                  label="监听网卡"
                  value={iface === "" ? null : iface}
                  onChange={(value) => {
                    setNet({ mode, iface: value });
                  }}
                  options={ifaces.map((item) => ({ value: item, label: item }))}
                  placeholder={ifaces.length === 0 ? "未配置 XPXE_BOOTNET_ALLOWED_IFACES" : "请选择"}
                />
              )}
              <div className="row-end">
                <SingletonSave kind="boot_network" state={net} />
              </div>
            </div>
          </Card>
          {mode === "external_dhcp" && <SnippetTabs />}
          {mode !== "redfish_only" && (
            <Card>
              <div className="page-stack">
                <CardLabel>DHCP 规则</CardLabel>
                <Select
                  label="UEFI 固件"
                  value={typeof rulesDoc.uefi_firmware === "string" ? rulesDoc.uefi_firmware : "ipxe_efi"}
                  onChange={(value) => {
                    rules.setDoc({ ...rulesDoc, uefi_firmware: value });
                  }}
                  options={[
                    { value: "ipxe_efi", label: "ipxe.efi（完整驱动）" },
                    { value: "snp_only_efi", label: "snponly.efi（固件网卡驱动）" },
                  ]}
                />
                <Switch label="UEFI HTTP Boot" checked={rulesDoc.http_boot_enabled === true} onChange={(value) => { rules.setDoc({ ...rulesDoc, http_boot_enabled: value }); }} />
                <Switch label="镜像 bootfile 选项（66/67）" checked={rulesDoc.mirror_bootfile_options === true} onChange={(value) => { rules.setDoc({ ...rulesDoc, mirror_bootfile_options: value }); }} />
                <div className="row-end">
                  <SingletonSave kind="dhcp_rules" state={rules} fallback={rulesDoc} />
                </div>
              </div>
            </Card>
          )}
        </div>
      )}
    </AsyncView>
  );
}

export function BootNetworkPage() {
  return (
    <div className="page-stack">
      <Section title="引导网络" description="与现网 DHCP 的协作方式；保存后守护进程在下一次轮询生效。">
        <Editor />
      </Section>
      <Observations />
    </div>
  );
}
