// 资产页“ISO 镜像”标签（Step 14 §3.5）：分块上传（断点续传）+ 进度、ISO 表（卷标、大小、SHA-256、识别结果、引用条目）、
// 删除前 AlertDialog 列出引用它的菜单条目。
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "../../../components/ui/button";
import { CopyButton } from "../../../components/ui/code";
import { DataTable } from "../../../components/ui/data-table";
import { AlertDialog } from "../../../components/ui/dialog";
import { Alert, Badge, Progress } from "../../../components/ui/feedback";
import { Field, Input } from "../../../components/ui/field";
import { FileDrop } from "../../../components/ui/file-drop";
import { Card, CardLabel } from "../../../components/ui/layout";
import { notify } from "../../../components/ui/toast";
import { api } from "../../../lib/api";
import { fmtBytes, fmtTime, shortSha } from "../../../lib/format";
import type { IsoItem } from "../../../lib/types";
import { uploadIso } from "../../../lib/upload";
import { AsyncView } from "../../AsyncView";
import { errorText, useAction, useIsos, useMe } from "../../data";

export const FAMILY_LABEL: Record<string, string> = { pve: "Proxmox VE 系", casper: "Ubuntu（casper）", live: "Debian Live", debian_installer: "Debian 安装器", unknown: "未识别" };
const SHA_RE = /^[0-9a-fA-F]{64}$/u;

function Uploader({ maxBytes }: { maxBytes: number }) {
  const client = useQueryClient();
  const [files, setFiles] = useState<File[]>([]);
  const [label, setLabel] = useState("");
  const [expected, setExpected] = useState("");
  const [ratio, setRatio] = useState<number | null>(null);
  const file = files[0];
  const badSha = expected.trim() !== "" && !SHA_RE.test(expected.trim());
  const start = async () => {
    if (file === undefined) return;
    setRatio(0);
    try {
      const result = await uploadIso(file, label.trim() === "" ? file.name : label.trim(), expected, setRatio);
      notify("ISO 已上传", "success", `${FAMILY_LABEL[result.detection.family] ?? result.detection.family} · ${shortSha(result.sha256)}`);
      setFiles([]);
      setLabel("");
      setExpected("");
      void client.invalidateQueries({ queryKey: ["isos"] });
      void client.invalidateQueries({ queryKey: ["audit"] });
    } catch (error) {
      notify("上传失败", "danger", errorText(error));
    } finally {
      setRatio(null);
    }
  };
  return (
    <Card>
      <div className="page-stack">
        <CardLabel>上传 ISO</CardLabel>
        <FileDrop label="选择或拖入 ISO" accept=".iso,application/x-iso9660-image" maxBytes={maxBytes} onFiles={setFiles} />
        <Field label="标签" hint="留空时使用文件名">
          <Input
            value={label}
            maxLength={128}
            onChange={(event) => {
              setLabel(event.target.value);
            }}
          />
        </Field>
        <Field label="期望的 SHA-256" hint="可选；不一致即拒收" {...(badSha ? { error: "须为 64 位十六进制" } : {})}>
          <Input
            className="mono"
            value={expected}
            onChange={(event) => {
              setExpected(event.target.value);
            }}
          />
        </Field>
        {ratio !== null && <Progress value={Math.round(ratio * 100)} label="上传进度" />}
        <div className="row-end">
          <Button
            variant="primary"
            disabled={file === undefined || badSha || ratio !== null}
            onClick={() => {
              void start();
            }}
          >
            {ratio === null ? "上传" : "上传中…"}
          </Button>
        </div>
      </div>
    </Card>
  );
}

export function IsoTab() {
  const query = useIsos();
  const me = useMe();
  const admin = me.data?.role === "admin";
  const { perform, busy } = useAction();
  const [pending, setPending] = useState<IsoItem | null>(null);
  return (
    <div className="page-stack">
      {!admin && <Alert title="只读">上传与删除 ISO 需要 admin 角色。</Alert>}
      {admin && <Uploader maxBytes={query.data?.max_bytes ?? 16 * 1024 * 1024 * 1024} />}
      <AsyncView
        query={query}
        isEmpty={(view) => view === null || view.items.length === 0}
        empty={{ title: "还没有 ISO", description: "上传安装 ISO 后，可在“手动引导菜单”中引用它。" }}
      >
        {(view) =>
          view === null ? null : (
            <div className="page-stack">
              <p className="muted-text tabular">
                已用 {fmtBytes(view.usage_bytes)} / 配额 {fmtBytes(view.quota_bytes)}
              </p>
              <DataTable<IsoItem>
                id="isos"
                label="ISO 镜像"
                rows={view.items}
                rowKey={(row) => row.iso.sha256}
                searchText={(row) => `${row.iso.label} ${row.iso.volume_id} ${row.iso.sha256} ${row.iso.detected_family}`}
                columns={[
                  { id: "label", header: "标签", cell: (row) => row.iso.label, hideable: false },
                  { id: "volume", header: "卷标", cell: (row) => <span className="mono">{row.iso.volume_id || "—"}</span> },
                  { id: "size", header: "大小", cell: (row) => fmtBytes(row.iso.size), sortValue: (row) => row.iso.size, data: true },
                  {
                    id: "sha",
                    header: "SHA-256",
                    data: true,
                    cell: (row) => (
                      <span className="row-gap">
                        <span className="mono">{shortSha(row.iso.sha256)}</span>
                        <CopyButton value={row.iso.sha256} label="复制 SHA-256" />
                      </span>
                    ),
                  },
                  {
                    id: "family",
                    header: "识别结果",
                    cell: (row) => (
                      <span className="row-gap">
                        <Badge tone={row.iso.detected_family === "unknown" ? "neutral" : "success"}>{FAMILY_LABEL[row.iso.detected_family] ?? row.iso.detected_family}</Badge>
                        {row.iso.hybrid && <Badge tone="outline">hybrid</Badge>}
                      </span>
                    ),
                  },
                  { id: "refs", header: "引用条目", cell: (row) => (row.referenced_by.length === 0 ? "—" : row.referenced_by.join("、")) },
                  { id: "at", header: "上传时间", cell: (row) => fmtTime(row.iso.uploaded_at), data: true },
                ]}
                rowMenu={() => [{ kind: "item", value: "delete", label: "删除", danger: true, disabled: !admin }]}
                onRowMenu={(row) => {
                  setPending(row);
                }}
              />
            </div>
          )
        }
      </AsyncView>
      <AlertDialog
        open={pending !== null}
        onOpenChange={(open) => {
          if (!open) setPending(null);
        }}
        title="删除 ISO"
        description="删除后 ISO 文件与抽取产物从磁盘移除；引用它的菜单条目将显示为不可用。"
        confirmLabel="删除"
        busy={busy}
        onConfirm={() => {
          if (pending === null) return;
          void perform({ success: "已删除", failure: "删除失败", invalidate: [["isos"], ["boot-menu"], ["audit"]], run: () => api(`/isos/${pending.iso.sha256}`, { method: "DELETE" }) }).then((result) => {
            if (result !== undefined) setPending(null);
          });
        }}
      >
        {pending !== null && (
          <dl className="kv">
            <dt>ISO</dt>
            <dd>{pending.iso.label}</dd>
            <dt>引用条目</dt>
            <dd data-testid="iso-refs">{pending.referenced_by.length === 0 ? "无" : pending.referenced_by.join("、")}</dd>
          </dl>
        )}
      </AlertDialog>
    </div>
  );
}
