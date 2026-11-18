// 参考实现：.code-block、.code-wrap/.code-copy-btn（app.js enhanceCodeBlocks）、.diff-line.diff-bad/.diff-good、app.js copyText。
import { Check, Copy } from "lucide-react";
import { useState } from "react";
import { copyText } from "../../lib/clipboard";
import { cx } from "../../lib/cx";
import { notify } from "./toast";

export function CopyButton({ value, label = "复制", className }: { value: string; label?: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={cx("code-copy-btn", className)}
      aria-label={label}
      onClick={() => {
        void copyText(value).then((result) => {
          if (result === "failed") {
            notify("复制失败", "danger", "当前浏览器不允许访问剪贴板，请手动选中复制");
            return;
          }
          setCopied(true);
          notify("已复制", "success");
          window.setTimeout(() => {
            setCopied(false);
          }, 1500);
        });
      }}
    >
      {copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}
      <span>{copied ? "已复制" : label}</span>
    </button>
  );
}

export function CodeBlock({ code, label }: { code: string; label: string }) {
  return (
    <div className="code-wrap">
      <pre className="code-block" tabIndex={0} aria-label={label}>
        <code>{code}</code>
      </pre>
      <CopyButton value={code} />
    </div>
  );
}

export interface DiffLine {
  kind: "same" | "add" | "del";
  text: string;
}

export function DiffView({ lines, label }: { lines: readonly DiffLine[]; label: string }) {
  return (
    <div className="diff-view" role="group" aria-label={label}>
      {lines.map((line, index) => (
        // 差异行顺序固定，序号即稳定键
        <div key={`${String(index)}-${line.kind}`} className={cx("diff-line", line.kind === "add" && "diff-good", line.kind === "del" && "diff-bad")}>
          <span className="diff-sign" aria-hidden="true">
            {line.kind === "add" ? "+" : line.kind === "del" ? "−" : " "}
          </span>
          <code>{line.text}</code>
        </div>
      ))}
    </div>
  );
}
