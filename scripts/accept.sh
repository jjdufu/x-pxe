#!/usr/bin/env bash
# 验收编排（Step 13）：scripts/accept.sh [选项] <场景…>|lab
#   场景：A lan-bios  B lan-uefi  C nic-matrix  D http-boot  E existing-dhcp  F redfish  G static-ip
#         H no-reinstall  I disk-select  J timeouts  K restart-resume  L rogue-dhcp  M offline
#         N abi-mismatch  O assets-rollback  P security  Q webui  R gates；lab = A–R
#   选项：--version <镜像标签>（默认 git describe --always）  --accel kvm|tcg（默认 kvm；宿主 KVM 不可用时用 tcg）
#         --fresh（清空被测部署的数据卷后重新初始化）  --keep（保留虚拟机）  --report <文件>
# 流程：lab/setup.sh 与 lab/net.sh up → docker compose（deploy/compose.yaml + compose.lab.yaml）部署被测镜像
#       → 经管理 API 初始化（改密、资产、实验室 Profile 与 deb13-init-scripts 计划、引导网络）→ 依次运行场景
#       → 每个场景输出 PASS/FAIL、耗时与日志目录（lab/out/accept/<场景>/：串口日志、br-lab 抓包、事件/审计导出）
#       → 汇总为 docs/acceptance/<日期>.md。
# 不在任何输出中打印 secret（初始密码、管理员密码、API 令牌、BMC 密码只存在 lab/work/accept/ 下的 0600 文件中）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "${ROOT}/lab/lib/common.sh"
XPXE_VERSION="$(cd "${ROOT}" && git describe --always)"
FRESH=0 REPORT=""
export KEEP=0 XPXE_LAB_ACCEL="${XPXE_LAB_ACCEL:-kvm}"
while [ $# -gt 0 ]; do
  case "$1" in
    --version) XPXE_VERSION="$2"; shift 2 ;;
    --accel) XPXE_LAB_ACCEL="$2"; shift 2 ;;
    --fresh) FRESH=1; shift ;;
    --keep) KEEP=1; shift ;;
    --report) REPORT="$2"; shift 2 ;;
    *) break ;;
  esac
done
[ $# -gt 0 ] || { sed -n '2,12p' "$0" >&2; exit 2; }
source "${ROOT}/lab/accept/lib.sh"
source "${ROOT}/lab/accept/scenarios.sh"

declare -A NAMES=([A]=lan-bios [B]=lan-uefi [C]=nic-matrix [D]=http-boot [E]=existing-dhcp [F]=redfish [G]=static-ip
  [H]=no-reinstall [I]=disk-select [J]=timeouts [K]=restart-resume [L]=rogue-dhcp [M]=offline [N]=abi-mismatch
  [O]=assets-rollback [P]=security [Q]=webui [R]=gates)
ids_of() { local a; for a in "$@"; do
  if [ "$a" = lab ]; then echo A B C D E F G H I J K L M N O P Q R; continue; fi
  local k found=""; for k in "${!NAMES[@]}"; do [ "$a" = "$k" ] || [ "$a" = "${NAMES[$k]}" ] && found="$k"; done
  [ -n "${found}" ] || die "未知场景 $a"; echo "${found}"
done; }
mapfile -t RUN < <(ids_of "$@" | tr ' ' '\n' | grep .)

log "被测镜像 x-pxe:${XPXE_VERSION}，加速 ${XPXE_LAB_ACCEL}，场景 ${RUN[*]}"
[ -x "${LAB_BIN}" ] || "${LAB_DIR}/setup.sh"
"${LAB_DIR}/net.sh" up
# 每次都按 coexist 重启现网 DHCP 替身：上一次运行（E 场景或 lab/run.sh manual-*）可能留下 product 配置
"${LAB_DIR}/dhcp.sh" start coexist
[ "${FRESH}" = 1 ] && deploy_wipe
deploy_up false
api_bootstrap
prepare_product

mkdir -p "${OUT}/accept"
SUMMARY="${OUT}/accept/summary.txt"; touch "${SUMMARY}"   # 分批运行时累加；报告取每个场景最近一次结果
for idx in "${!RUN[@]}"; do
  id="${RUN[idx]}"; next="${RUN[idx + 1]:-}"
  begin "${id}"
  case "${id}" in
    A|B) sc_lan "${id}" ;;
    *) "sc_${id}" ;;
  esac || check "场景脚本" 1 "场景函数异常退出（$?）"
  finish "${id}" | tee -a "${SUMMARY}"
  # 释放 tap 与 CPU。A/B 的虚拟机只在紧接着跑 B/H 时保留（它们占着 tap0–tap3，其他场景也要用）；
  # H 会按 vm.sh 记录的命令行把关掉的 A/B 虚拟机从已装好的盘重新开机
  case "${id}:${next}" in A:B|A:H|B:H) ;; *) vm_stop_all ;; esac
done

# ---------- 报告 ----------
DATE="$(date +%F)"
REPORT="${REPORT:-${ROOT}/docs/acceptance/${DATE}.md}"
mkdir -p "$(dirname "${REPORT}")"
{
  echo "# x-pxe 实验室验收报告 ${DATE}"
  echo
  echo "- 生成：scripts/accept.sh $*（$(date '+%F %T %Z')）"
  echo "- 提交：$(cd "${ROOT}" && git rev-parse --short HEAD)$(cd "${ROOT}" && git diff --quiet || echo '（工作树有未提交修改）')"
  echo "- 镜像：x-pxe:${XPXE_VERSION}，ID $(docker image inspect -f '{{.Id}}' "x-pxe:${XPXE_VERSION}")，大小 $(( $(docker image inspect -f '{{.Size}}' "x-pxe:${XPXE_VERSION}") / 1024 / 1024 )) MiB"
  echo "- 版本：$(docker run --rm "x-pxe:${XPXE_VERSION}" version 2>&1)"
  echo "- 宿主：$(uname -r)，Docker $(docker version -f '{{.Server.Version}}')，虚拟机加速 ${XPXE_LAB_ACCEL}"
  echo
  echo "| 场景 | 结果 | 耗时 | 日志目录 |"
  echo "| :--- | :--- | ---: | :--- |"
  for sid in A B C D E F G H I J K L M N O P Q R; do
    [ -s "${OUT}/accept/${sid}/result" ] || { echo "| ${sid} \`${NAMES[$sid]}\` | 未运行 | - | - |"; continue; }
    read -r _ verdict secs dir < "${OUT}/accept/${sid}/result"
    echo "| ${sid} \`${NAMES[$sid]}\` | ${verdict} | ${secs} | \`${dir}\` |"
  done
  echo
  echo "## 明细"
  echo
  echo '```'
  for sid in A B C D E F G H I J K L M N O P Q R; do if [ -s "${OUT}/accept/${sid}/result" ]; then cat "${OUT}/accept/${sid}/result"; fi; done
  echo '```'
} > "${REPORT}"
log "报告：${REPORT}"
for id in "${RUN[@]}"; do grep -q "^${id} PASS" "${OUT}/accept/${id}/result" || exit 1; done
