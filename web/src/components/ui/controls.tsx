// 参考实现：.ui-switch[role=switch]、.ui-checkbox/.checkbox-row（app.js syncIndeterminate）、.radio-card-grid/.radio-card(.selected)、
// components.js“数值、区间…”的 stepper、.ui-range、.dual-range-wrap/-track/-fill。
import { Checkbox as ArkCheckbox } from "@ark-ui/react/checkbox";
import { NumberInput } from "@ark-ui/react/number-input";
import { RadioGroup } from "@ark-ui/react/radio-group";
import { Slider as ArkSlider } from "@ark-ui/react/slider";
import { Switch as ArkSwitch } from "@ark-ui/react/switch";
import { Check, Minus, Plus } from "lucide-react";
import type { ReactNode } from "react";

export function Switch({ checked, onChange, label, disabled = false }: { checked: boolean; onChange: (checked: boolean) => void; label: ReactNode; disabled?: boolean }) {
  return (
    <ArkSwitch.Root
      className="switch-row"
      checked={checked}
      disabled={disabled}
      onCheckedChange={(details) => {
        onChange(details.checked);
      }}
    >
      <ArkSwitch.Control className="ui-switch">
        <ArkSwitch.Thumb className="ui-switch-thumb" />
      </ArkSwitch.Control>
      <ArkSwitch.Label className="switch-label">{label}</ArkSwitch.Label>
      <ArkSwitch.HiddenInput />
    </ArkSwitch.Root>
  );
}

export type CheckState = boolean | "indeterminate";

export function Checkbox({ checked, onChange, label, hideLabel = false, value }: { checked: CheckState; onChange: (checked: boolean) => void; label: string; hideLabel?: boolean; value?: string }) {
  return (
    <ArkCheckbox.Root
      className="checkbox-row"
      checked={checked}
      {...(value === undefined ? {} : { value })}
      onCheckedChange={(details) => {
        onChange(details.checked === true);
      }}
    >
      <ArkCheckbox.Control className="ui-checkbox">
        <ArkCheckbox.Indicator>
          <Check size={12} strokeWidth={3} aria-hidden="true" />
        </ArkCheckbox.Indicator>
        <ArkCheckbox.Indicator indeterminate>
          <Minus size={12} strokeWidth={3} aria-hidden="true" />
        </ArkCheckbox.Indicator>
      </ArkCheckbox.Control>
      <ArkCheckbox.Label className={hideLabel ? "sr-only" : "checkbox-label"}>{label}</ArkCheckbox.Label>
      <ArkCheckbox.HiddenInput />
    </ArkCheckbox.Root>
  );
}

export interface RadioCardOption {
  value: string;
  title: string;
  sub?: string;
  preview?: ReactNode;
}

export function RadioCards({ options, value, onChange, label, columns = 3 }: { options: readonly RadioCardOption[]; value: string; onChange: (value: string) => void; label: string; columns?: 2 | 3 | 4 }) {
  return (
    <RadioGroup.Root
      value={value}
      onValueChange={(details) => {
        if (details.value !== null) onChange(details.value);
      }}
    >
      <RadioGroup.Label className="ui-label">{label}</RadioGroup.Label>
      <div className={`radio-card-grid cols-${String(columns)}`}>
        {options.map((option) => (
          <RadioGroup.Item key={option.value} value={option.value} className="radio-card">
            {option.preview}
            <RadioGroup.ItemText>
              <span className="radio-card-title">{option.title}</span>
              {option.sub !== undefined && <span className="radio-card-sub">{option.sub}</span>}
            </RadioGroup.ItemText>
            <RadioGroup.ItemControl className="sr-only" />
            <RadioGroup.ItemHiddenInput />
          </RadioGroup.Item>
        ))}
      </div>
    </RadioGroup.Root>
  );
}

export function NumberField({ label, value, onChange, min, max, step = 1, unit }: { label: string; value: number; onChange: (value: number) => void; min: number; max: number; step?: number; unit?: string }) {
  return (
    <NumberInput.Root
      className="form-field"
      value={String(value)}
      min={min}
      max={max}
      step={step}
      clampValueOnBlur
      onValueChange={(details) => {
        if (Number.isFinite(details.valueAsNumber)) onChange(details.valueAsNumber);
      }}
    >
      <NumberInput.Label className="ui-label">
        {label}
        {unit !== undefined && <span className="muted-text">（{unit}）</span>}
      </NumberInput.Label>
      <NumberInput.Control className="stepper">
        <NumberInput.DecrementTrigger aria-label="减少">
          <Minus size={14} aria-hidden="true" />
        </NumberInput.DecrementTrigger>
        <NumberInput.Input className="stepper-input tabular" />
        <NumberInput.IncrementTrigger aria-label="增加">
          <Plus size={14} aria-hidden="true" />
        </NumberInput.IncrementTrigger>
      </NumberInput.Control>
    </NumberInput.Root>
  );
}

export function Slider({ label, value, onChange, min, max, step = 1, format = String }: { label: string; value: readonly number[]; onChange: (value: number[]) => void; min: number; max: number; step?: number; format?: (value: number) => string }) {
  return (
    <ArkSlider.Root
      className="ui-slider"
      value={[...value]}
      min={min}
      max={max}
      step={step}
      minStepsBetweenThumbs={0}
      onValueChange={(details) => {
        onChange(details.value);
      }}
    >
      <div className="row-between">
        <ArkSlider.Label className="ui-label">{label}</ArkSlider.Label>
        <span className="muted-text tabular">{value.map(format).join(" – ")}</span>
      </div>
      <ArkSlider.Control className="ui-slider-control">
        <ArkSlider.Track className="ui-slider-track">
          <ArkSlider.Range className="ui-slider-range" />
        </ArkSlider.Track>
        {value.map((_, index) => (
          // 滑块数量固定，序号即稳定键
          <ArkSlider.Thumb key={`thumb-${String(index)}`} index={index} className="ui-slider-thumb">
            <ArkSlider.HiddenInput />
          </ArkSlider.Thumb>
        ))}
      </ArkSlider.Control>
    </ArkSlider.Root>
  );
}

/** 区间滑杆（两个滑块）。 */
export function RangeSlider(props: Parameters<typeof Slider>[0]) {
  return <Slider {...props} />;
}
