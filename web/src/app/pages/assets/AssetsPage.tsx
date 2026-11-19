// 资产（step-12 §3.7）：每个种类一张卡（当前激活版本、回滚）、获取菜单（官方源 / URL / 上传）、版本表（校验 / 激活 / 删除）。
import { useState } from "react";
import { Button } from "../../../components/ui/button";
import { CopyButton } from "../../../components/ui/code";
import { DataTable } from "../../../components/ui/data-table";
import { AlertDialog, Dialog } from "../../../components/ui/dialog";
import { Alert, Badge, Steps } from "../../../components/ui/feedback";
import { Field, Input, Textarea } from "../../../components/ui/field";
import { FileDrop } from "../../../components/ui/file-drop";
import { Card, CardLabel, Grid, Section } from "../../../components/ui/layout";
import { Tabs } from "../../../components/ui/tabs";
import { Menu } from "../../../components/ui/menu";
import { HoverCard } from "../../../components/ui/overlay";
import { api } from "../../../lib/api";
import { fmtBytes, fmtTime, shortSha } from "../../../lib/format";
import type { AssetRecord, AssetsView } from "../../../lib/types";
import { AsyncView } from "../../AsyncView";
import { useAction, useAssets, useSystemInfo } from "../../data";
import { IsoTab } from "./IsoTab";

const KINDS = [
  { kind: "ipxe_efi_x64", title: "iPXE UEFI（ipxe.efi）" },
  { kind: "ipxe_snponly_x64", title: "iPXE UEFI（snponly.efi）" },
  { kind: "ipxe_undionly", title: "iPXE BIOS（undionly.kpxe）" },
  { kind: "debian_netboot_amd64", title: "Debian netboot（amd64）" },
] as const;

const STAGE_TONE: Record<string, "success" | "warn" | "outline" | "neutral" | "danger"> = { active: "success", verified: "outline", canary: "warn", fetched: "neutral", failed: "danger" };
const STEPS = ["获取", "校验", "金丝雀", "激活"];
const stepOf = (stage: string) => ({ fetched: 1, verified: 2, canary: 3, active: 4 })[stage] ?? 0;

type Pending = { type: "activate" | "delete" | "verify"; asset: AssetRecord } | { type: "rollback"; kind: string } | { type: "url" | "upload"; kind: string } | null;

function Sha({ value }: { value: string }) {
  return (
    <span className="row-gap">
      <HoverCard content={<span className="mono">{value}</span>}>
        <span className="mono" tabIndex={0}>
          {shortSha(value)}
        </span>
      </HoverCard>
      <CopyButton value={value} label="复制 SHA-256" />
    </span>
  );
}

function versionLine(asset: AssetRecord): string {
  const version = asset.verify?.version;
  if (typeof version === "string") return version;
  return asset.kernel_abi ?? asset.label;
}

function KindCard({ kind, title, view, offline, onPending }: { kind: string; title: string; view: AssetsView; offline: boolean; onPending: (pending: Pending) => void }) {
  const { perform, busy } = useAction();
  const channel = view.channels.channels[kind];
  const active = view.assets.find((asset) => asset.id === channel?.asset_id);
  const candidates = view.assets.filter((asset) => asset.kind === kind);
  const fetchOfficial = () => perform({ success: "已开始获取", failure: "获取失败", invalidate: [["assets"]], run: () => api("/assets/fetch", { method: "POST", body: { kind } }) });
  return (
    <Card>
      <div className="row-between">
        <CardLabel>{title}</CardLabel>
        <Menu
          trigger={
            <Button size="sm" variant="outline" disabled={busy}>
              获取
            </Button>
          }
          entries={[
            { kind: "item", value: "official", label: "从官方源获取", disabled: offline },
            { kind: "item", value: "url", label: "从 URL 获取", disabled: offline },
            { kind: "item", value: "upload", label: "上传文件", disabled: kind.startsWith("debian") },
          ]}
          onSelect={(value) => {
            if (value === "official") void fetchOfficial();
            else onPending({ type: value === "url" ? "url" : "upload", kind });
          }}
        />
      </div>
      {active === undefined ? (
        <p className="muted-text">尚未激活任何版本{candidates.length > 0 ? `（有 ${String(candidates.length)} 个候选）` : ""}</p>
      ) : (
        <dl className="kv">
          <dt>当前版本</dt>
          <dd>{versionLine(active)}</dd>
          <dt>SHA-256</dt>
          <dd>
            <Sha value={active.sha256} />
          </dd>
          <dt>来源</dt>
          <dd>{active.source}</dd>
          <dt>日期</dt>
          <dd className="tabular">{fmtTime(active.created_at)}</dd>
          <dt>阶段</dt>
          <dd>
            <Badge tone="success">已激活</Badge>
          </dd>
        </dl>
      )}
      {candidates.length > 1 && (
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            onPending({ type: "rollback", kind });
          }}
        >
          回滚到上一版本
        </Button>
      )}
    </Card>
  );
}

