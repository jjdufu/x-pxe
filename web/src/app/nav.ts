// 导航结构（step-12 第 2 节）。
import { Activity, Boxes, FileClock, Gauge, HardDrive, Network, ScrollText, Server, Settings, Workflow, type LucideIcon } from "lucide-react";

export interface NavItem {
  path: string;
  label: string;
  icon: LucideIcon;
}

export interface NavGroup {
  label: string;
  items: NavItem[];
}

export const NAV: NavGroup[] = [
  {
    label: "运行",
    items: [
      { path: "/", label: "总览", icon: Gauge },
      { path: "/nodes", label: "节点", icon: Server },
    ],
  },
  {
    label: "配置",
    items: [
      { path: "/profiles", label: "装机 Profile", icon: HardDrive },
      { path: "/init-plans", label: "初始化计划", icon: Workflow },
      { path: "/boot-network", label: "引导网络", icon: Network },
      { path: "/assets", label: "资产", icon: Boxes },
    ],
  },
  {
    label: "追溯",
    items: [
      { path: "/history", label: "配置历史", icon: FileClock },
      { path: "/audit", label: "审计日志", icon: ScrollText },
    ],
  },
  {
    label: "系统",
    items: [{ path: "/settings", label: "设置", icon: Settings }],
  },
];

export const ALL_PAGES: NavItem[] = NAV.flatMap((group) => group.items);

export function pageLabel(path: string): string {
  const exact = ALL_PAGES.find((item) => item.path === path);
  if (exact !== undefined) return exact.label;
  const prefix = ALL_PAGES.find((item) => item.path !== "/" && path.startsWith(`${item.path}/`));
  return prefix?.label ?? "页面";
}

export const STATUS_ICON = Activity;
