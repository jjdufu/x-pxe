// 由默认值的值类型推导的通用设置表单（布尔→Switch、数字→NumberField、字符串→Input、字符串数组→每行一项、其余→JSON）。
import { useState } from "react";
import { NumberField, Switch } from "../../../components/ui/controls";
import { Badge } from "../../../components/ui/feedback";
import { Field, Textarea } from "../../../components/ui/field";
import { LinesInput, TextInput } from "./inputs";

type Value = unknown;

function JsonValue({ label, value, onChange, path, error }: { label: string; value: Value; onChange: (value: Value) => void; path: string; error?: string | undefined }) {
  const [text, setText] = useState(() => JSON.stringify(value, null, 2));
  const [bad, setBad] = useState(false);
  return (
    <div data-path={path}>
      <Field label={label} error={bad ? "JSON 格式错误" : error} hint="JSON">
        <Textarea
          className="mono"
          rows={4}
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            try {
              onChange(JSON.parse(event.target.value) as unknown);
              setBad(false);
            } catch {
              setBad(true);
            }
          }}
        />
      </Field>
    </div>
  );
}

const isStringArray = (value: Value): value is string[] => Array.isArray(value) && value.every((item) => typeof item === "string");
const isRecord = (value: Value): value is Record<string, Value> => typeof value === "object" && value !== null && !Array.isArray(value);

export function SchemaForm({ value, sample, onChange, path, errors, depth = 0 }: { value: Value; sample: Value; onChange: (value: Value) => void; path: string; errors: Record<string, string>; depth?: number }) {
  const shape = sample ?? value;
  if (!isRecord(shape) || !isRecord(value)) return <JsonValue label="设置" value={value} onChange={onChange} path={path} error={errors[path]} />;
  const tag = typeof value.type === "string" ? value.type : null;
  return (
    <div className={depth > 0 ? "schema-group" : "page-stack"}>
      {tag !== null && (
        <div className="row-gap">
          <span className="muted-text">类型</span>
          <Badge>{tag}</Badge>
        </div>
      )}
      {Object.keys({ ...shape, ...value })
        .filter((key) => key !== "type" || tag === null)
        .map((key) => {
          const current = value[key];
          const hint = shape[key] ?? current;
          const child = `${path}/${key}`;
          const set = (next: Value) => {
            onChange({ ...value, [key]: next });
          };
          if (typeof hint === "boolean") return <Switch key={key} label={key} checked={current === true} onChange={set} />;
          if (typeof hint === "number")
            return (
              <div key={key} data-path={child}>
                <NumberField label={key} min={0} max={Number.MAX_SAFE_INTEGER} value={typeof current === "number" ? current : hint} onChange={set} />
              </div>
            );
          if (typeof hint === "string") return <TextInput key={key} label={key} path={child} value={typeof current === "string" ? current : ""} onChange={set} error={errors[child]} />;
          if (isStringArray(hint) && (current === undefined || isStringArray(current)))
            return <LinesInput key={key} label={key} path={child} mono value={current ?? []} onChange={set} error={errors[child]} />;
          if (isRecord(hint) && isRecord(current) && typeof current.type !== "string" && depth < 2)
            return (
              <fieldset key={key} className="schema-fieldset" data-path={child}>
                <legend>{key}</legend>
                <SchemaForm value={current} sample={hint} onChange={set} path={child} errors={errors} depth={depth + 1} />
              </fieldset>
            );
          return <JsonValue key={key} label={key} value={current ?? null} onChange={set} path={child} error={errors[child]} />;
        })}
    </div>
  );
}
