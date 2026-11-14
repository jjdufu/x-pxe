#!/usr/bin/env bash
# 实验室脚本公共定义。被 setup/net/dhcp/vm/run.sh source。
set -euo pipefail
LAB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_DIR="$(cd "${LAB_DIR}/.." && pwd)"
WORK="${LAB_DIR}/work"
OUT="${LAB_DIR}/out"
BR=br-lab
LAB_IP=10.77.0.1
LAB_CIDR=10.77.0.1/24
TAPS=(tap0 tap1 tap2 tap3)
# 不接入网桥的孤立 tap：N2 双网卡场景的第一块网卡（没有 DHCP、没有出口）
TAP_ISOLATED=tap-iso
LAB_BIN_DIR=/opt/xpxe-lab/bin
LAB_BIN="${LAB_BIN_DIR}/xpxe-lab"
# 产品容器的非 root UID，H14 用它运行 proxydhcp/tftp
PROBE_UID=10001
mkdir -p "${WORK}" "${OUT}"

log() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*" >&2; }
die() { log "错误：$*"; exit 1; }

# /dev/kvm 的属组（本机为 kvm；规划时观察到的是 pulse-access）
kvm_group() { stat -c %G /dev/kvm; }

# 以 KVM 属组身份运行命令（当前会话未重新登录时组成员关系尚未生效）
as_kvm() { sg "$(kvm_group)" -c "$*"; }

# 后台服务的 pid 文件与日志
svc_pid() { echo "${WORK}/svc-$1.pid"; }
svc_log() { echo "${WORK}/svc-$1.log"; }
svc_running() { local p; p="$(svc_pid "$1")"; [ -f "$p" ] && sudo kill -0 "$(cat "$p")" 2>/dev/null; }
svc_stop() {
  local p; p="$(svc_pid "$1")"
  if [ -f "$p" ]; then sudo kill "$(cat "$p")" 2>/dev/null || true; rm -f "$p"; fi
}
