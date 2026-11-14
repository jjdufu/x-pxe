#!/usr/bin/env bash
# 隔离实验网：lab/net.sh up|down|selftest
# br-lab 不接任何物理网卡、不做 NAT；FORWARD 最前面两条规则阻断进出实验网的转发
# （Docker 发布到 10.77.0.1 的端口经 DNAT 进入容器，被 DNAT 例外放行）。
source "$(dirname "$0")/lib/common.sh"
RULE_OUT=(FORWARD -i "${BR}" ! -o "${BR}" -m conntrack ! --ctstate DNAT -j DROP)
RULE_IN=(FORWARD -o "${BR}" ! -i "${BR}" -m conntrack ! --ctstate RELATED,ESTABLISHED -j DROP)
BEFORE="${WORK}/net-before"

state() { ip -br link | awk '{print $1}' | sort; echo ---; sudo iptables -S FORWARD; }

up() {
  if ip link show "${BR}" >/dev/null 2>&1; then log "${BR} 已存在"; return 0; fi
  state > "${BEFORE}"
  sudo ip link add "${BR}" type bridge stp_state 0 forward_delay 0
  sudo ip addr add "${LAB_CIDR}" dev "${BR}"
  sudo ip link set "${BR}" up
  for t in "${TAPS[@]}"; do
    sudo ip tuntap add "${t}" mode tap user "$(id -un)"
    sudo ip link set "${t}" master "${BR}" up
  done
  sudo ip tuntap add "${TAP_ISOLATED}" mode tap user "$(id -un)"
  sudo ip link set "${TAP_ISOLATED}" up
  sudo iptables -C "${RULE_OUT[@]}" 2>/dev/null || sudo iptables -I "${RULE_OUT[@]:0:1}" 1 "${RULE_OUT[@]:1}"
  sudo iptables -C "${RULE_IN[@]}" 2>/dev/null || sudo iptables -I "${RULE_IN[@]:0:1}" 1 "${RULE_IN[@]:1}"
  # 桥内二层流量即便经过 iptables（br_netfilter 已加载时）也只匹配 -i br-lab -o br-lab，
  # 不会被上面两条规则丢弃，因此不修改全局 sysctl bridge-nf-call-iptables。
  log "${BR} up（${LAB_CIDR}，${TAPS[*]} + 孤立 ${TAP_ISOLATED}）"
}

down() {
  sudo ip netns del ns-probe 2>/dev/null || true
  for t in "${TAPS[@]}" "${TAP_ISOLATED}"; do sudo ip link del "${t}" 2>/dev/null || true; done
  for v in $(ip -br link | awk '{print $1}' | sed 's/@.*//' | grep -E '^vh-' || true); do sudo ip link del "${v}" 2>/dev/null || true; done
  sudo ip link del "${BR}" 2>/dev/null || true
  while sudo iptables -C "${RULE_OUT[@]}" 2>/dev/null; do sudo iptables -D "${RULE_OUT[@]}"; done
  while sudo iptables -C "${RULE_IN[@]}" 2>/dev/null; do sudo iptables -D "${RULE_IN[@]}"; done
  # 本实验室负责的对象：br-lab、tap*、vh-*、两条隔离规则。宿主上其他 Docker 栈的 veth 会在实验室运行期间自行增减，不纳入成败判定。
  local ok=0
  if ip link show "${BR}" >/dev/null 2>&1; then log "FAIL：${BR} 仍存在"; ok=1; else log "已删除 ${BR}"; fi
  for t in "${TAPS[@]}" "${TAP_ISOLATED}"; do
    if ip link show "${t}" >/dev/null 2>&1; then log "FAIL：${t} 仍存在"; ok=1; fi
  done
  if sudo iptables -C "${RULE_OUT[@]}" 2>/dev/null || sudo iptables -C "${RULE_IN[@]}" 2>/dev/null; then
    log "FAIL：隔离规则仍在 FORWARD 中"; ok=1
  else
    log "隔离规则已从 FORWARD 拆除"
  fi
  return "${ok}"
}

# 在 ns-probe 中准备探测端（10.77.0.9，默认路由指向 10.77.0.1，以便真正尝试转发）
probe_ns() {
  ip netns list | grep -qw ns-probe && return 0
  sudo ip netns add ns-probe
  sudo ip link add vh-probe type veth peer name vp-probe
  sudo ip link set vh-probe master "${BR}" up
  sudo ip link set vp-probe netns ns-probe
  sudo ip netns exec ns-probe sh -c "ip link set lo up; ip link set vp-probe up; ip addr add 10.77.0.9/24 dev vp-probe; ip route add default via ${LAB_IP}"
}

selftest() {
  probe_ns
  local ok=0
  local masters; masters="$(ip -br link show master "${BR}" | awk '{print $1}' | sed 's/@.*//' | tr '\n' ' ')"
  log "br-lab 成员：${masters}"
  for m in ${masters}; do
    case "${m}" in tap[0-9]|vh-*) ;; *) log "FAIL：br-lab 上出现非实验接口 ${m}"; ok=1 ;; esac
  done
  command -v ping >/dev/null || die "缺少 ping（iputils-ping），自检无法判定"
  # 对照组：宿主自身能访问外网，证明下面的“不通”来自隔离规则而不是宿主没有外网
  if sudo ping -c1 -W2 1.1.1.1 >/dev/null 2>&1 && curl -s -m 5 -o /dev/null http://1.1.1.1/; then
    log "对照：宿主能访问 1.1.1.1（ICMP 与 HTTP）"
  else
    log "FAIL：宿主自身无法访问 1.1.1.1，隔离结论无法判定"; ok=1
  fi
  if sudo ip netns exec ns-probe ping -c1 -W2 1.1.1.1 >/dev/null 2>&1; then log "FAIL：ns-probe 能 ping 1.1.1.1"; ok=1; else log "PASS：ns-probe 无法 ping 1.1.1.1"; fi
  if sudo ip netns exec ns-probe curl -s -m 5 -o /dev/null http://1.1.1.1/; then log "FAIL：ns-probe 能访问 http://1.1.1.1"; ok=1; else log "PASS：ns-probe 无法访问 http://1.1.1.1"; fi
  if sudo ip netns exec ns-probe ping -c1 -W2 "${LAB_IP}" >/dev/null 2>&1; then log "PASS：ns-probe 能访问 ${LAB_IP}"; else log "FAIL：ns-probe 无法访问 ${LAB_IP}"; ok=1; fi
  if docker ps --format '{{.Names}}' | grep -qx lab-acng; then
    if sudo ip netns exec ns-probe curl -s -m 5 -o /dev/null "http://${LAB_IP}:3142/acng-report.html"; then log "PASS：ns-probe 能访问 APT 缓存 ${LAB_IP}:3142（DNAT 例外）"; else log "FAIL：ns-probe 无法访问 ${LAB_IP}:3142"; ok=1; fi
  else
    log "FAIL：lab-acng 未运行，无法验证 DNAT 例外"; ok=1
  fi
  return "${ok}"
}

case "${1:-}" in
  up) up ;;
  down) down ;;
  selftest) selftest ;;
  probe-ns) probe_ns ;;
  *) echo "用法：lab/net.sh up|down|selftest" >&2; exit 2 ;;
esac