function Dialogs({ pending, onClose, view }: { pending: Pending; onClose: () => void; view: AssetsView }) {
  const { perform, busy } = useAction();
  const [reason, setReason] = useState("");
  const [url, setUrl] = useState("");
  const [files, setFiles] = useState<File[]>([]);
  const close = () => {
    setReason("");
    setUrl("");
    setFiles([]);
    onClose();
  };
  const done = (result: unknown) => {
    if (result !== undefined) close();
  };
  const previous = (kind: string) => {
    const current = view.channels.channels[kind]?.asset_id;
    return view.assets.filter((asset) => asset.kind === kind && asset.id !== current && asset.stage !== "failed")[0];
  };
  return (
    <>
      <AlertDialog
        open={pending?.type === "activate"}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        title="激活资产"
        description="未执行金丝雀装机时必须填写跳过理由（写入审计）。"
        confirmLabel="激活"
        busy={busy}
        confirmDisabled={reason.trim() === ""}
        onConfirm={() => {
          if (pending?.type !== "activate") return;
          void perform({
            success: "已激活",
            failure: "激活失败",
            invalidate: [["assets"], ["audit"]],
            run: () => api("/assets/activate", { method: "POST", body: { kind: pending.asset.kind, asset_id: pending.asset.id, skip_canary_reason: reason.trim() } }),
          }).then(done);
        }}
      >
        {pending?.type === "activate" && (
          <div className="page-stack">
            <Steps labels={STEPS} current={stepOf(pending.asset.stage)} label="激活进度" />
            <Field label="跳过金丝雀的理由" required>
              <Textarea
                value={reason}
                onChange={(event) => {
                  setReason(event.target.value);
                }}
              />
            </Field>
          </div>
        )}
      </AlertDialog>
      <AlertDialog
        open={pending?.type === "delete"}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        title="删除资产"
        description="删除后文件从磁盘移除；正在使用的版本不能删除。"
        confirmLabel="删除"
        busy={busy}
        onConfirm={() => {
          if (pending?.type !== "delete") return;
          void perform({ success: "已删除", failure: "删除失败", invalidate: [["assets"]], run: () => api(`/assets/${String(pending.asset.id)}`, { method: "DELETE" }) }).then(done);
        }}
      />
      <AlertDialog
        open={pending?.type === "rollback"}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        title="回滚到上一版本"
        confirmLabel="回滚"
        busy={busy}
        onConfirm={() => {
          if (pending?.type !== "rollback") return;
          void perform({ success: "已回滚", failure: "回滚失败", invalidate: [["assets"], ["audit"]], run: () => api("/assets/rollback", { method: "POST", body: { kind: pending.kind } }) }).then(done);
        }}
      >
        {pending?.type === "rollback" && (
          <dl className="kv">
            <dt>当前</dt>
            <dd className="mono">{shortSha(view.channels.channels[pending.kind]?.sha256 ?? "")}</dd>
            <dt>回滚到</dt>
            <dd className="mono">{(() => {
              const target = previous(pending.kind);
              return target === undefined ? "—" : `${versionLine(target)} · ${shortSha(target.sha256)}`;
            })()}</dd>
          </dl>
        )}
      </AlertDialog>
      <Dialog
        open={pending?.type === "url"}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        title="从 URL 获取"
        footer={
          <Button
            variant="primary"
            disabled={busy || url.trim() === ""}
            onClick={() => {
              if (pending?.type !== "url") return;
              void perform({ success: "已获取", failure: "获取失败", invalidate: [["assets"]], run: () => api("/assets/fetch", { method: "POST", body: { kind: pending.kind, url: url.trim() } }) }).then(done);
            }}
          >
            获取
          </Button>
        }
      >
        <Field label="下载地址" required>
          <Input
            value={url}
            placeholder="以 http(s):// 开头的下载地址"
            onChange={(event) => {
              setUrl(event.target.value);
            }}
          />
        </Field>
      </Dialog>
      <Dialog
        open={pending?.type === "upload"}
        onOpenChange={(open) => {
          if (!open) close();
        }}
        title="上传资产"
        footer={
          <Button
            variant="primary"
            disabled={busy || files.length === 0}
            onClick={() => {
              const file = files[0];
              if (pending?.type !== "upload" || file === undefined) return;
              void perform({ success: "已上传并校验", failure: "上传失败", invalidate: [["assets"]], run: () => api(`/assets/upload?kind=${pending.kind}`, { method: "POST", raw: file }) }).then(done);
            }}
          >
            上传
          </Button>
        }
      >
        <FileDrop key={pending?.type === "upload" ? pending.kind : "none"} label="选择或拖入文件" maxBytes={512 * 1024 * 1024} onFiles={setFiles} />
        {files[0] !== undefined && (
          <p className="muted-text">
            {files[0].name} · {fmtBytes(files[0].size)}
          </p>
        )}
      </Dialog>
    </>
  );
}

