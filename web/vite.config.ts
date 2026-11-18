import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

// Tailwind 的版权横幅带官网地址；保留许可声明、去掉 URL，满足“产物不含外链”的扫描规则。
function stripBannerUrl(): Plugin {
  return {
    name: "xpxe-strip-banner-url",
    generateBundle(_options, bundle) {
      for (const asset of Object.values(bundle)) {
        if (asset.type === "asset" && asset.fileName.endsWith(".css") && typeof asset.source === "string") {
          asset.source = asset.source.replace(/(\/\*![^*]*?MIT License) \| https?:\/\/[^\s*]+/gu, "$1");
        }
      }
    },
  };
}

// 生产构建只含 index.html；`--mode gallery` 额外打包组件样例页到 dist-gallery（不进入生产产物）。
export default defineConfig(({ mode }) => {
  const gallery = mode === "gallery";
  return {
    plugins: [react(), tailwindcss(), stripBannerUrl()],
    build: {
      outDir: gallery ? "dist-gallery" : "dist",
      sourcemap: false,
      rolldownOptions: { input: gallery ? { main: "index.html", gallery: "gallery.html" } : { main: "index.html" } },
    },
    test: { environment: "jsdom", include: ["src/**/*.test.{ts,tsx}", "scripts/**/*.test.mjs"], setupFiles: ["src/test-setup.ts"] },
  };
});
