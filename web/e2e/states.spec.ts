// 门禁第 9 项：每个页面用路由拦截注入 Populated/Empty/Loading/Error，断言各态表现并保存截图；错误态“重新加载”可恢复。
import { expect, mockApi, PAGES, test, type ApiState } from "./fixtures";

const STATES: ApiState[] = ["populated", "empty", "loading", "error"];

for (const item of PAGES) {
  for (const state of STATES) {
    test(`${item.name} ${item.path}：${state}`, async ({ page, external }, info) => {
      await mockApi(page, state, item.endpoint);
      await page.goto(item.path);
      const main = page.locator("main#main");
      if (state === "loading") {
        await expect(main.locator(".skeleton").first()).toBeVisible();
      } else if (state === "error") {
        await expect(main.getByRole("alert")).toContainText("INTERNAL");
        await expect(main.getByRole("button", { name: "重新加载" })).toBeVisible();
      } else if (state === "empty") {
        await expect(main.locator(".empty-state")).toBeVisible();
      } else {
        await expect(main.locator(".skeleton")).toHaveCount(0);
        await expect(main.locator(".empty-state, [role=alert]")).toHaveCount(0);
      }
      await page.screenshot({ path: info.outputPath(`${item.path === "/" ? "overview" : item.path.slice(1)}-${state}.png`) });
      expect(external).toHaveLength(0);
    });
  }
}

test("错误态“重新加载”调用 refetch 并恢复", async ({ page, external }) => {
  let fail = true;
  await mockApi(page);
  await page.route("**/api/v1/nodes", async (route) => {
    if (fail) {
      await route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ error: { code: "UNAVAILABLE", message: "暂不可用" } }) });
      return;
    }
    await route.fallback();
  });
  await page.goto("/nodes");
  await expect(page.getByRole("alert")).toContainText("UNAVAILABLE");
  fail = false;
  await page.getByRole("button", { name: "重新加载" }).click();
  await expect(page.getByRole("table", { name: "节点" })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(external).toHaveLength(0);
});
