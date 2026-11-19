import { describe, expect, it } from "vitest";
import type { BootMenuEntry } from "../../../lib/types";
import { cmdlineError, entryErrors, fixDefault, move, newEntry, normalize, pathError, prefill, switchKind } from "./model";

const entry = (id: string, enabled = true): BootMenuEntry => ({ id, title: id, enabled, arch: "any", kind: { type: "exit" } });

describe("boot menu model", () => {
  it("移动条目且越界不变", () => {
    expect(move(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
    expect(move(["a", "b", "c"], 2, -2)).toEqual(["c", "a", "b"]);
    expect(move(["a", "b"], 0, -1)).toEqual(["a", "b"]);
  });

  it("cmdline 与路径白名单", () => {
    expect(cmdlineError("console=ttyS0,115200 quiet")).toBeUndefined();
    expect(cmdlineError("a;reboot")).toBeDefined();
    expect(cmdlineError("x".repeat(1025))).toBeDefined();
    expect(pathError("/boot/linux26")).toBeUndefined();
    expect(pathError("boot/linux26")).toBeDefined();
    expect(pathError("/boot/../etc")).toBeDefined();
  });

  it("条目校验：ID 重复、缺 ISO、chain URL", () => {
    expect(entryErrors(entry("a"), [entry("a")])["id"]).toBe("ID 已存在");
    expect(entryErrors({ ...entry("b"), kind: { type: "sanboot", iso: "" } }, [])["iso"]).toBeDefined();
    expect(entryErrors({ ...entry("c"), kind: { type: "chain", url: "ftp://x" } }, [])["url"]).toBeDefined();
    expect(entryErrors(entry("d"), [])).toEqual({});
  });

  it("切换类型保留 ISO，按识别结果预填", () => {
    expect(switchKind({ type: "sanboot", iso: "ab" }, "kernel_initrd")).toMatchObject({ iso: "ab", kernel_path: "" });
    expect(prefill("ab", { family: "pve", recommended: "kernel_initrd", kernel_path: "/boot/linux26", initrd_path: "/boot/initrd.img", embed_iso: true, iso_name: "proxmox.iso" })).toMatchObject({ type: "kernel_initrd", embed_iso: true });
    expect(prefill("ab", { family: "unknown" })).toEqual({ type: "sanboot", iso: "ab" });
  });

  it("默认值补齐、新条目 ID 不冲突、默认项随停用清空", () => {
    expect(normalize(null).timeout_s).toBe(10);
    expect(newEntry([entry("entry-1")]).id).toBe("entry-2");
    expect(fixDefault({ ...normalize(null), entries: [entry("a", false)], default_entry: "a" }).default_entry).toBeNull();
  });
});
