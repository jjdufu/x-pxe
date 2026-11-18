// 参考实现：#cmdModal、.cmd-dialog、.cmd-input-wrap、.cmd-list、.cmd-item.highlighted；app.js openCmdPalette、updateCmdHighlight、renderCmdList。
// 外壳（遮罩、焦点陷阱、ESC）用 Ark Dialog；列表为 combobox + listbox 模式（aria-activedescendant），↑↓ 移动高亮、Enter 执行。
import { Dialog as ArkDialog } from "@ark-ui/react/dialog";
import { Portal } from "@ark-ui/react/portal";
import { Search } from "lucide-react";
import { useId, useMemo, useState, type KeyboardEvent, type ReactNode } from "react";
import { cx } from "../../lib/cx";

export interface Command {
  id: string;
  label: string;
  group: string;
  keywords?: string;
  hint?: ReactNode;
  run: () => void;
}

export function filterCommands(commands: readonly Command[], query: string): Command[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...commands];
  return commands.filter((command) => `${command.label} ${command.keywords ?? ""} ${command.group}`.toLowerCase().includes(needle));
}

export function CommandPalette({ open, onOpenChange, commands }: { open: boolean; onOpenChange: (open: boolean) => void; commands: readonly Command[] }) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listId = useId();
  const items = useMemo(() => filterCommands(commands, query), [commands, query]);
  const current = Math.min(active, Math.max(0, items.length - 1));

  const close = () => {
    onOpenChange(false);
    setQuery("");
    setActive(0);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive(items.length === 0 ? 0 : (current + 1) % items.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive(items.length === 0 ? 0 : (current - 1 + items.length) % items.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      const command = items[current];
      if (command !== undefined) {
        close();
        command.run();
      }
    }
  };

  return (
    <ArkDialog.Root
      open={open}
      onOpenChange={(details) => {
        if (details.open) onOpenChange(true);
        else close();
      }}
    >
      <Portal>
        <ArkDialog.Backdrop className="ui-backdrop" />
        <ArkDialog.Positioner className="dialog-positioner cmd-positioner">
          <ArkDialog.Content className="cmd-dialog" aria-label="命令面板">
            <ArkDialog.Title className="sr-only">命令面板</ArkDialog.Title>
            <div className="cmd-input-wrap">
              <Search size={16} aria-hidden="true" />
              <input
                role="combobox"
                aria-expanded="true"
                aria-controls={listId}
                aria-autocomplete="list"
                aria-activedescendant={items[current] === undefined ? undefined : `${listId}-${items[current].id}`}
                aria-label="输入命令或页面名"
                placeholder="输入命令或页面名…"
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value);
                  setActive(0);
                }}
                onKeyDown={onKeyDown}
              />
              <kbd>ESC</kbd>
            </div>
            <ul className="cmd-list" id={listId} role="listbox" aria-label="命令">
              {items.length === 0 && <li className="cmd-empty muted-text">没有匹配的命令</li>}
              {items.map((command, index) => {
                const header = index === 0 || items[index - 1]?.group !== command.group ? command.group : null;
                return (
                  <li key={command.id} role="presentation">
                    {header !== null && (
                      <div className="cmd-group" role="presentation">
                        {header}
                      </div>
                    )}
                    <div
                      id={`${listId}-${command.id}`}
                      role="option"
                      aria-selected={index === current}
                      className={cx("cmd-item", index === current && "highlighted")}
                      onMouseMove={() => {
                        setActive(index);
                      }}
                      onClick={() => {
                        close();
                        command.run();
                      }}
                    >
                      <span>{command.label}</span>
                      {command.hint}
                    </div>
                  </li>
                );
              })}
            </ul>
          </ArkDialog.Content>
        </ArkDialog.Positioner>
      </Portal>
    </ArkDialog.Root>
  );
}
