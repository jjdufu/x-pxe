// 节点页（step-12 §3.3）：DataTable（键盘选行、行菜单、右键菜单）、批量操作、新建/导入、详情抽屉；SSE 只使查询失效，表格状态保持。
import { Plus, Radar } from "lucide-react";
import { useMemo, useState } from "react";
import { useSearchParams } from "react-router";
import { Button } from "../../../components/ui/button";
import { DataTable, type Column } from "../../../components/ui/data-table";
import { Badge } from "../../../components/ui/feedback";
import { Section } from "../../../components/ui/layout";
import type { MenuEntry } from "../../../components/ui/menu";
import { notify } from "../../../components/ui/toast";
import { copyText } from "../../../lib/clipboard";
import { fmtTime, stateLabel } from "../../../lib/format";
import type { NodeRow } from "../../../lib/types";
import { AsyncView, EmptyState } from "../../AsyncView";
import { useConfigs, useNodes } from "../../data";
import { ACTIVE_STATES, canArm, canDisarm, Countdown, StateBadge, STATES } from "./common";
import { ConfirmAction, ImportDialog, NewNodeDialog, type Pending } from "./NodeDialogs";
import { NodeDrawer } from "./NodeDrawer";

function rowMenu(node: NodeRow): MenuEntry[] {
  return [
    { kind: "item", value: "arm", label: "arm", disabled: !canArm(node) },
    { kind: "item", value: "disarm", label: "disarm", disabled: !canDisarm(node) },
    { kind: "item", value: "rearm", label: "重新装机", danger: true, disabled: node.state !== "ready" },
    { kind: "item", value: "deploy", label: "Redfish 部署", danger: true, disabled: node.channel.type !== "redfish" || node.state !== "armed" },
    { kind: "item", value: "eject", label: "弹出介质", disabled: node.channel.type !== "redfish" },
    { kind: "item", value: "reset", label: "重置为 Idle", disabled: node.state !== "failed" },
    { kind: "divider", value: "d1" },
    { kind: "item", value: "copy", label: "复制 MAC" },
    { kind: "item", value: "open", label: "打开详情", shortcut: "Enter" },
  ];
}

const FILTER_STATES = [...STATES.map((state) => ({ value: state, label: stateLabel(state) })), { value: "active", label: "进行中" }];

