// 右侧预览：产物 Tabs；更新文本不重建节点，保留各 Tab 滚动位置。
import { useState } from "react";
import { CodeBlock } from "../../../components/ui/code";
import { Alert, Badge } from "../../../components/ui/feedback";
import { Tabs } from "../../../components/ui/tabs";
import type { Artifact } from "../../../lib/types";

export function PreviewPane({ artifacts, error, loading, label }: { artifacts: readonly Artifact[]; error: string | null; loading: boolean; label: string }) {
  const [tab, setTab] = useState("");
  const current = artifacts.some((item) => item.name === tab) ? tab : (artifacts[0]?.name ?? "");
  return (
    <div className="page-stack" data-testid="preview-pane">
      <div className="row-gap">
        <strong>{label}</strong>
        {loading && <Badge tone="outline">渲染中</Badge>}
      </div>
      {error !== null && <Alert tone="warn" title="预览失败">{error}</Alert>}
      {artifacts.length > 0 ? (
        <Tabs
          label={label}
          value={current}
          onChange={setTab}
          items={artifacts.map((item) => ({ value: item.name, label: item.name, content: <CodeBlock code={item.text} label={item.name} /> }))}
        />
      ) : (
        error === null && <p className="muted-text">修正校验错误后显示渲染结果</p>
      )}
    </div>
  );
}
