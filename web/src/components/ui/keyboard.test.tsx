import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { MultiCombobox } from "./combobox";
import { CommandPalette, filterCommands } from "./command-palette";
import { Menu } from "./menu";
import { Select } from "./select";

const OPTIONS = [
  { value: "uefi", label: "UEFI PXE" },
  { value: "bios", label: "BIOS PXE" },
  { value: "http", label: "UEFI HTTP Boot" },
];

function SelectHarness({ onChange }: { onChange: (value: string) => void }) {
  const [value, setValue] = useState<string | null>(null);
  return (
    <Select
      label="引导方式"
      options={OPTIONS}
      value={value}
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}

describe("Select 键盘", () => {
  it("Enter 打开、↓ 移动、Enter 选择，aria-expanded 同步；ESC 关闭", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<SelectHarness onChange={onChange} />);
    const trigger = screen.getByRole("combobox", { name: "引导方式" });
    trigger.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(trigger.getAttribute("aria-expanded")).toBe("true");
    });
    await user.keyboard("{ArrowDown}{Enter}");
    await waitFor(() => {
      expect(onChange).toHaveBeenCalledWith("bios");
    });
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    expect(trigger.textContent).toContain("BIOS PXE");
    trigger.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(trigger.getAttribute("aria-expanded")).toBe("true");
    });
    expect(screen.getByRole("option", { name: "BIOS PXE" }).getAttribute("aria-selected")).toBe("true");
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(trigger.getAttribute("aria-expanded")).toBe("false");
    });
  });
});

describe("Menu 键盘", () => {
  it("Enter 打开、↓ 高亮、Enter 执行", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <Menu
        trigger={<button type="button">行操作</button>}
        entries={[
          { kind: "item", value: "arm", label: "Arm" },
          { kind: "item", value: "disarm", label: "Disarm" },
        ]}
        onSelect={onSelect}
      />,
    );
    const trigger = screen.getByRole("button", { name: "行操作" });
    trigger.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(trigger.getAttribute("aria-expanded")).toBe("true");
    });
    await user.keyboard("{ArrowDown}{Enter}");
    await waitFor(() => {
      expect(onSelect).toHaveBeenCalledWith("disarm");
    });
  });
});

function ComboHarness({ onChange }: { onChange: (value: string[]) => void }) {
  const [value, setValue] = useState<string[]>([]);
  return (
    <MultiCombobox
      label="时区"
      options={[
        { value: "Asia/Shanghai", label: "Asia/Shanghai" },
        { value: "UTC", label: "UTC" },
        { value: "Europe/Berlin", label: "Europe/Berlin" },
      ]}
      value={value}
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}

/** 逐键输入（jsdom 下一次性输入多字符时，过滤列表的重渲染会吞掉中间按键；真实浏览器中由 Playwright 覆盖）。 */
async function typeEach(user: ReturnType<typeof userEvent.setup>, text: string) {
  for (const ch of text) await user.keyboard(ch);
}

describe("Combobox 多选键盘", () => {
  it("输入过滤、自动高亮 + Enter 依次选中多个，标签可移除", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ComboHarness onChange={onChange} />);
    const input = screen.getByRole("combobox", { name: "时区" });
    await act(async () => {
      input.focus();
      await Promise.resolve();
    });
    await typeEach(user, "utc");
    // 输入即自动高亮第一个匹配项（inputBehavior=autohighlight），Enter 选中
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(onChange).toHaveBeenLastCalledWith(["UTC"]);
    });
    await typeEach(user, "asia");
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(onChange).toHaveBeenLastCalledWith(["UTC", "Asia/Shanghai"]);
    });
    await user.click(screen.getByRole("button", { name: "移除 UTC" }));
    expect(onChange).toHaveBeenLastCalledWith(["Asia/Shanghai"]);
  });
});

describe("CommandPalette", () => {
  it("按名称、关键字与分组过滤", () => {
    const commands = [
      { id: "a", label: "节点", group: "跳转页面", keywords: "/nodes", run: () => undefined },
      { id: "b", label: "资产", group: "跳转页面", run: () => undefined },
      { id: "c", label: "切换主题", group: "主题", run: () => undefined },
    ];
    expect(filterCommands(commands, "nodes").map((item) => item.id)).toEqual(["a"]);
    expect(filterCommands(commands, "主题").map((item) => item.id)).toEqual(["c"]);
    expect(filterCommands(commands, "  ")).toHaveLength(3);
  });

  it("↑↓ 移动高亮（aria-activedescendant）、Enter 执行并关闭", async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <CommandPalette
        open
        onOpenChange={onOpenChange}
        commands={[
          { id: "a", label: "节点", group: "跳转页面", run: () => undefined },
          { id: "b", label: "资产", group: "跳转页面", run },
        ]}
      />,
    );
    const input = await screen.findByRole("combobox", { name: "输入命令或页面名" });
    await act(async () => {
      input.focus();
      await Promise.resolve();
    });
    await user.keyboard("{ArrowDown}");
    const active = input.getAttribute("aria-activedescendant") ?? "";
    expect(document.getElementById(active)?.textContent).toContain("资产");
    await user.keyboard("{Enter}");
    expect(run).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
