// 菜单条目列表：拖拽排序；拖动手柄获得焦点时 ↑/↓ 键等价移动；Switch 启停；不可用条目标红。
import { ArrowDown, ArrowUp, GripVertical, Pencil, Trash2 } from "lucide-react";
import { useRef, useState, type KeyboardEvent } from "react";
import { Button, IconButton } from "../../../components/ui/button";
import { Switch } from "../../../components/ui/controls";
import { Badge } from "../../../components/ui/feedback";
import type { BootMenuEntry, BootMenuStatus } from "../../../lib/types";
import { move } from "./model";

const KIND_LABEL: Record<string, string> = { sanboot: "整盘 ISO", kernel_initrd: "内核 + initrd", chain: "链式脚本", exit: "本地启动" };
const ARCH_LABEL: Record<string, string> = { any: "全部固件", bios: "BIOS", uefi: "UEFI" };

export interface EntryListProps {
  entries: readonly BootMenuEntry[];
  status: BootMenuStatus | null;
  defaultEntry: string | null;
  onChange: (entries: BootMenuEntry[]) => void;
  onEdit: (index: number) => void;
}

function reason(status: BootMenuStatus | null, id: string): string | null {
  const action = status?.entries.find((item) => item.id === id)?.action;
  return action?.type === "unavailable" ? (action.reason ?? "不可用") : null;
}

export function EntryList({ entries, status, defaultEntry, onChange, onEdit }: EntryListProps) {
  const [dragging, setDragging] = useState<number | null>(null);
  const list = useRef<HTMLOListElement>(null);
  const shift = (index: number, delta: number) => {
    onChange(move(entries, index, delta));
    const target = index + delta;
    requestAnimationFrame(() => {
      list.current?.querySelector<HTMLButtonElement>(`[data-handle="${String(target)}"]`)?.focus();
    });
  };
  const onKey = (event: KeyboardEvent, index: number) => {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    shift(index, event.key === "ArrowUp" ? -1 : 1);
  };
  return (
    <ol className="menu-entries" ref={list} aria-label="菜单条目">
      {entries.map((entry, index) => {
        const broken = reason(status, entry.id);
        return (
          <li
            key={entry.id}
            className="menu-entry"
            data-testid={`entry-${entry.id}`}
            draggable
            onDragStart={() => {
              setDragging(index);
            }}
            onDragOver={(event) => {
              event.preventDefault();
            }}
            onDrop={() => {
              if (dragging !== null && dragging !== index) onChange(move(entries, dragging, index - dragging));
              setDragging(null);
            }}
          >
            <button type="button" className="drag-handle" data-handle={index} aria-label={`移动 ${entry.title}（↑/↓）`} onKeyDown={(event) => { onKey(event, index); }}>
              <GripVertical size={16} aria-hidden="true" />
            </button>
            <div className="menu-entry-main">
              <span className="menu-entry-title">{entry.title}</span>
              <span className="row-gap">
                <Badge tone="outline">{KIND_LABEL[entry.kind.type] ?? entry.kind.type}</Badge>
                <Badge>{ARCH_LABEL[entry.arch] ?? entry.arch}</Badge>
                {defaultEntry === entry.id && <Badge tone="solid">默认</Badge>}
                {broken !== null && entry.enabled && <Badge tone="danger">不可用：{broken}</Badge>}
              </span>
            </div>
            <Switch label={<span className="sr-only">启用 {entry.title}</span>} checked={entry.enabled} onChange={(enabled) => { onChange(entries.map((item, at) => (at === index ? { ...item, enabled } : item))); }} />
            <span className="row-gap">
              <IconButton label={`上移 ${entry.title}`} disabled={index === 0} onClick={() => { shift(index, -1); }}>
                <ArrowUp size={14} />
              </IconButton>
              <IconButton label={`下移 ${entry.title}`} disabled={index === entries.length - 1} onClick={() => { shift(index, 1); }}>
                <ArrowDown size={14} />
              </IconButton>
              <IconButton label={`编辑 ${entry.title}`} onClick={() => { onEdit(index); }}>
                <Pencil size={14} />
              </IconButton>
              <IconButton label={`删除 ${entry.title}`} onClick={() => { onChange(entries.filter((_, at) => at !== index)); }}>
                <Trash2 size={14} />
              </IconButton>
            </span>
          </li>
        );
      })}
      {entries.length === 0 && (
        <li className="muted-text">
          还没有条目。<Button variant="ghost" onClick={() => { onEdit(-1); }}>添加第一个</Button>
        </li>
      )}
    </ol>
  );
}
