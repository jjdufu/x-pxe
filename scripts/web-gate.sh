#!/usr/bin/env bash
# Web 门禁（docs/plan/step-11-webui-foundation.md 第 10 节）：本地与 CI 跑同一份，任一步失败即失败。
set -euo pipefail
cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)/web"

step() { printf '\n== web-gate %s ==\n' "$1"; }

step "0 依赖";        npm ci --no-audit --no-fund
step "1 类型";        npm run typecheck
step "2 代码规则";    npm run lint -- --max-warnings 0
step "3 单元测试";    npm run test
step "4 构建";        npm run build
step "5 产物扫描";    node scripts/scan-dist.mjs dist
step "7a 对比度数值"; node scripts/contrast.mjs
step "6–10 浏览器";
npm run build:gallery
if [[ "${CI:-}" == "true" ]]; then
  npx playwright install --with-deps chromium
else
  npx playwright install chromium
fi
# 离线（6）、axe 对比度矩阵（7）、键盘（8）、四态（9）、移动端（10）
npx playwright test

echo "web-gate: 全部通过"
