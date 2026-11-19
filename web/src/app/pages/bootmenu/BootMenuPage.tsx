// 手动引导菜单页（Step 14 §3.5）：总开关、MAC 白名单、默认项、倒计时、条目列表与编辑抽屉、右侧实时预览 menu.ipxe。
// 开启菜单、修改白名单、开启“对所有未知机器显示”都先经 AlertDialog 确认；保存走版本链（配置历史页可看差异并回滚）。
import { useQuery } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router";
import { Button } from "../../../components/ui/button";
import { CodeBlock } from "../../../components/ui/code";
import { MultiCombobox } from "../../../components/ui/combobox";
import { NumberField, Switch } from "../../../components/ui/controls";
import { AlertDialog } from "../../../components/ui/dialog";
import { Alert } from "../../../components/ui/feedback";
import { Field, Input } from "../../../components/ui/field";
import { Card, CardLabel, Section } from "../../../components/ui/layout";
import { Select } from "../../../components/ui/select";
import { api } from "../../../lib/api";
import type { BootMenuDoc, BootMenuEntry } from "../../../lib/types";
import { AsyncView, DefaultSkeleton } from "../../AsyncView";
import { useBootMenuStatus, useIsos, useMe, useNodes, useObservations } from "../../data";
import { SingletonSave, useSingleton } from "../configs/singleton";
import { EntryDrawer } from "./EntryDrawer";
import { EntryList } from "./EntryList";
import { EMPTY_MENU, fixDefault, newEntry, normalize } from "./model";

const MAC_RE = /^[0-9a-f]{2}(:[0-9a-f]{2}){5}$/u;

interface Pending {
  title: string;
  description: string;
  next: BootMenuDoc;
}

function usePreview(doc: BootMenuDoc) {
  const text = JSON.stringify(doc);
  const [settled, setSettled] = useState(text);
  useEffect(() => {
    const timer = setTimeout(() => {
      setSettled(text);
    }, 300);
    return () => {
      clearTimeout(timer);
    };
  }, [text]);
  return useQuery({
    queryKey: ["boot-menu", "preview", settled],
    queryFn: () => api<{ script: string; mac: string }>("/boot-menu/preview", { method: "POST", body: { document: JSON.parse(settled) as unknown } }),
    placeholderData: (previous) => previous,
    retry: false,
  });
}

function MacList({ doc, onAsk }: { doc: BootMenuDoc; onAsk: (pending: Pending) => void }) {
  const nodes = useNodes();
  const seen = useObservations();
  const [extra, setExtra] = useState("");
  const known = new Set([...(nodes.data ?? []).map((node) => node.mac), ...(seen.data ?? []).map((row) => row.mac), ...doc.allow_macs]);
  const candidate = extra.trim().toLowerCase().replaceAll("-", ":");
  const ask = (macs: string[]) => {
    onAsk({ title: "修改 MAC 白名单", description: `白名单内的机器在未武装时会看到手动引导菜单（共 ${String(macs.length)} 个）。`, next: { ...doc, allow_macs: macs } });
  };
  return (
    <div className="page-stack">
      <MultiCombobox label="MAC 白名单" options={[...known].sort().map((mac) => ({ value: mac, label: mac }))} value={doc.allow_macs} onChange={ask} />
      <div className="row-gap">
        <Field label="添加其他 MAC" {...(candidate !== "" && !MAC_RE.test(candidate) ? { error: "格式 aa:bb:cc:dd:ee:ff" } : {})}>
          <Input className="mono" value={extra} onChange={(event) => { setExtra(event.target.value); }} />
        </Field>
        <Button
          disabled={!MAC_RE.test(candidate) || doc.allow_macs.includes(candidate)}
          onClick={() => {
            ask([...doc.allow_macs, candidate]);
            setExtra("");
          }}
        >
          添加
        </Button>
      </div>
    </div>
  );
}

