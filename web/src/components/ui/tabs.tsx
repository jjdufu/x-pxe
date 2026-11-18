// 参考实现：.tabs-bar/.tab-btn（Tabs、Segmented）、.toggle-group/.toggle-btn（ToggleGroup）。
import { SegmentGroup } from "@ark-ui/react/segment-group";
import { Tabs as ArkTabs } from "@ark-ui/react/tabs";
import { ToggleGroup as ArkToggleGroup } from "@ark-ui/react/toggle-group";
import type { ReactNode } from "react";
import type { Option } from "./select";

export interface TabItem {
  value: string;
  label: string;
  content: ReactNode;
}

export function Tabs({ items, value, onChange, label }: { items: readonly TabItem[]; value: string; onChange: (value: string) => void; label: string }) {
  return (
    <ArkTabs.Root
      value={value}
      onValueChange={(details) => {
        onChange(details.value);
      }}
    >
      <ArkTabs.List className="tabs-bar" aria-label={label}>
        {items.map((item) => (
          <ArkTabs.Trigger key={item.value} value={item.value} className="tab-btn">
            {item.label}
          </ArkTabs.Trigger>
        ))}
      </ArkTabs.List>
      {items.map((item) => (
        <ArkTabs.Content key={item.value} value={item.value} className="tab-panel">
          {item.content}
        </ArkTabs.Content>
      ))}
    </ArkTabs.Root>
  );
}

export function Segmented({ options, value, onChange, label }: { options: readonly Option[]; value: string; onChange: (value: string) => void; label: string }) {
  return (
    <SegmentGroup.Root
      className="tabs-bar segmented"
      value={value}
      aria-label={label}
      onValueChange={(details) => {
        if (details.value !== null) onChange(details.value);
      }}
    >
      {options.map((option) => (
        <SegmentGroup.Item key={option.value} value={option.value} className="tab-btn">
          <SegmentGroup.ItemText>{option.label}</SegmentGroup.ItemText>
          <SegmentGroup.ItemControl />
          <SegmentGroup.ItemHiddenInput />
        </SegmentGroup.Item>
      ))}
    </SegmentGroup.Root>
  );
}

export function ToggleGroup({ options, value, onChange, label }: { options: readonly (Option & { icon?: ReactNode })[]; value: string; onChange: (value: string) => void; label: string }) {
  return (
    <ArkToggleGroup.Root
      className="toggle-group"
      aria-label={label}
      value={[value]}
      onValueChange={(details) => {
        const next = details.value[0];
        if (next !== undefined) onChange(next);
      }}
    >
      {options.map((option) => (
        <ArkToggleGroup.Item key={option.value} value={option.value} className="toggle-btn" aria-label={option.label}>
          {option.icon ?? option.label}
        </ArkToggleGroup.Item>
      ))}
    </ArkToggleGroup.Root>
  );
}
