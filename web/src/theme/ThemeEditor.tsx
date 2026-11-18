// Theme Editor（step-11 第 4 节）：右侧 Drawer（参考 #demoDrawer），控件取自参考实现 .tabs-bar/.tab-btn、.radio-card-grid、
// Park 区块 data-park-radius / data-park-accent 按钮组与 .token-swatch、.ui-range；Token 条参考 .token-bar/.token-chip。
import { ColorPicker, parseColor } from "@ark-ui/react/color-picker";
import { Download, RotateCcw } from "lucide-react";
import { useMemo, useState } from "react";
import { Button } from "../components/ui/button";
import { CodeBlock } from "../components/ui/code";
import { Switch, RadioCards, Slider } from "../components/ui/controls";
import { Drawer } from "../components/ui/dialog";
import { Alert, Badge } from "../components/ui/feedback";
import { Field, Input } from "../components/ui/field";
import { FIXED, Layer } from "../components/ui/layer";
import { Select } from "../components/ui/select";
import { Segmented } from "../components/ui/tabs";
import { cx } from "../lib/cx";
import { contrast } from "./color";
import { ACCENT_LABELS, ACCENT_PRESETS, GRAYS, GRAY_LABELS, GRAY_TOKENS, RADIUS_STEPS, type Gray, type ModeChoice } from "./presets";
import { FG_MIN, RING_MIN, resolveAccent } from "./resolve-accent";
import { DEFAULT_THEME, exportTheme } from "./state";
import { useTheme } from "./ThemeProvider";

const MODE_OPTIONS = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "System" },
];

function Ratio({ label, value, min }: { label: string; value: number; min: number }) {
  const pass = value >= min;
  return (
    <span className={cx("token-chip tabular", !pass && "token-chip-fail")}>
      {label} <strong>{value.toFixed(2)}</strong>
      <span className="sr-only">{pass ? "达标" : "不达标"}</span>
    </span>
  );
}

function AccentPicker({ hex, onChange }: { hex: string; onChange: (hex: string) => void }) {
  return (
    <ColorPicker.Root
      className="color-picker"
      value={parseColor(hex)}
      positioning={FIXED}
      onValueChangeEnd={(details) => {
        onChange(details.value.toString("hex").slice(0, 7).toLowerCase());
      }}
    >
      <ColorPicker.Label className="ui-label">自定义颜色</ColorPicker.Label>
      <ColorPicker.Control className="row-gap">
        <ColorPicker.ChannelInput channel="hex" className="ui-input color-hex mono" aria-label="自定义颜色十六进制值" />
        <ColorPicker.Trigger className="icon-btn" aria-label="打开取色器">
          <ColorPicker.ValueSwatch className="token-swatch" />
        </ColorPicker.Trigger>
      </ColorPicker.Control>
      <Layer>
        <ColorPicker.Positioner>
          <ColorPicker.Content className="popover-panel color-panel">
            <ColorPicker.Area className="color-area">
              <ColorPicker.AreaBackground className="color-area-bg" />
              <ColorPicker.AreaThumb className="color-thumb" />
            </ColorPicker.Area>
            <ColorPicker.ChannelSlider channel="hue" className="color-hue">
              <ColorPicker.ChannelSliderTrack className="color-hue-track" />
              <ColorPicker.ChannelSliderThumb className="color-thumb" />
            </ColorPicker.ChannelSlider>
          </ColorPicker.Content>
        </ColorPicker.Positioner>
      </Layer>
      <ColorPicker.HiddenInput />
    </ColorPicker.Root>
  );
}

function Preview() {
  const [choice, setChoice] = useState<string | null>("uefi");
  const [on, setOn] = useState(true);
  return (
    <div className="theme-preview ui-card">
      <div className="row-gap">
        <Button variant="primary">主按钮</Button>
        <Button variant="outline">描边按钮</Button>
        <Badge tone="success" dot>
          Ready
        </Badge>
        <Badge tone="danger">Failed</Badge>
      </div>
      <Field label="主机名">
        <Input defaultValue="node-01" />
      </Field>
      <Select
        label="引导方式"
        options={[
          { value: "uefi", label: "UEFI PXE" },
          { value: "bios", label: "BIOS PXE" },
          { value: "http", label: "UEFI HTTP Boot" },
        ]}
        value={choice}
        onChange={setChoice}
      />
      <Switch checked={on} onChange={setOn} label="启用模块" />
      <Alert tone="info" title="提示">
        修改实时生效并保存在本浏览器。
      </Alert>
      <div className="table-wrap">
        <table className="ui-table">
          <thead>
            <tr>
              <th>MAC</th>
              <th>状态</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td className="mono tabular">52:54:00:12:34:56</td>
              <td>Installing</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  );
}

function download(name: string, text: string, type: string) {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = name;
  anchor.click();
  URL.revokeObjectURL(url);
}

