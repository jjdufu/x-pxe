#!/usr/bin/env bash
# “现网 DHCP”替身：lab/dhcp.sh start <existing|coexist|rogue|product>|stop [rogue]
# product：验收场景 E 把产品生成的 Kea 片段合并进 existing 后写到 lab/kea/product.json（不入库）。
# 容器 --network none，再把 veth 一端移入容器命名空间接入 br-lab。
source "$(dirname "$0")/lib/common.sh"

attach() { # <容器> <宿主侧 veth> <地址>
  local name="$1" host="$2" addr="$3" pid
  pid="$(docker inspect -f '{{.State.Pid}}' "${name}")"
  sudo ip link del "${host}" 2>/dev/null || true
  sudo ip link add "${host}" type veth peer name "vc-${host#vh-}"
  sudo ip link set "${host}" master "${BR}" up
  sudo ip link set "vc-${host#vh-}" netns "${pid}"
  sudo nsenter -t "${pid}" -n sh -c "ip link set vc-${host#vh-} name eth0; ip addr add ${addr}/24 dev eth0; ip link set eth0 up"
}

run_kea() { # <容器> <配置> <宿主侧 veth> <地址>
  docker rm -f "$1" >/dev/null 2>&1 || true
  docker run -d --name "$1" --network none --cap-add NET_RAW \
    -v "${LAB_DIR}/kea/$2.json:/etc/kea/kea-dhcp4.conf:ro" xpxe-lab/kea >/dev/null
  attach "$1" "$3" "$4"
  log "$1 已启动（$2，$4）"
}

case "${1:-}" in
  start)
    case "${2:-}" in
      coexist|existing|product) run_kea lab-dhcp "$2" vh-dhcp 10.77.0.2 ;;
      rogue) run_kea lab-rogue rogue vh-rogue 10.77.0.3 ;;
      *) echo "用法：lab/dhcp.sh start <existing|coexist|rogue|product>" >&2; exit 2 ;;
    esac ;;
  stop)
    if [ "${2:-}" = rogue ]; then names=(lab-rogue); else names=(lab-dhcp lab-rogue); fi
    for n in "${names[@]}"; do docker rm -f "${n}" >/dev/null 2>&1 || true; done
    for v in vh-dhcp vh-rogue; do
      docker ps -a --format '{{.Names}}' | grep -qx "lab-${v#vh-}" || sudo ip link del "${v}" 2>/dev/null || true
    done
    log "已停止：${names[*]}" ;;
  *) echo "用法：lab/dhcp.sh start <existing|coexist|rogue|product>|stop [rogue]" >&2; exit 2 ;;
esac
