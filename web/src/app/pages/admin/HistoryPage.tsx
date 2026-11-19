// 配置历史（step-12 §3.8）：勾选同一对象的两个版本对比（字段差异 + 渲染差异）；回滚到任一版本（生成新版本）。
import { useState } from "react";
import { useSearchParams } from "react-router";
import { Button } from "../../../components/ui/button";
import { DataTable } from "../../../components/ui/data-table";
import { AlertDialog, Dialog } from "../../../components/ui/dialog";
import { Badge } from "../../../components/ui/feedback";
import { Field, Textarea } from "../../../components/ui/field";
import { Section } from "../../../components/ui/layout";
import { api } from "../../../lib/api";
import { fmtTime, shortSha } from "../../../lib/format";
import type { HistoryRow, PreviewResult } from "../../../lib/types";
import { AsyncView } from "../../AsyncView";
import { errorText, useAction, useHistory } from "../../data";
import { DiffContent } from "../configs/shared";

const KIND_LABEL: Record<string, string> = { profile: "Profile", init_plan: "初始化计划", boot_network: "引导网络", dhcp_rules: "DHCP 规则", timeouts: "装机超时", node_spec: "节点规格", boot_menu: "手动引导菜单" };
const keyOf = (row: HistoryRow) => `${row.kind}:${String(row.object_id)}:${String(row.version)}`;

function CompareDialog({ pair, onClose }: { pair: [HistoryRow, HistoryRow] | null; onClose: () => void }) {
  const [result, setResult] = useState<{ key: string; diff: PreviewResult | null; error: string | null } | null>(null);
  const key = pair === null ? "" : `${keyOf(pair[0])}-${keyOf(pair[1])}`;
  if (pair !== null && result?.key !== key) {
    const [a, b] = pair;
    const [from, to] = a.version < b.version ? [a, b] : [b, a];
    setResult({ key, diff: null, error: null });
    api<PreviewResult>(`/configs/${from.kind}/${String(from.object_id)}/diff?from=${String(from.version)}&to=${String(to.version)}`)
      .then((diff) => {
        setResult({ key, diff, error: null });
      })
      .catch((error: unknown) => {
        setResult({ key, diff: null, error: errorText(error) });
      });
  }
  return (
    <Dialog
      open={pair !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={pair === null ? "对比" : `${pair[0].name}：v${String(Math.min(pair[0].version, pair[1].version))} → v${String(Math.max(pair[0].version, pair[1].version))}`}
    >
      {result?.error != null ? <p className="field-error">{result.error}</p> : result?.diff == null ? <p className="muted-text">加载中…</p> : <DiffContent preview={result.diff} />}
    </Dialog>
  );
}

export function HistoryPage() {
  const query = useHistory();
  const [params] = useSearchParams();
  const [pair, setPair] = useState<[HistoryRow, HistoryRow] | null>(null);
  const [target, setTarget] = useState<HistoryRow | null>(null);
  const [comment, setComment] = useState("");
  const { perform, busy } = useAction();
  const object = params.get("object") ?? "";
  return (
    <Section title="配置历史" description="所有配置对象的版本；勾选同一对象的两个版本即可对比。">
      <AsyncView query={query} isEmpty={(rows) => rows.length === 0} empty={{ title: "暂无配置版本", description: "新建 Profile、初始化计划或引导网络后在这里查看历史。" }}>
        {(rows) => (
          <DataTable<HistoryRow>
            id="history"
            label="配置历史"
            rows={object === "" ? rows : rows.filter((row) => `${row.kind}:${String(row.object_id)}` === object)}
            rowKey={keyOf}
            searchText={(row) => `${row.name} ${row.comment} ${row.author ?? ""}`}
            filter={{ label: "类型", options: Object.entries(KIND_LABEL).map(([value, label]) => ({ value, label })), match: (row, value) => row.kind === value }}
            columns={[
              { id: "kind", header: "类型", cell: (row) => <Badge>{KIND_LABEL[row.kind] ?? row.kind}</Badge>, sortValue: (row) => row.kind },
              { id: "name", header: "对象", cell: (row) => row.name, sortValue: (row) => row.name, hideable: false },
              { id: "version", header: "版本", cell: (row) => <span>v{row.version}{row.version === row.head_version && <Badge tone="success">头</Badge>}</span>, sortValue: (row) => row.version, data: true },
              { id: "author", header: "作者", cell: (row) => row.author ?? "—" },
              { id: "at", header: "时间", cell: (row) => fmtTime(row.created_at), sortValue: (row) => row.created_at, data: true },
              { id: "comment", header: "说明", cell: (row) => row.comment || "—" },
              { id: "sha", header: "SHA-256", cell: (row) => shortSha(row.content_sha256), data: true },
            ]}
            bulkActions={(selected) => {
              const [a, b] = selected;
              const same = selected.length === 2 && a !== undefined && b !== undefined && a.kind === b.kind && a.object_id === b.object_id;
              return (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!same}
                  onClick={() => {
                    if (same) setPair([a, b]);
                  }}
                >
                  对比
                </Button>
              );
            }}
            rowMenu={(row) => [{ kind: "item", value: "rollback", label: "回滚到此版本", disabled: row.version === row.head_version || row.kind === "node_spec" }]}
            onRowMenu={(row) => {
              setTarget(row);
            }}
          />
        )}
      </AsyncView>
      <CompareDialog
        pair={pair}
        onClose={() => {
          setPair(null);
        }}
      />
      <AlertDialog
        open={target !== null}
        onOpenChange={(open) => {
          if (!open) setTarget(null);
        }}
        title={target === null ? "回滚" : `回滚 ${target.name} 到 v${String(target.version)}`}
        description="回滚会以该版本内容生成一个新版本，历史不会被删除。"
        confirmLabel="回滚"
        busy={busy}
        onConfirm={() => {
          if (target === null) return;
          void perform({
            success: "已回滚",
            failure: "回滚失败",
            invalidate: [["history"], ["configs", target.kind]],
            run: () => api(`/configs/${target.kind}/${String(target.object_id)}/rollback`, { method: "POST", body: { to_version: target.version, expected_head: target.head_version, comment: comment.trim() === "" ? null : comment.trim() } }),
          }).then(() => {
            setTarget(null);
            setComment("");
          });
        }}
      >
        <Field label="说明">
          <Textarea
            value={comment}
            onChange={(event) => {
              setComment(event.target.value);
            }}
          />
        </Field>
      </AlertDialog>
    </Section>
  );
}
