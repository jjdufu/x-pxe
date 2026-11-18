// 全局“收起浮层”信号：由 app/global-events.ts 的单次滚动监听触发，右键菜单等订阅后自行关闭。
type Listener = () => void;
const listeners = new Set<Listener>();

export function onDismissOverlays(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function dismissOverlays(): void {
  for (const listener of listeners) listener();
}
