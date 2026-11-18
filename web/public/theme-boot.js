// 防闪烁：React 挂载前同步应用已保存的主题（CSP 不允许内联脚本，故为同源外部文件）。
// 与 src/theme/state.ts 的 StoredTheme 结构一致；任何异常都退回默认主题。
(function () {
  var root = document.documentElement;
  var read = function () {
    try {
      return JSON.parse(window.localStorage.getItem("xpxe.theme.v1") || "null");
    } catch {
      return null;
    }
  };
  var prefersDark = function () {
    try {
      return window.matchMedia("(prefers-color-scheme: dark)").matches;
    } catch {
      return false;
    }
  };
  var stored = read();
  var dark = prefersDark();
  if (!stored || stored.v !== 1) {
    root.setAttribute("data-theme", dark ? "dark" : "light");
    root.setAttribute("data-gray", "zinc");
    return;
  }
  var mode = stored.mode === "system" ? (dark ? "dark" : "light") : stored.mode;
  root.setAttribute("data-theme", mode === "dark" ? "dark" : "light");
  root.setAttribute("data-gray", String(stored.gray || "zinc"));
  if (typeof stored.radius === "number") root.style.setProperty("--radius-md", stored.radius + "px");
  var applied = stored.applied && stored.applied[mode === "dark" ? "dark" : "light"];
  if (applied && /^#[0-9a-f]{6}$/i.test(applied.accent) && /^#[0-9a-f]{6}$/i.test(applied.accentFg)) {
    root.style.setProperty("--accent", applied.accent);
    root.style.setProperty("--accent-fg", applied.accentFg);
  }
})();
