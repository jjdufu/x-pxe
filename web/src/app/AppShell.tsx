// 应用外壳：取自参考实现 index.html 与 styles.css 第 2 节（.app-shell、.sidebar、.topbar、.canvas、移动端离屏抽屉）。
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { LogOut, Menu as MenuIcon, Moon, Palette, Search, Sun } from "lucide-react";
import { Suspense, useEffect, useMemo, useState } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router";
import { Badge } from "../components/ui/feedback";
import { CommandPalette, type Command } from "../components/ui/command-palette";
import { api } from "../lib/api";
import { onUnauthorized } from "../lib/auth-bus";
import { cx } from "../lib/cx";
import type { NodeRow } from "../lib/types";
import { DefaultSkeleton } from "./AsyncView";
import { ForcedPasswordDialog } from "./PasswordForm";
import { useTheme } from "../theme/ThemeProvider";
import { ThemeEditor } from "../theme/ThemeEditor";
import { registerGlobalEvents } from "./global-events";
import { ALL_PAGES, NAV, pageLabel } from "./nav";
import { useEventStream } from "./use-event-stream";

interface Me {
  user: { username: string; role: string; must_change_password?: boolean };
}

interface BootnetStatus {
  healthy: boolean;
}

function SidebarFooter() {
  const me = useQuery({ queryKey: ["me"], queryFn: () => api<Me | null>("/me"), retry: false });
  const navigate = useNavigate();
  const client = useQueryClient();
  const bootnet = useQuery({ queryKey: ["bootnet", "status"], queryFn: () => api<BootnetStatus | null>("/bootnet/status"), retry: false });
  const user = me.data?.user;
  const healthy = bootnet.data?.healthy;
  return (
    <div className="sidebar-footer">
      <div className="sidebar-user">
        <span className="ui-label">{user?.username ?? "未登录"}</span>
        {user !== undefined && <span className="muted-text">{user.role}</span>}
        {user !== undefined && (
          <button
            type="button"
            className="icon-btn"
            aria-label="退出登录"
            onClick={() => {
              void api("/auth/logout", { method: "POST" })
                .catch(() => undefined)
                .then(() => {
                  client.clear();
                  void navigate("/login");
                });
            }}
          >
            <LogOut size={14} aria-hidden="true" />
          </button>
        )}
      </div>
      <Badge tone={healthy === undefined ? "outline" : healthy ? "success" : "danger"} dot>
        bootnet {healthy === undefined ? "未知" : healthy ? "健康" : "异常"}
      </Badge>
    </div>
  );
}