function Editor({ doc, setDoc, admin }: { doc: BootMenuDoc; setDoc: (doc: BootMenuDoc) => void; admin: boolean }) {
  const isos = useIsos();
  const status = useBootMenuStatus();
  const preview = usePreview(doc);
  const [pending, setPending] = useState<Pending | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const items = isos.data?.items ?? [];
  const current = editing === null ? null : (doc.entries[editing] ?? newEntry(doc.entries));
  const saveEntry = (entry: BootMenuEntry) => {
    const entries = editing !== null && editing >= 0 && editing < doc.entries.length ? doc.entries.map((item, at) => (at === editing ? entry : item)) : [...doc.entries, entry];
    setDoc(fixDefault({ ...doc, entries }));
    setEditing(null);
  };
  const enabledEntries = doc.entries.filter((entry) => entry.enabled);
  return (
    <div className="editor-2col">
      <div className="page-stack">
        {!admin && <Alert title="只读">修改手动引导菜单需要 admin 角色。</Alert>}
        <Card>
          <div className="page-stack">
            <CardLabel>入口</CardLabel>
            <Switch
              label="启用手动引导菜单"
              checked={doc.enabled}
              onChange={(enabled) => {
                if (enabled) setPending({ title: "启用手动引导菜单", description: "未武装且命中白名单的机器将看到菜单并可手动装机。", next: { ...doc, enabled } });
                else setDoc({ ...doc, enabled });
              }}
            />
            <Switch
              label="对所有未知机器显示"
              checked={doc.show_unknown}
              onChange={(show) => {
                if (show) setPending({ title: "对所有未知机器显示", description: "任何未登记的机器 PXE 启动时都会看到菜单，请确认网络隔离。", next: { ...doc, show_unknown: show } });
                else setDoc({ ...doc, show_unknown: show });
              }}
            />
            <MacList doc={doc} onAsk={setPending} />
            <Select label="默认条目" options={[{ value: "", label: "无（等待选择）" }, ...enabledEntries.map((entry) => ({ value: entry.id, label: entry.title }))]} value={doc.default_entry ?? ""} onChange={(value) => { setDoc({ ...doc, default_entry: value === "" ? null : value }); }} />
            <NumberField label="倒计时" unit="秒（0 表示一直等待）" min={0} max={3600} value={doc.timeout_s} onChange={(timeout) => { setDoc({ ...doc, timeout_s: timeout }); }} />
          </div>
        </Card>
        <Card>
          <div className="page-stack">
            <div className="row-between">
              <CardLabel>条目</CardLabel>
              <Button onClick={() => { setEditing(-1); }}>
                <Plus size={14} aria-hidden="true" /> 添加条目
              </Button>
            </div>
            <EntryList entries={doc.entries} status={status.data ?? null} defaultEntry={doc.default_entry} onChange={(entries) => { setDoc(fixDefault({ ...doc, entries })); }} onEdit={setEditing} />
          </div>
        </Card>
      </div>
      <Card className="sticky-preview">
        <div className="page-stack">
          <CardLabel>menu.ipxe 预览{preview.data === undefined ? "" : ` · ${preview.data.mac}`}</CardLabel>
          {preview.isError ? <Alert tone="danger" title="无法渲染">{preview.error.message}</Alert> : <CodeBlock code={preview.data?.script ?? "…"} label="menu.ipxe 预览" />}
        </div>
      </Card>
      {current !== null && <EntryDrawer key={editing} entry={current} others={doc.entries.filter((_, at) => at !== editing)} isos={items} onClose={() => { setEditing(null); }} onSave={saveEntry} />}
      <AlertDialog
        open={pending !== null}
        onOpenChange={(open) => {
          if (!open) setPending(null);
        }}
        title={pending?.title ?? ""}
        description={pending?.description}
        confirmLabel="确认"
        onConfirm={() => {
          if (pending !== null) setDoc(pending.next);
          setPending(null);
        }}
      >
        <p className="muted-text">确认后仍需点击“保存”生效；保存写入配置历史与审计。</p>
      </AlertDialog>
    </div>
  );
}

export function BootMenuPage() {
  const state = useSingleton("boot_menu");
  const me = useMe();
  const admin = me.data?.role === "admin";
  const [creating, setCreating] = useState(false);
  const doc = state.doc === null ? (creating ? EMPTY_MENU : null) : normalize(state.doc);
  return (
    <Section
      title="手动引导菜单"
      description="未武装的机器经 PXE 进入菜单后手动选择 ISO 装机；默认关闭。"
      actions={
        <span className="row-gap">
          <Link to="/history" className="muted-text">
            版本历史
          </Link>
          {admin && doc !== null && <SingletonSave kind="boot_menu" state={state} fallback={EMPTY_MENU as unknown as Record<string, unknown>} />}
        </span>
      }
    >
      <AsyncView
        query={state.list}
        isEmpty={(objects) => objects.length === 0 && !creating}
        empty={{
          title: "尚未配置手动引导菜单",
          description: "添加条目并开启后，白名单内未武装的机器 PXE 启动会看到菜单。",
          ...(admin
            ? {
                action: (
                  <Button
                    variant="primary"
                    onClick={() => {
                      setCreating(true);
                      state.setDoc(EMPTY_MENU as unknown as Record<string, unknown>);
                    }}
                  >
                    开始配置
                  </Button>
                ),
              }
            : {}),
        }}
      >
        {() =>
          doc === null ? (
            <DefaultSkeleton />
          ) : (
            <Editor
              doc={doc}
              admin={admin}
              setDoc={(next) => {
                state.setDoc(next as unknown as Record<string, unknown>);
              }}
            />
          )
        }
      </AsyncView>
    </Section>
  );
}
