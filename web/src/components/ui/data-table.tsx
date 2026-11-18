// DataTable（规范 §3 TABLE & CHECKBOX）：以参考实现 components.js #sec-table 与 app.js getFilteredTableRows、
// syncTableSelectAll、renderTableRows 为蓝本。全选只作用于过滤后的可见行；行复选框用 tbody 事件委托。
import { ArrowDown, ArrowUp, ArrowUpDown, Columns3, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ChangeEvent, type ReactNode } from "react";
import { cx } from "../../lib/cx";
import { readJson, writeJson } from "../../lib/storage";
import { Button } from "./button";
import { Menu } from "./menu";
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

export function DataTable<Row>({ id, label, rows, rowKey, columns, searchText, filter, loading = false, bulkActions, primaryAction }: DataTableProps<Row>) {
  const [prefs, setPrefs] = useState<TablePrefs>(() => readJson(prefsKey(id), isPrefs) ?? { hidden: [], sort: null });
  const [query, setQuery] = useState("");
  const [filterValue, setFilterValue] = useState(ALL);
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
      return true;
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
  }, [rows, query, searchText, filter, filterValue, prefs.sort, columns]);

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
  };

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
            </tr>
          </thead>
          <tbody onChange={onBodyChange}>
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
                  </tr>
                ))
              : visible.map((row) => {
                  const key = rowKey(row);
                  return (
                    <tr key={key} data-selected={selected.has(key) ? "" : undefined}>
                      <td className="col-check">
                        <input type="checkbox" className="ui-checkbox" aria-label={`选择 ${key}`} data-row-key={key} checked={selected.has(key)} onChange={delegated} />
                      </td>
                      {shown.map((column) => (
                        <td key={column.id} className={cx(column.data === true && "tabular mono")}>
                          {column.cell(row)}
                        </td>
                      ))}
                    </tr>
                  );
                })}
            {!loading && visible.length === 0 && (
              <tr>
                <td colSpan={shown.length + 1} className="table-empty">
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
