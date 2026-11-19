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

# 实验室 APT 缓存容器 lab-acng。缓存目录挂在 ${WORK}/acng-cache（重建容器不丢缓存）。
# 可选实验室环境变量（不影响产品默认值与 CI）：
#   XPXE_LAB_APT_UPSTREAM=http://<内网镜像>/debian            deb.debian.org/debian 改从该镜像回源
#   XPXE_LAB_APT_SECURITY_UPSTREAM=http://<内网镜像>/debian-security   deb.debian.org/debian-security 同上
# 虚拟机侧的镜像源地址不变（10.77.0.1:3142/deb.debian.org/...），只改 acng 的回源。
acng_upstream() { [ -z "${XPXE_LAB_APT_UPSTREAM:-}${XPXE_LAB_APT_SECURITY_UPSTREAM:-}" ] || echo "${XPXE_LAB_APT_UPSTREAM:-}|${XPXE_LAB_APT_SECURITY_UPSTREAM:-}"; }
acng_current() { docker inspect -f '{{index .Config.Labels "xpxe.upstream"}}' lab-acng 2>/dev/null; }
acng_run() {
  local c="${WORK}/acng-cache" conf="${WORK}/acng.conf"
  mkdir -p "${c}"
  # 旧版本的容器把缓存放在容器内：重建前先拷出来
  if docker ps -a --format '{{.Names}}' | grep -qx lab-acng; then
    if ! docker inspect -f '{{range .Mounts}}{{.Destination}} {{end}}' lab-acng | grep -q /var/cache/apt-cacher-ng; then
      sudo docker cp lab-acng:/var/cache/apt-cacher-ng/. "${c}/" 2>/dev/null || true
    fi
    docker rm -f lab-acng >/dev/null 2>&1 || true
  fi
  # 回源改写：deb.debian.org/debian 走内置 Remap-debrep 的 backends_debian；debian-security 改写 Remap-secdeb 的后端。
  # 缓存键（debrep/、secdeb/）不变，已有缓存继续可用。
  local mounts=()
  if [ -n "${XPXE_LAB_APT_UPSTREAM:-}" ]; then
    echo "${XPXE_LAB_APT_UPSTREAM%/}/" > "${WORK}/acng-backends-debian"
    chmod a+r "${WORK}/acng-backends-debian"
    mounts+=(-v "${WORK}/acng-backends-debian:/etc/apt-cacher-ng/backends_debian:ro")
  fi
  if [ -n "${XPXE_LAB_APT_SECURITY_UPSTREAM:-}" ]; then
    docker run --rm --entrypoint cat xpxe-lab/acng /etc/apt-cacher-ng/acng.conf \
      | sed -E "s#^(Remap-secdeb: [^;]*;).*#\\1 ${XPXE_LAB_APT_SECURITY_UPSTREAM%/}/#" > "${conf}"
    chmod a+r "${conf}"
    mounts+=(-v "${conf}:/etc/apt-cacher-ng/acng.conf:ro")
  fi
  docker run -d --name lab-acng -p "${LAB_IP}:3142:3142" --label "xpxe.upstream=$(acng_upstream)" \
    -v "${c}:/var/cache/apt-cacher-ng" "${mounts[@]}" xpxe-lab/acng >/dev/null
  # acng 在容器内以 apt-cacher-ng 用户写缓存
  docker exec lab-acng sh -c 'chown -R apt-cacher-ng: /var/cache/apt-cacher-ng 2>/dev/null || true'
}
# 正在运行的 lab-acng 与当前回源设置一致时返回 0
acng_matches() { docker ps --format '{{.Names}}' | grep -qx lab-acng && [ "$(acng_current)" = "$(acng_upstream)" ]; }
