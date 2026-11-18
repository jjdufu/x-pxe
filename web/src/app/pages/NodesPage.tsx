// 节点页（Step 11 版本）：DataTable 基座 + 四态；行操作、详情抽屉在 Step 12 实现。
import { useQuery } from "@tanstack/react-query";
import { Badge, type Tone } from "../../components/ui/feedback";
import { DataTable, type Column } from "../../components/ui/data-table";
import { Section } from "../../components/ui/layout";
import { api } from "../../lib/api";
import { AsyncView } from "../AsyncView";

export interface NodeRow {
  id: number;
  hostname: string;
  mac: string;
  state: string;
  channel: string;
  firmware: string;
  state_since: string;
}

const TONES: Record<string, Tone> = { Ready: "success", Failed: "danger", Armed: "warn", Installing: "warn", Installed: "warn", Configuring: "warn" };

const STATES = ["Idle", "Armed", "Installing", "Installed", "Configuring", "Ready", "Failed"];

const columns: Column<NodeRow>[] = [
  { id: "hostname", header: "主机名", cell: (row) => row.hostname, sortValue: (row) => row.hostname, hideable: false },
  { id: "mac", header: "MAC", cell: (row) => row.mac, sortValue: (row) => row.mac, data: true },
  { id: "state", header: "状态", cell: (row) => <Badge tone={TONES[row.state] ?? "neutral"}>{row.state}</Badge>, sortValue: (row) => row.state },
  { id: "channel", header: "通道", cell: (row) => row.channel },
  { id: "firmware", header: "固件", cell: (row) => row.firmware },
  { id: "since", header: "状态起始", cell: (row) => row.state_since, sortValue: (row) => row.state_since, data: true },
];

export function NodesPage() {
  const query = useQuery({ queryKey: ["nodes"], queryFn: () => api<NodeRow[]>("/nodes") });
  return (
    <Section title="节点" description="已登记的目标机与当前状态。">
      <AsyncView
        query={query}
        isEmpty={(rows) => rows.length === 0}
        empty={{ title: "还没有节点", description: "登记第一台目标机后，它会出现在这里。" }}
        loading={<DataTable id="nodes" label="节点" rows={[]} rowKey={(row) => String(row.id)} columns={columns} loading />}
      >
        {(rows) => (
          <DataTable
            id="nodes"
            label="节点"
            rows={rows}
            rowKey={(row) => String(row.id)}
            columns={columns}
            searchText={(row) => `${row.hostname} ${row.mac}`}
            filter={{ label: "状态", options: STATES.map((state) => ({ value: state, label: state })), match: (row, value) => row.state === value }}
          />
        )}
      </AsyncView>
    </Section>
  );
}
