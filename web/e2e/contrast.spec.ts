// 门禁第 7 项：样例页在 4 灰阶 × 2 模式 × 7 Accent × 圆角 {0, 6, 99} 下运行 axe 颜色对比度检查；
// 默认主题的两种模式另跑完整 WCAG 2 A/AA 规则。
import AxeBuilder from "@axe-core/playwright";
import { expect, mockApi, test } from "./fixtures";

const GRAYS = ["zinc", "neutral", "slate", "sand"];
const MODES = ["light", "dark"];
const ACCENTS = ["neutral", "slate", "blue", "emerald", "violet", "amber", "rose"];
const RADII = [0, 6, 99];

for (const gray of GRAYS) {
  for (const mode of MODES) {
    test(`对比度 ${gray}/${mode}（7 Accent × 3 圆角）`, async ({ page, external }) => {
      test.setTimeout(180_000);
      await mockApi(page);
      const failures: string[] = [];
      for (const accent of ACCENTS) {
        for (const radius of RADII) {
          await page.goto(`/gallery.html?gray=${gray}&mode=${mode}&accent=${accent}&radius=${String(radius)}`);
          await expect(page.getByRole("heading", { name: "组件样例" })).toBeVisible();
          const result = await new AxeBuilder({ page }).withRules(["color-contrast", "link-in-text-block"]).analyze();
          for (const violation of result.violations) {
            for (const node of violation.nodes) failures.push(`${accent}/${String(radius)} ${violation.id} ${node.target.join(" ")} ${node.failureSummary ?? ""}`);
          }
        }
      }
      expect(failures).toEqual([]);
      expect(external).toHaveLength(0);
    });
  }
}

for (const mode of MODES) {
  test(`完整 WCAG 2 A/AA：样例页与应用外壳（${mode}）`, async ({ page, external }) => {
    await mockApi(page);
    for (const path of [`/gallery.html?mode=${mode}`, "/nodes"]) {
      if (path === "/nodes") {
        await page.addInitScript((theme) => {
          window.localStorage.setItem("xpxe.theme.v1", JSON.stringify({ v: 1, mode: theme, gray: "zinc", radius: 6, accent: { kind: "preset", name: "neutral" }, applied: { light: { accent: "#18181b", accentFg: "#ffffff" }, dark: { accent: "#fafafa", accentFg: "#09090b" } } }));
        }, mode);
      }
      await page.goto(path);
      await page.waitForLoadState("networkidle");
      const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
      const summary = result.violations.map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(" | ")}`);
      expect(summary, path).toEqual([]);
    }
    expect(external).toHaveLength(0);
  });
}
