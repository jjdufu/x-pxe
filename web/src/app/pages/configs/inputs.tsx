// 列表型输入：保留用户正在输入的原文（如结尾逗号），每次变化都把解析结果交给上层（预览实时更新）。
import { useState } from "react";
import { Field, Input, Textarea } from "../../../components/ui/field";
import { splitLines, splitList } from "./shared";

function useListText(value: readonly string[], split: (text: string) => string[], join: string) {
  const [text, setText] = useState(value.join(join));
  const parsed = split(text);
  const same = parsed.length === value.length && parsed.every((item, index) => item === value[index]);
  if (!same) {
    // 外部值变化（载入新版本、恢复预设）时以外部为准
    const fresh = value.join(join);
    if (fresh !== text) setText(fresh);
  }
  return [text, setText] as const;
}

export function ListInput({ label, value, onChange, path, error, hint }: { label: string; value: readonly string[]; onChange: (value: string[]) => void; path: string; error?: string | undefined; hint?: string }) {
  const [text, setText] = useListText(value, splitList, ", ");
  return (
    <div data-path={path}>
      <Field label={label} error={error} hint={hint ?? "多个值用逗号或空格分隔"}>
        <Input
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            onChange(splitList(event.target.value));
          }}
        />
      </Field>
    </div>
  );
}

export function LinesInput({ label, value, onChange, path, error, hint, mono = false }: { label: string; value: readonly string[]; onChange: (value: string[]) => void; path: string; error?: string | undefined; hint?: string; mono?: boolean }) {
  const [text, setText] = useListText(value, splitLines, "\n");
  return (
    <div data-path={path}>
      <Field label={label} error={error} hint={hint ?? "每行一项"}>
        <Textarea
          className={mono ? "mono" : undefined}
          rows={Math.min(8, Math.max(3, value.length + 1))}
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            onChange(splitLines(event.target.value));
          }}
        />
      </Field>
    </div>
  );
}

export function TextInput({ label, value, onChange, path, error, hint, placeholder }: { label: string; value: string; onChange: (value: string) => void; path: string; error?: string | undefined; hint?: string; placeholder?: string }) {
  return (
    <div data-path={path}>
      <Field label={label} error={error} {...(hint === undefined ? {} : { hint })}>
        <Input
          value={value}
          placeholder={placeholder}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
      </Field>
    </div>
  );
}
