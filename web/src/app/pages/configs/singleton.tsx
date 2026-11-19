// 单例配置（boot_network / dhcp_rules / timeouts）：取第一个对象的头版本；保存时有则 PUT、无则 POST。
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "../../../components/ui/button";
import { Dialog } from "../../../components/ui/dialog";
import { Badge } from "../../../components/ui/feedback";
import { Field, Textarea } from "../../../components/ui/field";
import { notify } from "../../../components/ui/toast";
import { api } from "../../../lib/api";
import { errorText, useConfigHead, useConfigs } from "../../data";
import type { Doc } from "./shared";

export function useSingleton(kind: string) {
  const list = useConfigs(kind);
  const first = list.data?.[0];
  const head = useConfigHead<Doc>(kind, first?.id ?? null);
  const [draft, setDraft] = useState<{ key: string; doc: Doc } | null>(null);
  const key = `${String(first?.id ?? 0)}:${String(head.data?.version.version ?? 0)}`;
  const stored = head.data?.document ?? null;
  const doc = draft !== null && draft.key === key ? draft.doc : stored;
  return {
    list,
    object: first ?? null,
    head,
    doc,
    dirty: draft !== null && draft.key === key,
    setDoc: (next: Doc) => {
      setDraft({ key, doc: next });
    },
    reset: () => {
      setDraft(null);
    },
    expected: head.data?.version.version ?? 0,
  };
}

/** 保存按钮 + 说明对话框。 */
export function SingletonSave({ kind, state, name = "default", fallback }: { kind: string; state: ReturnType<typeof useSingleton>; name?: string; fallback?: Doc }) {
  const [open, setOpen] = useState(false);
  const [comment, setComment] = useState("");
  const [busy, setBusy] = useState(false);
  const client = useQueryClient();
  const save = async () => {
    const document = state.doc ?? fallback;
    if (document === undefined) return;
    setBusy(true);
    try {
      if (state.object === null) {
        await api(`/configs/${kind}`, { method: "POST", body: { name, document, comment } });
      } else {
        await api(`/configs/${kind}/${String(state.object.id)}`, { method: "PUT", body: { expected_head: state.expected, document, comment } });
      }
      notify("已保存", "success");
      setOpen(false);
      setComment("");
      await client.invalidateQueries({ queryKey: ["configs", kind] });
      await client.invalidateQueries({ queryKey: ["bootnet"] });
      void client.invalidateQueries({ queryKey: ["boot-menu"] });
      void client.invalidateQueries({ queryKey: ["history"] });
      state.reset();
    } catch (error) {
      notify("保存失败", "danger", errorText(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="row-gap">
      {state.object !== null && <span className="muted-text tabular">v{state.expected}</span>}
      {state.dirty && <Badge tone="warn">未保存</Badge>}
      <Button
        variant="primary"
        disabled={!state.dirty && state.object !== null}
        onClick={() => {
          setOpen(true);
        }}
      >
        保存
      </Button>
      <Dialog
        open={open}
        onOpenChange={setOpen}
        title="保存配置"
        footer={
          <>
            <Button
              variant="outline"
              onClick={() => {
                setOpen(false);
              }}
            >
              取消
            </Button>
            <Button
              variant="primary"
              disabled={busy || comment.trim() === ""}
              onClick={() => {
                void save();
              }}
            >
              保存
            </Button>
          </>
        }
      >
        <Field label="变更说明" required>
          <Textarea
            value={comment}
            onChange={(event) => {
              setComment(event.target.value);
            }}
          />
        </Field>
      </Dialog>
    </div>
  );
}
