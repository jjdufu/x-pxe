// 参考实现：.tooltip-wrap/.tooltip-bubble、.hover-card-wrap/-panel、.popover-panel。
import { HoverCard as ArkHoverCard } from "@ark-ui/react/hover-card";
import { Popover as ArkPopover } from "@ark-ui/react/popover";
import { Tooltip as ArkTooltip } from "@ark-ui/react/tooltip";
import type { ReactElement, ReactNode } from "react";
import { FIXED, Layer } from "./layer";

export function Tooltip({ content, children }: { content: ReactNode; children: ReactElement }) {
  return (
    <ArkTooltip.Root openDelay={300} closeDelay={100} positioning={FIXED}>
      <ArkTooltip.Trigger asChild>{children}</ArkTooltip.Trigger>
      <Layer>
        <ArkTooltip.Positioner>
          <ArkTooltip.Content className="tooltip-bubble">{content}</ArkTooltip.Content>
        </ArkTooltip.Positioner>
      </Layer>
    </ArkTooltip.Root>
  );
}

export function HoverCard({ content, children }: { content: ReactNode; children: ReactElement }) {
  return (
    <ArkHoverCard.Root openDelay={300} positioning={FIXED}>
      <ArkHoverCard.Trigger asChild>{children}</ArkHoverCard.Trigger>
      <Layer>
        <ArkHoverCard.Positioner>
          <ArkHoverCard.Content className="hover-card-panel">{content}</ArkHoverCard.Content>
        </ArkHoverCard.Positioner>
      </Layer>
    </ArkHoverCard.Root>
  );
}

export function Popover({ title, content, children }: { title: string; content: ReactNode; children: ReactElement }) {
  return (
    <ArkPopover.Root positioning={FIXED}>
      <ArkPopover.Trigger asChild>{children}</ArkPopover.Trigger>
      <Layer>
        <ArkPopover.Positioner>
          <ArkPopover.Content className="popover-panel">
            <ArkPopover.Title className="popover-title">{title}</ArkPopover.Title>
            <ArkPopover.Description asChild>
              <div>{content}</div>
            </ArkPopover.Description>
          </ArkPopover.Content>
        </ArkPopover.Positioner>
      </Layer>
    </ArkPopover.Root>
  );
}
