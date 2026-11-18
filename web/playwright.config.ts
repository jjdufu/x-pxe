import { defineConfig, devices } from "@playwright/test";

// 浏览器用例（web-gate 第 6–10 项）：在组件样例构建（dist-gallery，含生产入口 index.html 与 gallery.html）上运行，全部离线。
export default defineConfig({
  testDir: "e2e",
  fullyParallel: true,
  forbidOnly: true,
  retries: 0,
  workers: process.env.CI === undefined ? 4 : 2,
  reporter: [["list"]],
  timeout: 60_000,
  use: {
    baseURL: "http://127.0.0.1:4173",
    trace: "off",
    screenshot: "off",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "npx vite preview --outDir dist-gallery --host 127.0.0.1 --port 4173 --strictPort",
    url: "http://127.0.0.1:4173/gallery.html",
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
