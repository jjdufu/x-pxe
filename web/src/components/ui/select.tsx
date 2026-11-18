// 参考实现：.ui-select、.ui-select-trigger、.ui-select-menu、.ui-select-option、.ui-select-check；
// app.js bindCustomSelectsAndDropdowns（↑↓、Enter、ESC、外部点击、aria-expanded/aria-selected）——这些行为由 Ark Select 状态机提供。
import { Select as ArkSelect, createListCollection } from "@ark-ui/react/select";
import { Check, ChevronDown } from "lucide-react";
import { useMemo } from "react";
import { cx } from "../../lib/cx";
import { FIXED, Layer } from "./layer";

export interface Option {
  value: string;
  label: string;
  disabled?: boolean;
}

export interface SelectProps {
  label: string;
  options: readonly Option[];
  value: string | null;
  onChange: (value: string) => void;
  placeholder?: string;
  hideLabel?: boolean;
  disabled?: boolean;
  className?: string;
}

export function Select({ label, options, value, onChange, placeholder = "请选择", hideLabel = false, disabled = false, className }: SelectProps) {
  const collection = useMemo(() => createListCollection({ items: [...options], isItemDisabled: (item) => item.disabled === true }), [options]);
  return (
    <ArkSelect.Root
      collection={collection}
      value={value === null ? [] : [value]}
      onValueChange={(details) => {
        const next = details.value[0];
        if (next !== undefined) onChange(next);
      }}
      disabled={disabled}
      positioning={{ ...FIXED, sameWidth: true }}
      className={cx("ui-select", className)}
    >
      <ArkSelect.Label className={hideLabel ? "sr-only" : "ui-label"}>{label}</ArkSelect.Label>
      <ArkSelect.Control>
        <ArkSelect.Trigger className="ui-select-trigger">
          <ArkSelect.ValueText placeholder={placeholder} />
          <ArkSelect.Indicator className="ui-select-chevron">
            <ChevronDown size={14} aria-hidden="true" />
          </ArkSelect.Indicator>
        </ArkSelect.Trigger>
      </ArkSelect.Control>
      <Layer>
        <ArkSelect.Positioner>
          <ArkSelect.Content className="ui-select-menu">
            {collection.items.map((item) => (
              <ArkSelect.Item key={item.value} item={item} className="ui-select-option">
                <ArkSelect.ItemText>{item.label}</ArkSelect.ItemText>
                <ArkSelect.ItemIndicator className="ui-select-check">
                  <Check size={14} aria-hidden="true" />
                </ArkSelect.ItemIndicator>
              </ArkSelect.Item>
            ))}
          </ArkSelect.Content>
        </ArkSelect.Positioner>
      </Layer>
    </ArkSelect.Root>
  );
}
