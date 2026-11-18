// 路由表：外壳内的全部页面（Step 11 为占位页，Step 12 替换为业务页面）。
import { createBrowserRouter, type RouteObject } from "react-router";
import { AppShell } from "./AppShell";
import { EndpointPage } from "./pages/EndpointPage";
import { NodesPage } from "./pages/NodesPage";

export const routes: RouteObject[] = [
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <EndpointPage title="总览" description="系统信息与运行概况。" endpoint="/system/info" queryKey={["system"]} emptyTitle="暂无系统信息" emptyDescription="控制面未返回系统信息。" /> },
      { path: "nodes", element: <NodesPage /> },
      { path: "profiles", element: <EndpointPage title="装机 Profile" description="装机参数版本链。" endpoint="/configs/profile" queryKey={["configs", "profile"]} emptyTitle="还没有 Profile" emptyDescription="新建一个装机 Profile 以开始。" /> },
      { path: "init-plans", element: <EndpointPage title="初始化计划" description="装机后的初始化模块组合。" endpoint="/configs/init_plan" queryKey={["configs", "init_plan"]} emptyTitle="还没有初始化计划" emptyDescription="新建一个初始化计划以开始。" /> },
      { path: "boot-network", element: <EndpointPage title="引导网络" description="bootnet 健康与指令版本。" endpoint="/bootnet/status" queryKey={["bootnet", "status"]} emptyTitle="bootnet 未上报" emptyDescription="尚未收到 bootnet 心跳。" /> },
      { path: "assets", element: <EndpointPage title="资产" description="iPXE 与 Debian netboot 资产。" endpoint="/assets" queryKey={["assets"]} emptyTitle="还没有资产" emptyDescription="获取或上传第一份资产。" /> },
      { path: "history", element: <EndpointPage title="配置历史" description="所有配置对象的版本链。" endpoint="/configs/profile" queryKey={["configs", "profile", "history"]} emptyTitle="暂无历史" emptyDescription="配置变更后会出现在这里。" /> },
      { path: "audit", element: <EndpointPage title="审计日志" description="审计哈希链校验。" endpoint="/audit/verify" queryKey={["audit", "verify"]} emptyTitle="暂无审计记录" emptyDescription="操作发生后会出现在这里。" /> },
      { path: "settings", element: <EndpointPage title="设置" description="我的账户与外观。" endpoint="/me" queryKey={["me"]} emptyTitle="未登录" emptyDescription="登录后显示账户信息。" /> },
    ],
  },
];

export function createRouter() {
  return createBrowserRouter(routes);
}
