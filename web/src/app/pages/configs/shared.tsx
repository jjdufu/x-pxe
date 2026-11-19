// 配置编辑器共用：草稿（只在首次加载/切换对象/载入新版本时初始化）、300 ms 去抖预览、字段错误、保存栏（差异 + 必填说明 + 409 处理）。
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Button } from "../../../components/ui/button";
import { DiffView } from "../../../components/ui/code";
import { Dialog } from "../../../components/ui/dialog";
import { Alert, Badge } from "../../../components/ui/feedback";
import { Field, Textarea } from "../../../components/ui/field";
import { notify } from "../../../components/ui/toast";
import { api, ApiError } from "../../../lib/api";
import type { ConfigHead, FieldIssue, PreviewResult } from "../../../lib/types";
import { errorText } from "../../data";

export type Doc = Record<string, unknown>;

/** 草稿状态：基准版本 + 草稿；后台出现新头版本时只提示，不覆盖。 */
export function useDraft(head: ConfigHead<Doc> | undefined) {
  const [state, setState] = useState<{ objectId: number; base: number; doc: Doc; dirty: boolean } | null>(null);
  if (head !== undefined && (state === null || state.objectId !== head.version.object_id)) {
    setState({ objectId: head.version.object_id, base: head.version.version, doc: head.document, dirty: false });
  }
  const latest = head?.version.version ?? 0;
  return {
    doc: state?.doc ?? null,
    base: state?.base ?? 0,
    dirty: state?.dirty ?? false,
    stale: state !== null && latest > state.base,
    setDoc: (doc: Doc) => {
      if (state !== null) setState({ ...state, doc, dirty: true });
    },
    /** 载入服务器最新版本（丢弃草稿）。 */
    reload: () => {
      if (head !== undefined) setState({ objectId: head.version.object_id, base: head.version.version, doc: head.document, dirty: false });
    },
    /** 保存成功后：草稿即新基准。 */
    saved: (version: number) => {
      if (state !== null) setState({ ...state, base: version, dirty: false });
    },
    /** 基于最新版本重做：保留草稿，把基准移到最新头版本。 */
    rebase: () => {
      if (state !== null) setState({ ...state, base: latest });
    },
  };
}

/** 草稿变化 300 ms 后请求预览；结果更新时不重建预览区（保留滚动位置）。 */
export function usePreview(kind: string, body: Record<string, unknown> | null) {
  const [result, setResult] = useState<PreviewResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const serial = JSON.stringify(body);
  const counter = useRef(0);
  useEffect(() => {
    if (serial === "null") return undefined;
    const timer = window.setTimeout(() => {
      counter.current += 1;
      const ticket = counter.current;
      setLoading(true);
      api<PreviewResult>(`/configs/${kind}/preview`, { method: "POST", body: JSON.parse(serial) as unknown })
        .then((reply) => {
          if (ticket !== counter.current) return;
          setResult(reply);
          setError(null);
        })
        .catch((cause: unknown) => {
          if (ticket === counter.current) setError(errorText(cause));
        })
        .finally(() => {
          if (ticket === counter.current) setLoading(false);
        });
    }, 300);
    return () => {
      window.clearTimeout(timer);
    };
  }, [kind, serial]);
  return { result, error, loading };
}

/** 字段路径 → 错误文本（取第一条）。 */
export function issueMap(issues: readonly FieldIssue[] | undefined): Record<string, string> {
  const out: Record<string, string> = {};
  for (const issue of issues ?? []) {
    const path = issue.path.replace(/^\/document/u, "");
    out[path] ??= issue.message;
  }
  return out;
}

/** 顶部汇总：点击跳到对应字段。 */
export function IssuesAlert({ issues, title = "校验未通过" }: { issues: readonly FieldIssue[]; title?: string }) {
  if (issues.length === 0) return null;
  return (
    <Alert tone="warn" title={`${title}（${String(issues.length)} 项）`}>
      <ul className="list-plain">
        {issues.map((issue) => (
          <li key={`${issue.path}-${issue.code}`}>
            <button
              type="button"
              className="ui-btn ui-btn-ghost ui-btn-sm"
              onClick={() => {
                const path = issue.path.replace(/^\/document/u, "");
                const target = document.querySelector<HTMLElement>(`[data-path="${path}"] input, [data-path="${path}"] textarea, [data-path="${path}"] button`);
                target?.focus();
              }}
            >
              <span className="mono">{issue.path}</span>
            </button>
            {issue.message}
          </li>
        ))}
      </ul>
    </Alert>
  );
}

export function StaleBanner({ show, onReload }: { show: boolean; onReload: () => void }) {
  if (!show) return null;
  return (
    <Alert
      tone="info"
      title="服务器上有新版本"
      action={
        <Button size="sm" variant="outline" onClick={onReload}>
          载入新版本
        </Button>
      }
    >
      其他人保存了新版本；当前草稿未被覆盖。
    </Alert>
  );
}

