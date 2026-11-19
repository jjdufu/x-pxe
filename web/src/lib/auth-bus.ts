// 未登录信号：任何请求收到 401 时广播，外壳订阅后跳转登录页（不在 api.ts 内直接操纵路由）。
type Listener = () => void;
const listeners = new Set<Listener>();

export function onUnauthorized(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function signalUnauthorized(): void {
  for (const listener of listeners) listener();
}
