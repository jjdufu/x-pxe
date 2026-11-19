// Profile 与初始化计划列表（step-12 §3.4/3.5）：DataTable + 从预设或空白新建。
import { useQueries } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router";
import { Button } from "../../../components/ui/button";
import { DataTable, type Column } from "../../../components/ui/data-table";
import { Dialog } from "../../../components/ui/dialog";
import { Field, Input } from "../../../components/ui/field";
import { Section } from "../../../components/ui/layout";
import { RadioCards } from "../../../components/ui/controls";
import { api } from "../../../lib/api";
import { fmtTime } from "../../../lib/format";
import type { ConfigHead, ConfigObject } from "../../../lib/types";
import { AsyncView } from "../../AsyncView";
import { useAction, useConfigs, useHistory, useNodes, usePresets } from "../../data";

interface ListSpec {
  kind: "profile" | "init_plan";
  title: string;
  description: string;
  route: string;
  emptyTitle: string;
  blank: Record<string, unknown>;
}

interface Row extends ConfigObject {
  summary: string;
  detail: string;
  refs: number;
  author: string;
  updated: number | null;
}

function summarize(kind: string, doc: Record<string, unknown> | undefined): [string, string] {
  if (doc === undefined) return ["—", "—"];
  if (kind === "profile") {
    const mirror = doc.mirror as { scheme?: string; host_port?: string; directory?: string } | undefined;
    const parts = doc.partitioning as { type?: string } | undefined;
    return [mirror === undefined ? "—" : `${mirror.scheme ?? ""}://${mirror.host_port ?? ""}${mirror.directory ?? ""}`, parts?.type ?? "—"];
  }
  const modules = doc.modules as Record<string, { enabled?: boolean }> | undefined;
  const disabled = Object.values(modules ?? {}).filter((item) => item.enabled === false).length;
  return [`${String(Object.keys(modules ?? {}).length)} 个模块有自定义`, `${String(disabled)} 个模块关闭`];
}

function CreateDialog({ spec, open, onOpenChange }: { spec: ListSpec; open: boolean; onOpenChange: (open: boolean) => void }) {
  const presets = usePresets();
  const names = Object.keys((spec.kind === "profile" ? presets.data?.profile : presets.data?.init_plan) ?? {});
  const [source, setSource] = useState("");
  const [name, setName] = useState("");
  const { perform, busy } = useAction();
  const navigate = useNavigate();
  const chosen = source === "" ? (names[0] ?? "blank") : source;
  const create = async () => {
    const table = spec.kind === "profile" ? presets.data?.profile : presets.data?.init_plan;
    const base = chosen === "blank" ? spec.blank : (table?.[chosen] ?? spec.blank);
    const document = { ...base, name: name.trim() };
    const created = await perform({
      success: "已新建",
      failure: "新建失败",
      invalidate: [["configs", spec.kind], ["history"]],
      run: () => api<{ object_id: number }>(`/configs/${spec.kind}`, { method: "POST", body: { name: name.trim(), document } }),
    });
    if (created !== undefined) {
      onOpenChange(false);
      void navigate(`${spec.route}/${String(created.object_id)}`);
    }
  };
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={`新建${spec.title}`}
      footer={
        <>
          <Button
            variant="outline"
            onClick={() => {
              onOpenChange(false);
            }}
          >
            取消
          </Button>
          <Button
            variant="primary"
            disabled={busy || name.trim() === ""}
            onClick={() => {
              void create();
            }}
          >
            新建
          </Button>
        </>
      }
    >
      <div className="page-stack">
        <Field label="名称" required>
          <Input
            value={name}
            onChange={(event) => {
              setName(event.target.value);
            }}
          />
        </Field>
        <RadioCards
          label="来源"
          columns={3}
          value={chosen}
          onChange={setSource}
          options={[...names.map((item) => ({ value: item, title: item, sub: "内置预设" })), { value: "blank", title: "空白", sub: "最小可用文档" }]}
        />
      </div>
    </Dialog>
  );
}

export function ConfigListPage(spec: ListSpec) {
  const query = useConfigs(spec.kind);
  const nodes = useNodes();
  const history = useHistory();
  const navigate = useNavigate();
  const [creating, setCreating] = useState(false);
  const heads = useQueries({
    queries: (query.data ?? []).map((item) => ({
      queryKey: ["configs", spec.kind, item.id],
      queryFn: () => api<ConfigHead>(`/configs/${spec.kind}/${String(item.id)}`),
    })),
  });
  const rowsOf = (items: ConfigObject[]): Row[] =>
    items.map((item, index) => {
      const [summary, detail] = summarize(spec.kind, heads[index]?.data?.document);
      const last = (history.data ?? []).find((entry) => entry.kind === spec.kind && entry.object_id === item.id && entry.version === item.head_version);
      const refs = (nodes.data ?? []).filter((node) => (spec.kind === "profile" ? node.profile_id : node.init_plan_id) === item.id).length;
      return { ...item, summary, detail, refs, author: last?.author ?? "—", updated: last?.created_at ?? null };
    });
  const columns: Column<Row>[] = [
    { id: "name", header: "名称", cell: (row) => row.name, sortValue: (row) => row.name, hideable: false },
    { id: "summary", header: spec.kind === "profile" ? "镜像源" : "模块", cell: (row) => <span className="mono">{row.summary}</span> },
    { id: "detail", header: spec.kind === "profile" ? "分区方案" : "关闭", cell: (row) => row.detail },
    { id: "refs", header: "引用节点", cell: (row) => row.refs, sortValue: (row) => row.refs, data: true },
    { id: "head", header: "头版本", cell: (row) => `v${String(row.head_version)}`, sortValue: (row) => row.head_version, data: true },
    { id: "author", header: "更新人", cell: (row) => row.author },
    { id: "updated", header: "更新时间", cell: (row) => fmtTime(row.updated), sortValue: (row) => row.updated ?? 0, data: true },
  ];
  const newButton = (
    <Button
      size="sm"
      variant="primary"
      onClick={() => {
        setCreating(true);
      }}
    >
      <Plus size={14} aria-hidden="true" />
      新建
    </Button>
  );
  return (
    <Section title={spec.title} description={spec.description}>
      <AsyncView query={query} isEmpty={(items) => items.length === 0} empty={{ title: spec.emptyTitle, description: "从内置预设或空白文档新建。", action: newButton }}>
        {(items) => (
          <DataTable
            id={`configs-${spec.kind}`}
            label={spec.title}
            rows={rowsOf(items)}
            rowKey={(row) => String(row.id)}
            columns={columns}
            searchText={(row) => `${row.name} ${row.summary}`}
            primaryAction={newButton}
            rowMenu={() => [
              { kind: "item", value: "edit", label: "编辑", shortcut: "Enter" },
              { kind: "item", value: "history", label: "查看历史" },
            ]}
            onRowMenu={(row, value) => {
              void navigate(value === "edit" ? `${spec.route}/${String(row.id)}` : `/history?object=${spec.kind}:${String(row.id)}`);
            }}
            onRowActivate={(row) => {
              void navigate(`${spec.route}/${String(row.id)}`);
            }}
          />
        )}
      </AsyncView>
      <CreateDialog spec={spec} open={creating} onOpenChange={setCreating} />
    </Section>
  );
}
