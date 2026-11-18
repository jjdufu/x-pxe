// 浮层挂载点：页面上用 Portal 挂到 body；在 Dialog/Drawer 内部改为就地渲染，避免被对话框焦点陷阱隔离。
// 定位统一用 fixed 策略，就地渲染时也不会被可滚动容器裁剪。
import { Portal } from "@ark-ui/react/portal";
import { createContext, useContext, type ReactNode } from "react";

export const InlineLayer = createContext(false);

export const FIXED = { strategy: "fixed", gutter: 4 } as const;

export function Layer({ children }: { children: ReactNode }) {
  const inline = useContext(InlineLayer);
  return <Portal disabled={inline}>{children}</Portal>;
}
