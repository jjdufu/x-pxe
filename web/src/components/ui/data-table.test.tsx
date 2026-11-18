import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { DataTable, selectAllState, type Column } from "./data-table";

interface Row {
  id: string;
  host: string;
}

const ROWS: Row[] = [
  { id: "1", host: "node-01" },
  { id: "2", host: "node-02" },
  { id: "3", host: "edge-01" },
];
const COLUMNS: Column<Row>[] = [{ id: "host", header: "主机名", cell: (row) => row.host }];

const header = () => screen.getByRole<HTMLInputElement>("checkbox", { name: "全选可见行" });
const row = (key: string) => screen.getByRole<HTMLInputElement>("checkbox", { name: `选择 ${key}` });

describe("selectAllState", () => {
  it("只看可见行", () => {
    expect(selectAllState(["1", "2"], new Set(["1", "2", "3"]))).toEqual({ checked: true, indeterminate: false });
    expect(selectAllState(["1", "2"], new Set(["3"]))).toEqual({ checked: false, indeterminate: false });
    expect(selectAllState(["1", "2"], new Set(["2"]))).toEqual({ checked: false, indeterminate: true });
    expect(selectAllState([], new Set(["2"]))).toEqual({ checked: false, indeterminate: false });
  });
});

describe("DataTable 全选与半选", () => {
  it("全选只作用于过滤后的可见行，过滤变化立即重算，隐藏的已选行保持选中", async () => {
    const user = userEvent.setup();
    render(<DataTable id="t1" label="节点" rows={ROWS} rowKey={(item) => item.id} columns={COLUMNS} searchText={(item) => item.host} />);
    await user.click(row("3"));
    expect(header().indeterminate).toBe(true);
    expect(screen.getByText(/已选 1 行/u)).toBeTruthy();

    await user.type(screen.getByRole("searchbox", { name: "搜索节点" }), "node");
    // edge-01（已选）被过滤掉：可见行无选中
    expect(header().checked).toBe(false);
    expect(header().indeterminate).toBe(false);
    expect(screen.getByText(/已选 0 行/u)).toBeTruthy();

    await user.click(header());
    expect(row("1").checked && row("2").checked).toBe(true);
    expect(header().checked).toBe(true);
    expect(screen.getByText(/已选 2 行/u)).toBeTruthy();

    await user.click(row("2"));
    expect(header().indeterminate).toBe(true);

    await user.clear(screen.getByRole("searchbox", { name: "搜索节点" }));
    // 清除过滤：edge-01 仍保持选中
    expect(row("3").checked).toBe(true);
    expect(screen.getByText(/已选 2 行/u)).toBeTruthy();
  });

  it("加载态显示 4 行骨架，过滤为空时提供重置筛选", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<DataTable id="t2" label="节点" rows={[]} rowKey={(item: Row) => item.id} columns={COLUMNS} loading />);
    const table = screen.getByRole("table", { name: "节点" });
    expect(within(table).getAllByRole("row")).toHaveLength(5);
    unmount();
    render(<DataTable id="t3" label="节点" rows={ROWS} rowKey={(item) => item.id} columns={COLUMNS} searchText={(item) => item.host} />);
    await user.type(screen.getByRole("searchbox", { name: "搜索节点" }), "zzz");
    await user.click(screen.getByRole("button", { name: "重置筛选" }));
    expect(screen.getAllByRole("checkbox")).toHaveLength(4);
  });
});
