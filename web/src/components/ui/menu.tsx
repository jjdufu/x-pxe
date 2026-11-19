// 参考实现：.ui-dropdown、.ui-dropdown-menu、.ui-dropdown-item(.danger)、.ui-dropdown-divider；index.html #customContextMenu（带 kbd 提示）。
import { Menu as ArkMenu } from "@ark-ui/react/menu";
import { Check } from "lucide-react";
import { useEffect, useState, type ReactElement, type ReactNode } from "react";
import { onDismissOverlays } from "../../lib/overlay-bus";
import { cx } from "../../lib/cx";
import { FIXED, Layer } from "./layer";

export type MenuEntry =
  | { kind: "item"; value: string; label: string; danger?: boolean; shortcut?: string; disabled?: boolean }
  | { kind: "check"; value: string; label: string; checked: boolean }
  | { kind: "divider"; value: string };

interface MenuListProps {
  entries: readonly MenuEntry[];
  onSelect: (value: string) => void;
  onCheck?: (value: string, checked: boolean) => void;
}

function MenuList({ entries, onSelect, onCheck }: MenuListProps) {
  return (
    <Layer>
      <ArkMenu.Positioner>
        <ArkMenu.Content className="ui-dropdown-menu">
          {entries.map((entry) => {
            if (entry.kind === "divider") return <ArkMenu.Separator key={entry.value} className="ui-dropdown-divider" />;
            if (entry.kind === "check") {
              return (
                <ArkMenu.CheckboxItem
                  key={entry.value}
                  value={entry.value}
                  checked={entry.checked}
                  closeOnSelect={false}
                  onCheckedChange={(checked) => onCheck?.(entry.value, checked)}
                  className="ui-dropdown-item"
                >
                  <ArkMenu.ItemText>{entry.label}</ArkMenu.ItemText>
                  <ArkMenu.ItemIndicator>
                    <Check size={14} aria-hidden="true" />
                  </ArkMenu.ItemIndicator>
                </ArkMenu.CheckboxItem>
              );
            }
            return (
              <ArkMenu.Item
                key={entry.value}
                value={entry.value}
                disabled={entry.disabled === true}
                onSelect={() => {
                  onSelect(entry.value);
                }}
                className={cx("ui-dropdown-item", entry.danger === true && "danger")}
              >
                <span>{entry.label}</span>
                {entry.shortcut !== undefined && <kbd>{entry.shortcut}</kbd>}
              </ArkMenu.Item>
            );
          })}
        </ArkMenu.Content>
      </ArkMenu.Positioner>
    </Layer>
  );
}

export interface MenuProps extends MenuListProps {
  /** 触发元素（经 asChild 合并到 Ark Trigger）。 */
  trigger: ReactElement;
}

export function Menu({ trigger, ...list }: MenuProps) {
  return (
    <ArkMenu.Root positioning={{ ...FIXED, placement: "bottom-end" }}>
      <ArkMenu.Trigger asChild>{trigger}</ArkMenu.Trigger>
      <MenuList {...list} />
    </ArkMenu.Root>
  );
}

export interface ContextMenuProps extends MenuListProps {
  children: ReactNode;
  className?: string;
}

/** 右键菜单：包裹区域内右键打开；滚动时由 global-events.ts 关闭（经 open 状态复位）。 */
export function ContextMenu({ children, className, ...list }: ContextMenuProps) {
  const [open, setOpen] = useState(false);
  useEffect(
    () =>
      onDismissOverlays(() => {
        setOpen(false);
      }),
    [],
  );
  return (
    <ArkMenu.Root
      open={open}
      onOpenChange={(details) => {
        setOpen(details.open);
      }}
    >
      {/* asChild + div：默认的 <button> 会把整张表（含复选框、按钮）包进按钮里，既不合法也会吞掉行点击 */}
      <ArkMenu.ContextTrigger asChild>
        <div className={className}>{children}</div>
      </ArkMenu.ContextTrigger>
      <MenuList {...list} />
    </ArkMenu.Root>
  );
}

export interface PointMenuProps extends MenuListProps {
  /** 打开位置（视口坐标）；null 表示关闭。 */
  point: { x: number; y: number } | null;
  onClose: () => void;
}

/**
 * 在指定坐标打开的菜单（表格右键）。与 ContextMenu 不同，它不包裹内容：
 * 包裹会让区域内的行操作 Menu 成为它的子菜单（Ark 视作 trigger-item，Enter 不再打开）。
 */
export function PointMenu({ point, onClose, ...list }: PointMenuProps) {
  useEffect(() => onDismissOverlays(onClose), [onClose]);
  return (
    <ArkMenu.Root
      open={point !== null}
      onOpenChange={(details) => {
        if (!details.open) onClose();
      }}
      positioning={{ ...FIXED, placement: "bottom-start", getAnchorRect: () => (point === null ? null : { x: point.x, y: point.y, width: 0, height: 0 }) }}
    >
      <MenuList {...list} />
    </ArkMenu.Root>
  );
}
