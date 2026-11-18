#!/bin/bash
# x-pxe 初始化模块 sysctl：内核参数
set -euo pipefail

cat > /etc/sysctl.d/99-fleet-netvm.conf << 'XPXE_EOF'

# ------ 转发 / IPv6 ------
# K8s 节点需要转发；纯 nginx 开着也无妨
net.ipv4.ip_forward = 1
# 确认集群无依赖 IPv6 后再禁用；若有双栈 Service 不要用这三项
net.ipv6.conf.all.disable_ipv6 = 1
net.ipv6.conf.default.disable_ipv6 = 1
net.ipv6.conf.lo.disable_ipv6 = 1

# ------ TCP ------
net.ipv4.tcp_syncookies = 1
net.ipv4.tcp_tw_reuse = 1
net.ipv4.tcp_fin_timeout = 30
net.ipv4.tcp_syn_retries = 5
net.ipv4.tcp_synack_retries = 5
net.ipv4.tcp_fastopen = 3
net.ipv4.tcp_slow_start_after_idle = 0

# 空闲 10 分钟后探活；配合 intvl/probes，死连接收得比 7200 快（适合 K8s/反代）
net.ipv4.tcp_keepalive_time = 600
net.ipv4.tcp_keepalive_intvl = 30
net.ipv4.tcp_keepalive_probes = 5

# 缓冲：default 保持克制，max 允许大流量抬升（勿再把 default 调到数 MB）
net.ipv4.tcp_rmem = 4096 87380 12582912
net.ipv4.tcp_wmem = 4096 16384 12582912
net.core.rmem_default = 212992
net.core.wmem_default = 16384
net.core.rmem_max = 12582912
net.core.wmem_max = 12582912

net.ipv4.tcp_max_orphans = 262144
net.ipv4.tcp_max_syn_backlog = 32768
net.ipv4.tcp_max_tw_buckets = 1048576
net.ipv4.ip_local_port_range = 10000 65535

# 拥塞：维持发行版常见组合即可；公网入口若要试 BBR，再单独开下面两行并回归测试
# net.ipv4.tcp_congestion_control = bbr
# net.core.default_qdisc = fq
# 当前建议保持（若系统已是 cubic + fq_codel，不必强改）
# net.ipv4.tcp_congestion_control = cubic
# net.core.default_qdisc = fq_codel

# ------ 队列 ------
net.core.netdev_max_backlog = 32768
net.core.somaxconn = 32768

# ------ 内存 / VM（ES/Java/K8s 友好）------
vm.max_map_count = 262144
vm.dirty_background_ratio = 5
vm.dirty_ratio = 10
vm.overcommit_memory = 1
vm.swappiness = 10
vm.min_free_kbytes = 65536
vm.vfs_cache_pressure = 50

# ------ 文件监控（kubelet / 同步类）------
fs.fanotify.max_queued_events = 65536
fs.inotify.max_user_watches = 524288

# ------ 安全（与 Calico/K8s 兼容）------
net.ipv4.conf.all.accept_redirects = 0
net.ipv4.conf.default.accept_redirects = 0
net.ipv4.conf.all.secure_redirects = 0
net.ipv4.conf.default.secure_redirects = 0
# 严格 rp_filter=1 易在覆盖网络/不对称路径上丢包；全员用 0（或改成 2 loose）
net.ipv4.conf.all.rp_filter = 0
net.ipv4.conf.default.rp_filter = 0

XPXE_EOF
if ! sysctl --system; then
  echo "xpxe-warning: sysctl --system 有失败项（不中断）" >&2
fi
