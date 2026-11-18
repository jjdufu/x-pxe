// 唯一允许调用 document/window.addEventListener 的文件（ESLint 强制）。在应用根组件挂载时注册一次、卸载时移除：
// ⌘K / Ctrl+K 打开命令面板；滚动时收起右键菜单（passive）。对应参考实现 app.js 末尾的单次全局绑定。
import { dismissOverlays } from "../lib/overlay-bus";

export interface GlobalHandlers {
  openPalette: () => void;
}

export function registerGlobalEvents(handlers: GlobalHandlers): () => void {
  const onKeyDown = (event: KeyboardEvent) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      handlers.openPalette();
    }
  };
  const onScroll = () => {
    dismissOverlays();
  };
  document.addEventListener("keydown", onKeyDown);
  window.addEventListener("scroll", onScroll, { passive: true });
  return () => {
    document.removeEventListener("keydown", onKeyDown);
    window.removeEventListener("scroll", onScroll);
  };
}
