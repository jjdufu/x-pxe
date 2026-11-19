import { defineConfig, devices } from "@playwright/test";

// 浏览器用例（web-gate 第 6–10 项）：在组件样例构建（dist-gallery，含生产入口 index.html 与 gallery.html）上运行，全部离线。
// 联调（step-12 §5）：命令行带 integration（`npx playwright test e2e/integration.spec.ts`）时改为启动真实控制面联调栈
// （`cargo run -p xpxe-e2e --bin webui-stack`，托管 web/dist），默认运行忽略联调用例。
// 配置在 worker 进程中会再求值一次（其 argv 不含测试路径），故由主进程写入环境变量传递。
const integration = process.env.XPXE_INTEGRATION === "1" || process.argv.some((arg) => arg.includes("integration"));
if (integration) process.env.XPXE_INTEGRATION = "1";

export default defineConfig({
  testDir: "e2e",
  fullyParallel: !integration,
  forbidOnly: true,
  retries: 0,
  workers: integration ? 1 : process.env.CI === undefined ? 4 : 2,
  reporter: [["list"]],
  timeout: integration ? 300_000 : 60_000,
  ...(integration ? {} : { testIgnore: "integration.spec.ts" }),
  ...(integration ? { expect: { timeout: 20_000 } } : {}),
  use: {
    ...(integration ? { actionTimeout: 20_000 } : {}),
    baseURL: integration ? "http://127.0.0.1:8088" : "http://127.0.0.1:4173",
    trace: "off",
    screenshot: integration ? "only-on-failure" : "off",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], ...(integration ? { viewport: { width: 1440, height: 900 } } : {}) } }],
  webServer: integration
    ? {
        command: "npm run build && cd .. && cargo build -q -p xpxe-lab && cargo run -q -p xpxe-e2e --bin webui-stack -- --dir target/webui-stack-it --web web/dist",
        url: "http://127.0.0.1:8088/",
        reuseExistingServer: false,
        timeout: 900_000,
      }
    : {
        command: "npx vite preview --outDir dist-gallery --host 127.0.0.1 --port 4173 --strictPort",
        url: "http://127.0.0.1:4173/gallery.html",
        reuseExistingServer: false,
        timeout: 60_000,
      },
});