function NodesTable({ rows }: { rows: NodeRow[] }) {
  const [params, setParams] = useSearchParams();
  const profiles = useConfigs("profile");
  const plans = useConfigs("init_plan");
  const [pending, setPending] = useState<Pending>(null);
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [importMac, setImportMac] = useState("");
  const openId = params.get("node");
  const openNode = openId === null ? undefined : rows.find((row) => String(row.id) === openId);
  const initialState = params.get("state") ?? undefined;

  const setOpen = (id: number | null) => {
    const next = new URLSearchParams(params);
    if (id === null) next.delete("node");
    else next.set("node", String(id));
    next.delete("action");
    setParams(next, { replace: true });
  };

  // 命令面板 “arm <主机名>” 跳转：/nodes?node=<id>&action=arm（确认框关闭时清除 action）
  const action = params.get("action");
  const shown: Pending = pending ?? (action === "arm" && openNode !== undefined ? { kind: "arm", nodes: [openNode] } : null);

  const names = useMemo(() => {
    const map = new Map<string, string>();
    for (const item of profiles.data ?? []) map.set(`p${String(item.id)}`, item.name);
    for (const item of plans.data ?? []) map.set(`i${String(item.id)}`, item.name);
    return map;
  }, [profiles.data, plans.data]);

  const columns: Column<NodeRow>[] = [
    { id: "hostname", header: "主机名", cell: (row) => row.hostname, sortValue: (row) => row.hostname, hideable: false },
    { id: "mac", header: "MAC", cell: (row) => row.mac, sortValue: (row) => row.mac, data: true },
    { id: "channel", header: "通道", cell: (row) => <Badge tone="outline">{row.channel.type === "redfish" ? "Redfish" : "LAN"}</Badge> },
    { id: "firmware", header: "固件", cell: (row) => row.firmware.toUpperCase() },
    { id: "profile", header: "Profile", cell: (row) => names.get(`p${String(row.profile_id)}`) ?? `#${String(row.profile_id)}` },
    { id: "plan", header: "初始化计划", cell: (row) => names.get(`i${String(row.init_plan_id)}`) ?? `#${String(row.init_plan_id)}` },
    { id: "state", header: "状态", cell: (row) => <StateBadge state={row.state} />, sortValue: (row) => row.state, hideable: false },
    { id: "deadline", header: "截止", cell: (row) => <Countdown deadline={row.deadline_at} />, data: true },
    { id: "since", header: "状态起始", cell: (row) => fmtTime(row.state_since), sortValue: (row) => row.state_since, data: true },
  ];

  const onMenu = (row: NodeRow, value: string) => {
    if (value === "open") setOpen(row.id);
    else if (value === "copy") {
      void copyText(row.mac).then((result) => {
        notify(result === "failed" ? "复制失败" : "已复制 MAC", result === "failed" ? "danger" : "success");
      });
    } else if (["arm", "disarm", "rearm", "deploy", "eject", "reset"].includes(value)) {
      setPending({ kind: value as NonNullable<Pending>["kind"], nodes: [row] });
    }
  };

  return (
    <>
      {rows.length === 0 ? (
        <EmptyState
          title="还没有节点"
          description="登记第一台目标机后，它会出现在这里。"
          action={
            <>
              <Button
                variant="primary"
                size="sm"
                onClick={() => {
                  setCreating(true);
                }}
              >
                <Plus size={14} aria-hidden="true" />
                新建节点
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  setImporting(true);
                }}
              >
                从观测导入
              </Button>
            </>
          }
        />
      ) : (
        <DataTable
          id="nodes"
          label="节点"
          rows={rows}
          rowKey={(row) => String(row.id)}
          columns={columns}
          searchText={(row) => `${row.hostname} ${row.mac} ${row.network.type === "static" ? row.network.address : ""} ${row.channel.bmc ?? ""}`}
          filter={{
            label: "状态",
            options: FILTER_STATES,
            match: (row, value) => (value === "active" ? ACTIVE_STATES.includes(row.state) : row.state === value),
          }}
          initialFilter={initialState}
          filters={[
            {
              label: "通道",
              options: [
                { value: "lan", label: "LAN" },
                { value: "redfish", label: "Redfish" },
              ],
              match: (row, value) => row.channel.type === value,
            },
          ]}
          rowMenu={rowMenu}
          onRowMenu={onMenu}
          onRowActivate={(row) => {
            setOpen(row.id);
          }}
          activeKey={openId}
          bulkActions={(selected) => (
            <>
              <Button
                size="sm"
                variant="outline"
                disabled={selected.filter(canArm).length === 0}
                onClick={() => {
                  setPending({ kind: "arm", nodes: selected.filter(canArm) });
                }}
              >
                批量 arm ({selected.filter(canArm).length})
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={selected.filter(canDisarm).length === 0}
                onClick={() => {
                  setPending({ kind: "disarm", nodes: selected.filter(canDisarm) });
                }}
              >
                批量 disarm ({selected.filter(canDisarm).length})
              </Button>
            </>
          )}
          primaryAction={
            <>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  setImporting(true);
                }}
              >
                <Radar size={14} aria-hidden="true" />
                <span className="hide-sm">从观测导入</span>
              </Button>
              <Button
                size="sm"
                variant="primary"
                onClick={() => {
                  setCreating(true);
                }}
              >
                <Plus size={14} aria-hidden="true" />
                新建节点
              </Button>
            </>
          }
        />
      )}
      <NodeDrawer
        node={openNode}
        onClose={() => {
          setOpen(null);
        }}
      />
      <ConfirmAction
        pending={shown}
        onClose={() => {
          setPending(null);
          if (action !== null) {
            const next = new URLSearchParams(params);
            next.delete("action");
            setParams(next, { replace: true });
          }
        }}
      />
      <NewNodeDialog open={creating} onOpenChange={setCreating} initialMac={importMac} />
      <ImportDialog
        open={importing}
        onOpenChange={setImporting}
        known={rows.map((row) => row.mac)}
        onPick={(mac) => {
          setImportMac(mac);
          setImporting(false);
          setCreating(true);
        }}
      />
    </>
  );
}

export function NodesPage() {
  const query = useNodes();
  return (
    <Section title="节点" description="已登记的目标机与当前状态。">
      <AsyncView
        query={query}
        loading={<DataTable id="nodes" label="节点" rows={[]} rowKey={(row: NodeRow) => String(row.id)} columns={[]} loading />}
      >
        {(rows) => <NodesTable rows={rows} />}
      </AsyncView>
    </Section>
  );
}
