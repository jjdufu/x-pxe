// 唯一允许访问 navigator.clipboard 的文件（ESLint 强制）。
// 内网 HTTP 下 navigator.clipboard 常不存在：降级为隐藏 textarea + execCommand('copy')（参考实现 app.js copyText）。
export type CopyResult = "clipboard" | "fallback" | "failed";

function legacyCopy(text: string): boolean {
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.top = "-1000px";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  let ok: boolean;
  try {
    // execCommand 已废弃，但仍是非安全上下文（内网 HTTP）下唯一可用的复制方式
    const legacy: { execCommand: (command: string) => boolean } = document;
    ok = legacy.execCommand("copy");
  } catch {
    ok = false;
  }
  area.remove();
  return ok;
}

/** 复制文本：优先异步剪贴板 API，失败或不存在时降级；都失败返回 "failed"（调用方提示手动复制）。 */
export async function copyText(text: string): Promise<CopyResult> {
  const clipboard = typeof navigator === "undefined" ? undefined : (navigator.clipboard as Clipboard | undefined);
  if (clipboard !== undefined && typeof clipboard.writeText === "function") {
    try {
      await clipboard.writeText(text);
      return "clipboard";
    } catch {
      // 继续降级
    }
  }
  return legacyCopy(text) ? "fallback" : "failed";
}