export function ThemeEditor() {
  const { prefs, mode, resolved, setPrefs, editorOpen, setEditorOpen } = useTheme();
  const tokens = GRAY_TOKENS[prefs.gray][mode];
  const exported = useMemo(() => exportTheme(prefs, resolved), [prefs, resolved]);
  const customHex = prefs.accent.kind === "custom" ? prefs.accent.hex : "#2563eb";

  return (
    <Drawer
      open={editorOpen}
      onOpenChange={setEditorOpen}
      title="主题"
      description="颜色模式、灰阶、圆角与 Accent。只保存在本浏览器。"
      footer={
        <>
          <Button
            variant="ghost"
            onClick={() => {
              setPrefs(DEFAULT_THEME);
            }}
          >
            <RotateCcw size={14} aria-hidden="true" />
            重置
          </Button>
          <Button
            variant="outline"
            onClick={() => {
              download("xpxe-theme.json", exported.json, "application/json");
            }}
          >
            <Download size={14} aria-hidden="true" />
            导出主题配置
          </Button>
        </>
      }
    >
      <div className="theme-editor">
        <div className="form-field">
          <span className="ui-label" id="theme-mode-label">
            颜色模式
          </span>
          <Segmented
            label="颜色模式"
            options={MODE_OPTIONS}
            value={prefs.mode}
            onChange={(value) => {
              setPrefs({ ...prefs, mode: value as ModeChoice });
            }}
          />
        </div>

        <RadioCards
          label="灰阶"
          columns={2}
          value={prefs.gray}
          onChange={(value) => {
            setPrefs({ ...prefs, gray: value as Gray });
          }}
          options={GRAYS.map((gray) => ({
            value: gray,
            title: GRAY_LABELS[gray],
            sub: GRAY_TOKENS[gray][mode].text,
            preview: (
              <span className="gray-preview" aria-hidden="true">
                {(["bg", "surface2", "text"] as const).map((key) => (
                  <span key={key} className="token-swatch" style={{ background: GRAY_TOKENS[gray][mode][key] }} />
                ))}
              </span>
            ),
          }))}
        />

        <div className="form-field">
          <span className="ui-label">圆角</span>
          <div className="toggle-group radius-steps" role="group" aria-label="圆角阶梯">
            {RADIUS_STEPS.map((step) => (
              <button
                key={step.label}
                type="button"
                className="toggle-btn"
                aria-pressed={prefs.radius === step.value}
                data-state={prefs.radius === step.value ? "on" : "off"}
                onClick={() => {
                  setPrefs({ ...prefs, radius: step.value });
                }}
              >
                {step.label}
              </button>
            ))}
          </div>
          <Slider
            label="圆角（px）"
            min={0}
            max={99}
            value={[prefs.radius]}
            format={(value) => `${String(value)} px`}
            onChange={(value) => {
              const next = value[0];
              if (next !== undefined) setPrefs({ ...prefs, radius: next });
            }}
          />
        </div>

        <div className="form-field">
          <span className="ui-label">Accent</span>
          <div className="accent-swatches" role="group" aria-label="Accent 预设">
            {ACCENT_PRESETS.map((name) => {
              const swatch = resolveAccent({ kind: "preset", name }, mode, prefs.gray);
              const active = prefs.accent.kind === "preset" && prefs.accent.name === name;
              return (
                <button
                  key={name}
                  type="button"
                  className={cx("accent-swatch", active && "active")}
                  aria-pressed={active}
                  onClick={() => {
                    setPrefs({ ...prefs, accent: { kind: "preset", name } });
                  }}
                >
                  <span className="token-swatch" style={{ background: swatch.accent }} aria-hidden="true" />
                  {ACCENT_LABELS[name]}
                </button>
              );
            })}
          </div>
          <AccentPicker
            hex={customHex}
            onChange={(hex) => {
              setPrefs({ ...prefs, accent: { kind: "custom", hex } });
            }}
          />
          {resolved.adjusted && (
            <Alert tone="warn" title="已自动加深以满足对比度">
              Accent 已调整为 <span className="mono">{resolved.accent}</span>。
            </Alert>
          )}
        </div>

        <div className="form-field">
          <span className="ui-label">实时预览</span>
          <Preview />
        </div>

        <div className="token-bar" aria-label="当前 Token 与对比度">
          {(
            [
              ["--bg", tokens.bg],
              ["--surface", tokens.surface],
              ["--text", tokens.text],
              ["--text-muted", tokens.textMuted],
              ["--border-input", tokens.borderInput],
              ["--accent", resolved.accent],
              ["--accent-fg", resolved.accentFg],
              ["--radius-md", `${String(prefs.radius)}px`],
            ] as const
          ).map(([name, value]) => (
            <span key={name} className="token-chip mono">
              {name}: {value}
            </span>
          ))}
          <Ratio label="text/bg" value={contrast(tokens.text, tokens.bg)} min={FG_MIN} />
          <Ratio label="muted/surface-2" value={contrast(tokens.textMuted, tokens.surface2)} min={FG_MIN} />
          <Ratio label="accent-fg/accent" value={contrast(resolved.accentFg, resolved.accent)} min={FG_MIN} />
          <Ratio label="accent/bg" value={contrast(resolved.accent, tokens.bg)} min={RING_MIN} />
          <Ratio label="border-input/bg" value={contrast(tokens.borderInput, tokens.bg)} min={3} />
        </div>

        <div className="form-field">
          <span className="ui-label">导出（Tailwind v4）</span>
          <CodeBlock label="主题 JSON" code={exported.json} />
          <CodeBlock label="CSS 变量片段" code={exported.css} />
        </div>
      </div>
    </Drawer>
  );
}
