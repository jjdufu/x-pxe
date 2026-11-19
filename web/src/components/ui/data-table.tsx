// DataTable（规范 §3 TABLE & CHECKBOX）：以参考实现 components.js #sec-table 与 app.js getFilteredTableRows、
// syncTableSelectAll、renderTableRows 为蓝本。全选只作用于过滤后的可见行；行复选框用 tbody 事件委托。
import { ArrowDown, ArrowUp, ArrowUpDown, Columns3, MoreHorizontal, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ChangeEvent, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { cx } from "../../lib/cx";
import { readJson, writeJson } from "../../lib/storage";
import { Button } from "./button";
import { Menu, PointMenu, type MenuEntry } from "./menu";
import { Select, type Option } from "./select";
import { Skeleton } from "./feedback";

export interface Column<Row> {
  id: string;
  header: string;
  cell: (row: Row) => ReactNode;
  /** 排序键；缺省时该列不可排序。 */
  sortValue?: (row: Row) => string | number;
  /** 数值、时间、MAC、IP、SHA-256、大小列：tabular + mono。 */
  data?: boolean;
  /** 能否在列菜单中隐藏（默认可以）。 */
  hideable?: boolean;
}

export interface TableFilter<Row> {
  label: string;
  options: readonly Option[];
  match: (row: Row, value: string) => boolean;
}

export interface DataTableProps<Row> {
  id: string;
  label: string;
  rows: readonly Row[];
  rowKey: (row: Row) => string;
  columns: readonly Column<Row>[];
  searchText?: (row: Row) => string;
  filter?: TableFilter<Row>;
  loading?: boolean;
  /** 批量操作：只收到“可见且选中”的行。 */
  bulkActions?: (rows: Row[]) => ReactNode;
  primaryAction?: ReactNode;
  /** 追加的筛选（如通道）。 */
  filters?: readonly TableFilter<Row>[];
  /** 第一个筛选的初始值（如从总览带筛选跳转）。 */
  initialFilter?: string | undefined;
  /** 行操作菜单（操作列 + 右键菜单共用）。 */
  rowMenu?: (row: Row) => readonly MenuEntry[];
  onRowMenu?: (row: Row, value: string) => void;
  /** Enter 或点击行：打开详情。 */
  onRowActivate?: (row: Row) => void;
  /** 当前打开详情的行（高亮）。 */
  activeKey?: string | null;
}

interface TablePrefs {
  hidden: string[];
  sort: { id: string; dir: "asc" | "desc" } | null;
}

function isPrefs(value: unknown): value is TablePrefs {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return Array.isArray(record.hidden) && record.hidden.every((item) => typeof item === "string") && (record.sort === null || typeof record.sort === "object");
}

/** 行复选框的变化由 tbody 委托处理；这里只满足 React 受控输入的要求。 */
const delegated = () => undefined;

const prefsKey = (id: string) => `xpxe.table.${id}.v1`;
const ALL = "__all__";

/** 全选框状态：只看可见行。 */
export function selectAllState(visible: readonly string[], selected: ReadonlySet<string>): { checked: boolean; indeterminate: boolean } {
  const count = visible.filter((key) => selected.has(key)).length;
  return { checked: visible.length > 0 && count === visible.length, indeterminate: count > 0 && count < visible.length };
}

/** 行内可交互元素上的点击不触发“打开详情”。 */
function interactive(target: EventTarget): boolean {
  return target instanceof Element && target.closest("button, input, a, label, [role=menuitem]") !== null;
}

/** 焦点在行间移动：操作按钮之间、或整行之间。 */
function moveFocus(from: Element, delta: 1 | -1): boolean {
  const row = from.closest("tr[data-row]");
  if (row === null) return false;
  const next = (delta === 1 ? row.nextElementSibling : row.previousElementSibling) as HTMLElement | null;
  if (next?.dataset.row === undefined) return false;
  const button = from.classList.contains("row-menu-btn") ? next.querySelector<HTMLElement>(".row-menu-btn") : null;
  (button ?? next).focus();
  return true;
}

export function DataTable<Row>({ id, label, rows, rowKey, columns, searchText, filter, loading = false, bulkActions, primaryAction, filters = [], initialFilter, rowMenu, onRowMenu, onRowActivate, activeKey = null }: DataTableProps<Row>) {
  const [prefs, setPrefs] = useState<TablePrefs>(() => readJson(prefsKey(id), isPrefs) ?? { hidden: [], sort: null });
  const [query, setQuery] = useState("");
  const [filterValue, setFilterValue] = useState(initialFilter ?? ALL);
  const [extraValues, setExtraValues] = useState<readonly string[]>(() => filters.map(() => ALL));
  const [contextKey, setContextKey] = useState<string | null>(null);
  const [contextPoint, setContextPoint] = useState<{ x: number; y: number } | null>(null);
  const closeContext = useCallback(() => {
    setContextPoint(null);
  }, []);
  const bodyRef = useRef<HTMLTableSectionElement>(null);
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set());
  const headerBox = useRef<HTMLInputElement>(null);

  const updatePrefs = (next: TablePrefs) => {
    setPrefs(next);
    writeJson(prefsKey(id), next);
  };

  const shown = columns.filter((column) => !prefs.hidden.includes(column.id));

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    let out = rows.filter((row) => {
      if (needle !== "" && searchText !== undefined && !searchText(row).toLowerCase().includes(needle)) return false;
      if (filter !== undefined && filterValue !== ALL && !filter.match(row, filterValue)) return false;
      return filters.every((extra, index) => {
        const value = extraValues[index] ?? ALL;
        return value === ALL || extra.match(row, value);
      });
    });
    const sort = prefs.sort;
    const column = sort === null ? undefined : columns.find((item) => item.id === sort.id);
    if (sort !== null && column?.sortValue !== undefined) {
      const key = column.sortValue;
      out = [...out].sort((a, b) => {
        const left = key(a);
        const right = key(b);
        const order = typeof left === "number" && typeof right === "number" ? left - right : String(left).localeCompare(String(right));
        return sort.dir === "asc" ? order : -order;
      });
    }
    return out;
  }, [rows, query, searchText, filter, filterValue, filters, extraValues, prefs.sort, columns]);

  const visibleKeys = useMemo(() => visible.map(rowKey), [visible, rowKey]);
  const all = selectAllState(visibleKeys, selected);
  const actionable = visible.filter((row) => selected.has(rowKey(row)));

  useEffect(() => {
    // 同参考实现 syncIndeterminate：indeterminate 只能经 DOM 属性设置
    if (headerBox.current !== null) headerBox.current.indeterminate = all.indeterminate;
  }, [all.indeterminate]);

  const toggleAll = () => {
    const next = new Set(selected);
    if (all.checked) for (const key of visibleKeys) next.delete(key);
    else for (const key of visibleKeys) next.add(key);
    setSelected(next);
  };

  // tbody 事件委托：一次绑定处理所有行复选框（同参考实现 tbody.onchange）
  const onBodyChange = (event: ChangeEvent<HTMLTableSectionElement>) => {
    const target = event.target as unknown as HTMLInputElement;
    const key = target.dataset.rowKey;
    if (key === undefined) return;
    const next = new Set(selected);
    if (target.checked) next.add(key);
    else next.delete(key);
    setSelected(next);
  };

  const sortBy = (column: Column<Row>) => {
    if (column.sortValue === undefined) return;
    const current = prefs.sort;
    const dir = current?.id === column.id && current.dir === "asc" ? "desc" : "asc";
    updatePrefs({ ...prefs, sort: { id: column.id, dir } });
  };

  const resetFilters = () => {
    setQuery("");
    setFilterValue(ALL);
    setExtraValues(filters.map(() => ALL));
  };

  const onSearchKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "ArrowDown") return;
    const first = bodyRef.current?.querySelector<HTMLElement>("tr[data-row]");
    if (first === undefined || first === null) return;
    event.preventDefault();
    (first.querySelector<HTMLElement>(".row-menu-btn") ?? first).focus();
  };

  /** 捕获阶段处理方向键：行操作按钮是 Ark Menu 触发器，冒泡阶段它会把 ↑↓ 当作“打开菜单”。 */
  const onBodyKeyCapture = (event: KeyboardEvent<HTMLTableSectionElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const target = event.target as HTMLElement;
    if (target.closest("[role=menu]") !== null) return;
    if (moveFocus(target, event.key === "ArrowDown" ? 1 : -1)) {
      event.preventDefault();
      event.stopPropagation();
    }
  };

  const onBodyKey = (event: KeyboardEvent<HTMLTableSectionElement>) => {
    const target = event.target as HTMLElement;
    if (event.key === "Enter" && target.dataset.row !== undefined && onRowActivate !== undefined) {
      const row = visible.find((item) => rowKey(item) === target.dataset.row);
      if (row !== undefined) {
        event.preventDefault();
        onRowActivate(row);
      }
    }
  };

  const onRowClick = (event: MouseEvent<HTMLTableRowElement>, row: Row) => {
    if (onRowActivate !== undefined && !interactive(event.target)) onRowActivate(row);
  };

  const contextRow = contextKey === null ? undefined : visible.find((row) => rowKey(row) === contextKey);
  const hasMenu = rowMenu !== undefined;

  return (
    <div className="data-table">
      <div className="table-toolbar row-between">
        <div className="row-gap">
          <label className="input-group table-search">
            <span className="input-addon">
              <Search size={14} aria-hidden="true" />
            </span>
            <input
              className="table-search-input"
              type="search"
              aria-label={`搜索${label}`}
              placeholder="搜索"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
              }}
              onKeyDown={onSearchKey}
            />
          </label>
          {filter !== undefined && (
            <Select
              className="table-filter"
              label={filter.label}
              hideLabel
              options={[{ value: ALL, label: `全部${filter.label}` }, ...filter.options]}
              value={filterValue}
              onChange={setFilterValue}
            />
          )}
          {filters.map((extra, index) => (
            <Select
              key={extra.label}
              className="table-filter"
              label={extra.label}
              hideLabel
              options={[{ value: ALL, label: `全部${extra.label}` }, ...extra.options]}
              value={extraValues[index] ?? ALL}
              onChange={(value) => {
                setExtraValues(extraValues.map((item, position) => (position === index ? value : item)));
              }}
            />
          ))}
          <span className="muted-text tabular" aria-live="polite">
            已选 {actionable.length} 行
          </span>
        </div>
        <div className="row-gap">
          {bulkActions?.(actionable)}
          <Menu
            trigger={
              <Button variant="outline" size="sm" aria-label="显示列">
                <Columns3 size={14} aria-hidden="true" />
                <span className="hide-sm">列</span>
              </Button>
            }
            entries={columns
              .filter((column) => column.hideable !== false)
              .map((column) => ({ kind: "check" as const, value: column.id, label: column.header, checked: !prefs.hidden.includes(column.id) }))}
            onSelect={() => undefined}
            onCheck={(value, checked) => {
              updatePrefs({ ...prefs, hidden: checked ? prefs.hidden.filter((item) => item !== value) : [...prefs.hidden, value] });
            }}
          />
          {primaryAction}
        </div>
      </div>
      <PointMenu
        point={contextRow === undefined ? null : contextPoint}
        entries={contextRow === undefined || rowMenu === undefined ? [] : rowMenu(contextRow)}
        onSelect={(value) => {
          if (contextRow !== undefined) onRowMenu?.(contextRow, value);
        }}
        onClose={closeContext}
      />
      <div className="table-wrap">
        <table className="ui-table" aria-label={label} aria-busy={loading}>
          <thead>
            <tr>
              <th className="col-check">
                <input
                  ref={headerBox}
                  type="checkbox"
                  className="ui-checkbox"
                  aria-label="全选可见行"
                  checked={all.checked}
                  disabled={loading || visible.length === 0}
                  onChange={toggleAll}
                />
              </th>
              {shown.map((column) => {
                const sorted = prefs.sort?.id === column.id ? prefs.sort.dir : null;
                return (
                  <th key={column.id} className={cx(column.sortValue !== undefined && "sortable")} aria-sort={sorted === null ? undefined : sorted === "asc" ? "ascending" : "descending"}>
                    {column.sortValue === undefined ? (
                      column.header
                    ) : (
                      <button
                        type="button"
                        className="th-sort"
                        onClick={() => {
                          sortBy(column);
                        }}
                      >
                        {column.header}
                        {sorted === "asc" ? <ArrowUp size={12} aria-hidden="true" /> : sorted === "desc" ? <ArrowDown size={12} aria-hidden="true" /> : <ArrowUpDown size={12} aria-hidden="true" />}
                      </button>
                    )}
                  </th>
                );
              })}
              {hasMenu && (
                <th className="col-menu">
                  <span className="sr-only">操作</span>
                </th>
              )}
            </tr>
          </thead>
          <tbody
            ref={bodyRef}
            onChange={onBodyChange}
            onKeyDown={onBodyKey}
            onKeyDownCapture={onBodyKeyCapture}
            onContextMenu={(event) => {
              const row = (event.target as Element).closest<HTMLElement>("tr[data-row]");
              if (!hasMenu || row?.dataset.row === undefined) return;
              event.preventDefault();
              setContextKey(row.dataset.row);
              setContextPoint({ x: event.clientX, y: event.clientY });
            }}
          >
            {loading
              ? ["s1", "s2", "s3", "s4"].map((key) => (
                  <tr key={key}>
                    <td className="col-check">
                      <Skeleton width={16} height={16} />
                    </td>
                    {shown.map((column) => (
                      <td key={column.id}>
                        <Skeleton width="70%" />
                      </td>
                    ))}
                    {hasMenu && <td className="col-menu" />}
                  </tr>
                ))
              : visible.map((row) => {
                  const key = rowKey(row);
                  return (
                    <tr
                      key={key}
                      data-row={key}
                      tabIndex={-1}
                      data-selected={selected.has(key) ? "" : undefined}
                      data-active={activeKey === key ? "" : undefined}
                      className={cx(onRowActivate !== undefined && "row-clickable")}
                      onClick={(event) => {
                        onRowClick(event, row);
                      }}
                    >
                      <td className="col-check">
                        <input type="checkbox" className="ui-checkbox" aria-label={`选择 ${key}`} data-row-key={key} checked={selected.has(key)} onChange={delegated} />
                      </td>
                      {shown.map((column) => (
                        <td key={column.id} className={cx(column.data === true && "tabular mono")}>
                          {column.cell(row)}
                        </td>
                      ))}
                      {rowMenu !== undefined && (
                        <td className="col-menu">
                          <Menu
                            trigger={
                              <button type="button" className="icon-btn row-menu-btn" aria-label={`操作 ${key}`}>
                                <MoreHorizontal size={15} aria-hidden="true" />
                              </button>
                            }
                            entries={rowMenu(row)}
                            onSelect={(value) => onRowMenu?.(row, value)}
                          />
                        </td>
                      )}
                    </tr>
                  );
                })}
            {!loading && visible.length === 0 && (
              <tr>
                <td colSpan={shown.length + (hasMenu ? 2 : 1)} className="table-empty">
                  <span className="muted-text">没有匹配的行</span>
                  <Button variant="ghost" size="sm" onClick={resetFilters}>
                    重置筛选
                  </Button>
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