function NetbootAssets() {
  const query = useAssets();
  const system = useSystemInfo();
  const offline = system.data?.offline === true;
  const [pending, setPending] = useState<Pending>(null);
  const { perform } = useAction();
  return (
    <>
      {offline && <Alert title="离线模式">XPXE_OFFLINE=true：只能上传，官方源与 URL 获取已禁用。</Alert>}
      <AsyncView
        query={query}
        isEmpty={(view) => view === null || view.assets.length === 0}
        empty={{
          title: "尚未获取任何资产",
          description: "首次部署需先获取 iPXE 与 Debian netboot，激活后节点才能引导。",
          action: (
            <Button
              variant="primary"
              disabled={offline}
              onClick={() => {
                for (const item of KINDS) void perform({ success: `已开始获取 ${item.title}`, failure: "获取失败", invalidate: [["assets"]], run: () => api("/assets/fetch", { method: "POST", body: { kind: item.kind } }) });
              }}
            >
              从官方源获取
            </Button>
          ),
        }}
      >
        {(view) =>
          view === null ? null : (
            <div className="page-stack">
              <Grid cols={2}>
                {KINDS.map((item) => (
                  <KindCard key={item.kind} kind={item.kind} title={item.title} view={view} offline={offline} onPending={setPending} />
                ))}
              </Grid>
              <DataTable<AssetRecord>
                id="assets"
                label="资产版本"
                rows={view.assets}
                rowKey={(row) => String(row.id)}
                searchText={(row) => `${row.kind} ${row.label} ${row.sha256}`}
                filter={{ label: "种类", options: KINDS.map((item) => ({ value: item.kind, label: item.title })), match: (row, value) => row.kind === value }}
                columns={[
                  { id: "kind", header: "种类", cell: (row) => row.kind, sortValue: (row) => row.kind },
                  { id: "label", header: "标签", cell: (row) => row.label, hideable: false },
                  { id: "sha", header: "SHA-256", cell: (row) => <Sha value={row.sha256} />, data: true },
                  { id: "size", header: "大小", cell: (row) => fmtBytes(row.size), sortValue: (row) => row.size, data: true },
                  { id: "source", header: "来源", cell: (row) => row.source },
                  {
                    id: "stage",
                    header: "阶段",
                    cell: (row) => {
                      const live = view.channels.channels[row.kind]?.asset_id === row.id;
                      return <Badge tone={live ? "success" : (STAGE_TONE[row.stage] ?? "neutral")}>{live ? "已激活" : row.stage}</Badge>;
                    },
                  },
                  { id: "at", header: "时间", cell: (row) => fmtTime(row.created_at), sortValue: (row) => row.created_at, data: true },
                ]}
                rowMenu={(row) => {
                  const live = view.channels.channels[row.kind]?.asset_id === row.id;
                  return [
                    { kind: "item", value: "verify", label: "重新校验" },
                    { kind: "item", value: "activate", label: "激活", disabled: live },
                    { kind: "divider", value: "d1" },
                    { kind: "item", value: "delete", label: "删除", danger: true, disabled: live },
                  ];
                }}
                onRowMenu={(row, value) => {
                  if (value === "verify") {
                    void perform({ success: "校验通过", failure: "校验失败", invalidate: [["assets"]], run: () => api(`/assets/${String(row.id)}/verify`, { method: "POST" }) });
                  } else setPending({ type: value === "delete" ? "delete" : "activate", asset: row });
                }}
              />
            </div>
          )
        }
      </AsyncView>
      <Dialogs pending={pending} onClose={() => { setPending(null); }} view={query.data ?? { assets: [], channels: { channels: {} } }} />
    </>
  );
}

export function AssetsPage() {
  const [tab, setTab] = useState("netboot");
  return (
    <Section title="资产" description="iPXE 与 Debian netboot 的版本、校验与激活；手动网络引导用的安装 ISO。">
      <Tabs
        label="资产类别"
        value={tab}
        onChange={setTab}
        items={[
          { value: "netboot", label: "iPXE 与 netboot", content: <NetbootAssets /> },
          { value: "iso", label: "ISO 镜像", content: <IsoTab /> },
        ]}
      />
    </Section>
  );
}