/** 差异内容：字段差异 + 渲染差异。 */
export function DiffContent({ preview }: { preview: PreviewResult | null }) {
  const changes = preview?.changes ?? [];
  const rendered = preview?.rendered_diff ?? [];
  if (changes.length === 0 && rendered.length === 0) return <p className="muted-text">与头版本没有差异</p>;
  return (
    <div className="page-stack" data-testid="diff-content">
      {changes.length > 0 && (
        <DiffView
          label="字段差异"
          lines={changes.flatMap((change) => [
            { kind: "del" as const, text: `${change.path}: ${change.before}` },
            { kind: "add" as const, text: `${change.path}: ${change.after}` },
          ])}
        />
      )}
      {rendered.map((item) => (
        <div key={item.name} className="page-stack">
          <strong className="mono">{item.name}</strong>
          <DiffView label={`${item.name} 渲染差异`} lines={item.lines.filter((line) => line.kind !== "same")} />
        </div>
      ))}
    </div>
  );
}

export interface SaveBarProps {
  kind: string;
  objectId: number;
  draft: ReturnType<typeof useDraft>;
  /** 带基准的预览结果（差异来源）。 */
  diffPreview: PreviewResult | null;
  valid: boolean;
}

/** 底部保存栏：头版本、未保存徽章、差异对话框、保存对话框（必填说明）、409 冲突处理。 */
export function SaveBar({ kind, objectId, draft, diffPreview, valid }: SaveBarProps) {
  const [diffOpen, setDiffOpen] = useState(false);
  const [saveOpen, setSaveOpen] = useState(false);
  const [comment, setComment] = useState("");
  const [conflict, setConflict] = useState(false);
  const [busy, setBusy] = useState(false);
  const client = useQueryClient();
  const save = async () => {
    setBusy(true);
    try {
      const reply = await api<{ version: number }>(`/configs/${kind}/${String(objectId)}`, {
        method: "PUT",
        body: { expected_head: draft.base, document: draft.doc, comment },
      });
      notify(`已保存为 v${String(reply.version)}`, "success");
      draft.saved(reply.version);
      setSaveOpen(false);
      setComment("");
      setConflict(false);
      void client.invalidateQueries({ queryKey: ["configs", kind] });
      void client.invalidateQueries({ queryKey: ["history"] });
    } catch (error) {
      if (error instanceof ApiError && error.status === 409) {
        setConflict(true);
        setSaveOpen(false);
        void client.invalidateQueries({ queryKey: ["configs", kind, objectId] });
      }
      notify("保存失败", "danger", errorText(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      {conflict && (
        <Alert
          tone="warn"
          title="版本冲突"
          action={
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                draft.rebase();
                setConflict(false);
                setDiffOpen(true);
              }}
            >
              基于最新版本重做
            </Button>
          }
        >
          他人已保存了新版本。查看对方修改的差异后，可基于最新版本重新保存当前草稿。
        </Alert>
      )}
      <div className="save-bar">
        <div className="row-gap">
          <span className="muted-text tabular">头版本 v{draft.base}</span>
          {draft.dirty && <Badge tone="warn">未保存</Badge>}
        </div>
        <div className="row-gap">
          <Button
            variant="outline"
            onClick={() => {
              setDiffOpen(true);
            }}
          >
            差异
          </Button>
          <Button
            variant="primary"
            disabled={!draft.dirty || !valid}
            onClick={() => {
              setSaveOpen(true);
            }}
          >
            保存
          </Button>
        </div>
      </div>
      <Dialog open={diffOpen} onOpenChange={setDiffOpen} title="与头版本的差异">
        <DiffContent preview={diffPreview} />
      </Dialog>
      <Dialog
        open={saveOpen}
        onOpenChange={setSaveOpen}
        title="保存新版本"
        footer={
          <>
            <Button
              variant="outline"
              onClick={() => {
                setSaveOpen(false);
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
    </>
  );
}

/** 读写嵌套字段。 */
export function getPath(doc: unknown, path: readonly string[]): unknown {
  let current: unknown = doc;
  for (const key of path) {
    if (typeof current !== "object" || current === null) return undefined;
    current = (current as Record<string, unknown>)[key];
  }
  return current;
}

export function setPath(doc: Doc, path: readonly string[], value: unknown): Doc {
  const [head, ...rest] = path;
  if (head === undefined) return doc;
  if (rest.length === 0) return { ...doc, [head]: value };
  const child = doc[head];
  const next = typeof child === "object" && child !== null && !Array.isArray(child) ? (child as Doc) : {};
  return { ...doc, [head]: setPath(next, rest, value) };
}

export const lines = (value: unknown): string => (Array.isArray(value) ? value.map(String).join("\n") : "");
export const splitLines = (text: string): string[] =>
  text
    .split(/\n/u)
    .map((item) => item.trim())
    .filter((item) => item !== "");
export const splitList = (text: string): string[] => text.split(/[\s,]+/u).filter((item) => item !== "");
