// 路由表：登录页在外壳之外；外壳内各业务页面按路由懒加载（外壳内有 Suspense 骨架）。
import { lazy, Suspense } from "react";
import { createBrowserRouter, type RouteObject } from "react-router";
import { AppShell } from "./AppShell";

const OverviewPage = lazy(() => import("./pages/OverviewPage").then((module) => ({ default: module.OverviewPage })));
const NodesPage = lazy(() => import("./pages/nodes/NodesPage").then((module) => ({ default: module.NodesPage })));
const ConfigListPage = lazy(() => import("./pages/configs/ConfigListPage").then((module) => ({ default: module.ConfigListPage })));
const ProfileEditorPage = lazy(() => import("./pages/configs/ProfileEditor").then((module) => ({ default: module.ProfileEditorPage })));
const PlanEditorPage = lazy(() => import("./pages/configs/PlanEditor").then((module) => ({ default: module.PlanEditorPage })));
const BootNetworkPage = lazy(() => import("./pages/bootnet/BootNetworkPage").then((module) => ({ default: module.BootNetworkPage })));
const AssetsPage = lazy(() => import("./pages/assets/AssetsPage").then((module) => ({ default: module.AssetsPage })));
const HistoryPage = lazy(() => import("./pages/admin/HistoryPage").then((module) => ({ default: module.HistoryPage })));
const AuditPage = lazy(() => import("./pages/admin/AuditPage").then((module) => ({ default: module.AuditPage })));
const SettingsPage = lazy(() => import("./pages/admin/SettingsPage").then((module) => ({ default: module.SettingsPage })));
const LoginPage = lazy(() => import("./pages/LoginPage").then((module) => ({ default: module.LoginPage })));

const PROFILE_BLANK = {
  name: "",
  locale: "en_US.UTF-8",
  keymap: "us",
  timezone: "UTC",
  ntp: [],
  console: { items: ["tty0"] },
  mirror: { scheme: "http", host_port: "deb.debian.org", directory: "/debian", proxy: null },
  security: { type: "official" },
  suites: { updates: true, backports: false },
  components: { items: ["main"] },
  tasks: { items: ["ssh-server", "standard"] },
  accounts: { root: { type: "disabled" }, primary_user: null },
  disk: { prefer: "unique", size_gib: { min: 16, max: null }, exclude_models: [], include: [] },
  partitioning: { type: "atomic_lvm" },
  firmware_cpio: false,
  debug: { type: "off" },
  pkgsel_upgrade: "none",
  extra_repos: [],
};

export const routes: RouteObject[] = [
  {
    path: "/login",
    element: (
      <Suspense fallback={null}>
        <LoginPage />
      </Suspense>
    ),
  },
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <OverviewPage /> },
      { path: "nodes", element: <NodesPage /> },
      {
        path: "profiles",
        element: <ConfigListPage kind="profile" title="装机 Profile" description="Debian 安装参数（preseed）的版本链。" route="/profiles" emptyTitle="还没有 Profile" blank={PROFILE_BLANK} />,
      },
      { path: "profiles/:id", element: <ProfileEditorPage /> },
      {
        path: "init-plans",
        element: <ConfigListPage kind="init_plan" title="初始化计划" description="装机完成后由 agent 执行的初始化模块组合。" route="/init-plans" emptyTitle="还没有初始化计划" blank={{ name: "", modules: {} }} />,
      },
      { path: "init-plans/:id", element: <PlanEditorPage /> },
      { path: "boot-network", element: <BootNetworkPage /> },
      { path: "assets", element: <AssetsPage /> },
      { path: "history", element: <HistoryPage /> },
      { path: "audit", element: <AuditPage /> },
      { path: "settings", element: <SettingsPage /> },
    ],
  },
];

export function createRouter() {
  return createBrowserRouter(routes);
}
