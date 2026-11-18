// 参考实现：.combobox-wrap、.combobox-tags、.combobox-dropdown；app.js renderCombobox（多选标签、输入过滤）。
import { Combobox as ArkCombobox, createListCollection } from "@ark-ui/react/combobox";
import { Check, X } from "lucide-react";
import { useMemo, useState } from "react";
import type { Option } from "./select";
import { FIXED, Layer } from "./layer";

export interface MultiComboboxProps {
  label: string;
  options: readonly Option[];
  value: readonly string[];
  onChange: (value: string[]) => void;
  placeholder?: string;
}

export function MultiCombobox({ label, options, value, onChange, placeholder = "输入以筛选" }: MultiComboboxProps) {
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return needle === "" ? [...options] : options.filter((option) => option.label.toLowerCase().includes(needle) || option.value.toLowerCase().includes(needle));
  }, [options, query]);
  const collection = useMemo(() => createListCollection({ items: filtered }), [filtered]);
  const labelOf = (key: string) => options.find((option) => option.value === key)?.label ?? key;
  return (
    <ArkCombobox.Root
      className="combobox-wrap"
      collection={collection}
      multiple
      selectionBehavior="clear"
      inputBehavior="autohighlight"
      value={[...value]}
      onInputValueChange={(details) => {
        setQuery(details.inputValue);
      }}
      onValueChange={(details) => {
        onChange(details.value);
      }}
      positioning={{ ...FIXED, sameWidth: true }}
      openOnClick
    >
      <ArkCombobox.Label className="ui-label">{label}</ArkCombobox.Label>
      {value.length > 0 && (
        <div className="combobox-tags">
          {value.map((key) => (
            <span key={key} className="ui-badge">
              {labelOf(key)}
              <button
                type="button"
                className="badge-close"
                aria-label={`移除 ${labelOf(key)}`}
                onClick={() => {
                  onChange(value.filter((item) => item !== key));
                }}
              >
                <X size={11} aria-hidden="true" />
              </button>
            </span>
          ))}
        </div>
      )}
      <ArkCombobox.Control>
        <ArkCombobox.Input className="ui-input" placeholder={placeholder} />
      </ArkCombobox.Control>
      <Layer>
        <ArkCombobox.Positioner>
          <ArkCombobox.Content className="combobox-dropdown">
            {collection.items.length === 0 ? (
              <div className="combobox-empty muted-text">无匹配项</div>
            ) : (
              collection.items.map((item) => (
                <ArkCombobox.Item key={item.value} item={item} className="combobox-option">
                  <ArkCombobox.ItemText>{item.label}</ArkCombobox.ItemText>
                  <ArkCombobox.ItemIndicator className="ui-select-check">
                    <Check size={14} aria-hidden="true" />
                  </ArkCombobox.ItemIndicator>
                </ArkCombobox.Item>
              ))
            )}
          </ArkCombobox.Content>
        </ArkCombobox.Positioner>
      </Layer>
    </ArkCombobox.Root>
  );
}