export function AppShell() {
  const { mode, toggleMode, setEditorOpen } = useTheme();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [navOpen, setNavOpen] = useState(false);
  const location = useLocation();
  const navigate = useNavigate();
  const stream = useEventStream();
  const me = useQuery({ queryKey: ["me"], queryFn: () => api<Me | null>("/me"), retry: false });
  const nodes = useQuery({ queryKey: ["nodes"], queryFn: () => api<NodeRow[]>("/nodes"), enabled: paletteOpen });

  useEffect(
    () =>
      registerGlobalEvents({
        openPalette: () => {
          setPaletteOpen(true);
        },
      }),
    [],
  );

  useEffect(
    () =>
      onUnauthorized(() => {
        // 已在登录页时不再跳转（避免 next 嵌套）
        if (window.location.pathname === "/login") return;
        const here = `${window.location.pathname}${window.location.search}`;
        void navigate(`/login?next=${encodeURIComponent(here)}`, { replace: true });
      }),
    [navigate],
  );

  const commands = useMemo<Command[]>(
    () => [
      ...ALL_PAGES.map((page) => ({
        id: `go:${page.path}`,
        label: page.label,
        group: "跳转页面",
        keywords: page.path,
        run: () => {
          void navigate(page.path);
        },
      })),
      { id: "theme:toggle", label: mode === "dark" ? "切换到浅色模式" : "切换到深色模式", group: "主题", run: toggleMode },
      {
        id: "theme:editor",
        label: "打开 Theme Editor",
        group: "主题",
        run: () => {
          setEditorOpen(true);
        },
      },
      ...(nodes.data ?? []).flatMap((node) => [
        {
          id: `node:${String(node.id)}`,
          label: node.hostname,
          group: "节点",
          keywords: node.mac,
          hint: <span className="mono tabular muted-text">{node.mac}</span>,
          run: () => {
            void navigate(`/nodes?node=${String(node.id)}`);
          },
        },
        ...(["idle", "failed"].includes(node.state)
          ? [
              {
                id: `arm:${String(node.id)}`,
                label: `arm ${node.hostname}`,
                group: "节点操作",
                keywords: node.mac,
                hint: <kbd>arm</kbd>,
                run: () => {
                  void navigate(`/nodes?node=${String(node.id)}&action=arm`);
                },
              },
            ]
          : []),
      ]),
    ],
    [mode, navigate, setEditorOpen, toggleMode, nodes.data],
  );

  const closeNav = () => {
    setNavOpen(false);
  };

  return (
    <div className="app-shell">
      <aside className={cx("sidebar", navOpen && "open")} aria-label="主导航">
        <div className="sidebar-header">
          <div className="brand">
            <span className="brand-mark" aria-hidden="true">
              x
            </span>
            <div>
              <div className="brand-title">x-pxe</div>
              <div className="brand-sub">Debian 13 自动装机</div>
            </div>
          </div>
        </div>
        <button
          type="button"
          className="cmd-trigger"
          onClick={() => {
            setPaletteOpen(true);
          }}
        >
          <span className="cmd-trigger-left">
            <Search size={14} aria-hidden="true" />
            搜索或跳转
          </span>
          <kbd>⌘K</kbd>
        </button>
        <nav className="sidebar-nav">
          {NAV.map((group) => (
            <div key={group.label}>
              <div className="nav-group-label">{group.label}</div>
              {group.items.map((item) => (
                <NavLink key={item.path} to={item.path} end={item.path === "/"} className={({ isActive }) => cx("nav-item", isActive && "active")} onClick={closeNav}>
                  <span className="nav-item-left">
                    <item.icon size={15} aria-hidden="true" />
                    <span className="nav-name">{item.label}</span>
                  </span>
                </NavLink>
              ))}
            </div>
          ))}
        </nav>
        <SidebarFooter />
      </aside>
      <div className={cx("sidebar-backdrop", navOpen && "open")} onClick={closeNav} aria-hidden="true" />
      <div className="main-wrap">
        <header className="topbar">
          <div className="topbar-left">
            <button
              type="button"
              className="icon-btn mobile-only"
              aria-label="打开导航"
              aria-expanded={navOpen}
              onClick={() => {
                setNavOpen(true);
              }}
            >
              <MenuIcon size={16} aria-hidden="true" />
            </button>
            <div className="breadcrumb">
              x-pxe / <strong>{pageLabel(location.pathname)}</strong>
            </div>
            {stream !== "open" && (
              <Badge tone="warn" dot>
                <span className="hide-sm">实时推送</span>
                {stream === "connecting" ? "连接中" : "已断开"}
              </Badge>
            )}
          </div>
          <div className="topbar-actions">
            <span className="divider-v" aria-hidden="true" />
            <button
              type="button"
              className="ui-btn ui-btn-ghost ui-btn-sm"
              onClick={() => {
                setEditorOpen(true);
              }}
            >
              <Palette size={14} aria-hidden="true" />
              <span className="hide-sm">主题</span>
            </button>
            <button type="button" className="ui-btn ui-btn-ghost ui-btn-sm" onClick={toggleMode} aria-label={mode === "dark" ? "切换到浅色模式" : "切换到深色模式"}>
              {mode === "dark" ? <Sun size={14} aria-hidden="true" /> : <Moon size={14} aria-hidden="true" />}
              <span className="hide-sm">{mode === "dark" ? "浅色" : "深色"}</span>
            </button>
          </div>
        </header>
        <main className="canvas" id="main">
          {/* 强制改密期间不渲染页面（其接口会返回 403） */}
          {me.data?.user.must_change_password === true ? null : (
            <Suspense fallback={<DefaultSkeleton />}>
              <Outlet />
            </Suspense>
          )}
        </main>
      </div>
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} commands={commands} />
      <ThemeEditor />
      <ForcedPasswordDialog open={me.data?.user.must_change_password === true} />
    </div>
  );
}
