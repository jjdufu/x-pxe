// 菜单条目编辑抽屉：类型 RadioCards、ISO Select、路径候选（来自 ISO 目录树）、cmdline 白名单校验。
import { useState } from "react";
import { Button } from "../../../components/ui/button";
import { RadioCards, Switch } from "../../../components/ui/controls";
import { Drawer } from "../../../components/ui/dialog";
import { Alert } from "../../../components/ui/feedback";
import { Field, Input, Textarea } from "../../../components/ui/field";
import { Select } from "../../../components/ui/select";
import type { BootMenuEntry, EntryKind, IsoItem } from "../../../lib/types";
import { useIsoTree } from "../../data";
import { entryErrors, prefill, switchKind } from "./model";

const TYPES = [
  { value: "sanboot", title: "整盘 ISO", sub: "sanboot 经 HTTP 映射" },
  { value: "kernel_initrd", title: "内核 + initrd", sub: "从 ISO 抽取，可把 ISO 打包进 initrd" },
  { value: "chain", title: "链式脚本", sub: "同源或白名单前缀" },
  { value: "exit", title: "本地启动", sub: "exit" },
] as const;
const ARCHES = [
  { value: "any", label: "全部" },
  { value: "bios", label: "仅 BIOS" },
  { value: "uefi", label: "仅 UEFI" },
];

function PathInput({ label, value, error, paths, onChange }: { label: string; value: string; error: string | undefined; paths: readonly string[]; onChange: (value: string) => void }) {
  const list = `paths-${label}`;
  return (
    <Field label={label} {...(error === undefined ? {} : { error })}>
      <Input
        className="mono"
        list={list}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
      <datalist id={list}>
        {paths.map((path) => (
          <option key={path} value={path} />
        ))}
      </datalist>
    </Field>
  );
}

function KernelFields({ kind, errors, onChange }: { kind: Extract<EntryKind, { type: "kernel_initrd" }>; errors: Record<string, string>; onChange: (kind: EntryKind) => void }) {
  const tree = useIsoTree(kind.iso === "" ? null : kind.iso);
  const paths = (tree.data?.entries ?? []).filter((item) => !item.dir).map((item) => item.path);
  return (
    <>
      <PathInput label="内核路径" value={kind.kernel_path} error={errors["kernel_path"]} paths={paths} onChange={(value) => { onChange({ ...kind, kernel_path: value }); }} />
      <PathInput label="initrd 路径" value={kind.initrd_path} error={errors["initrd_path"]} paths={paths} onChange={(value) => { onChange({ ...kind, initrd_path: value }); }} />
      <Field label="内核命令行" hint="只允许字母、数字、空格与 . _ , : = / + @ % ~ -" {...(errors["cmdline"] === undefined ? {} : { error: errors["cmdline"] })}>
        <Textarea className="mono" rows={3} value={kind.cmdline} onChange={(event) => { onChange({ ...kind, cmdline: event.target.value }); }} />
      </Field>
      <Switch label="把 ISO 打包进 initrd（PVE 安装器需要）" checked={kind.embed_iso} onChange={(checked) => { onChange({ ...kind, embed_iso: checked, iso_name: checked ? kind.iso_name ?? "proxmox.iso" : null }); }} />
      {kind.embed_iso && (
        <>
          <Field label="initrd 内 ISO 文件名">
            <Input className="mono" value={kind.iso_name ?? ""} onChange={(event) => { onChange({ ...kind, iso_name: event.target.value }); }} />
          </Field>
          <Alert title="内存需求">ISO 会整体下载进内存；BIOS 下 iPXE 可用内存有限，建议 UEFI 并给目标机 ≥ 4 GiB 内存。</Alert>
        </>
      )}
    </>
  );
}

function KindFields({ kind, isos, errors, onChange }: { kind: EntryKind; isos: readonly IsoItem[]; errors: Record<string, string>; onChange: (kind: EntryKind) => void }) {
  if (kind.type === "exit") return null;
  if (kind.type === "chain") {
    return (
      <Field label="脚本 URL" hint="同源路径（/ 开头）或管理员配置的白名单前缀" {...(errors["url"] === undefined ? {} : { error: errors["url"] })}>
        <Input className="mono" value={kind.url} onChange={(event) => { onChange({ ...kind, url: event.target.value }); }} />
      </Field>
    );
  }
  const options = isos.map((item) => ({ value: item.iso.sha256, label: `${item.iso.label} · ${item.iso.sha256.slice(0, 12)}` }));
  return (
    <>
      <Field label="ISO" {...(errors["iso"] === undefined ? {} : { error: errors["iso"] })}>
        <Select
          label="ISO"
          hideLabel
          options={options}
          value={kind.iso === "" ? null : kind.iso}
          onChange={(sha) => {
            const detection = isos.find((item) => item.iso.sha256 === sha)?.iso.detection;
            onChange(kind.type === "sanboot" ? { type: "sanboot", iso: sha } : prefill(sha, detection).type === "kernel_initrd" ? prefill(sha, detection) : { ...kind, iso: sha });
          }}
        />
      </Field>
      {kind.type === "kernel_initrd" && <KernelFields kind={kind} errors={errors} onChange={onChange} />}
    </>
  );
}

export function EntryDrawer({ entry, others, isos, onClose, onSave }: { entry: BootMenuEntry | null; others: readonly BootMenuEntry[]; isos: readonly IsoItem[]; onClose: () => void; onSave: (entry: BootMenuEntry) => void }) {
  const [draft, setDraft] = useState<BootMenuEntry | null>(entry);
  const current = draft ?? entry;
  if (current === null) return null;
  const errors = entryErrors(current, others);
  const update = (next: Partial<BootMenuEntry>) => {
    setDraft({ ...current, ...next });
  };
  return (
    <Drawer
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title="编辑菜单条目"
      footer={
        <div className="row-end">
          <Button onClick={onClose}>取消</Button>
          <Button
            variant="primary"
            disabled={Object.keys(errors).length > 0}
            onClick={() => {
              onSave(current);
            }}
          >
            应用
          </Button>
        </div>
      }
    >
      <div className="page-stack">
        <Field label="标题" {...(errors["title"] === undefined ? {} : { error: errors["title"] })}>
          <Input value={current.title} maxLength={64} onChange={(event) => { update({ title: event.target.value }); }} />
        </Field>
        <Field label="ID" hint="菜单标签与审计中使用" {...(errors["id"] === undefined ? {} : { error: errors["id"] })}>
          <Input className="mono" value={current.id} maxLength={32} onChange={(event) => { update({ id: event.target.value }); }} />
        </Field>
        <Field label="固件">
          <Select label="固件" hideLabel options={ARCHES} value={current.arch} onChange={(arch) => { update({ arch: arch as BootMenuEntry["arch"] }); }} />
        </Field>
        <RadioCards label="条目类型" columns={2} options={TYPES} value={current.kind.type} onChange={(type) => { update({ kind: switchKind(current.kind, type as EntryKind["type"]) }); }} />
        <KindFields kind={current.kind} isos={isos} errors={errors} onChange={(kind) => { update({ kind }); }} />
      </div>
    </Drawer>
  );
}
