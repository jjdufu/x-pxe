#!/usr/bin/env bash
# Web 门禁：本地与 CI 跑同一份，任一步失败即失败。
set -euo pipefail
cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)/web"

npm ci --no-audit --no-fund
npm run typecheck
npm run lint -- --max-warnings 0
npm run test
npm run build
node scripts/scan-dist.mjs

echo "web-gate: 全部通过"
