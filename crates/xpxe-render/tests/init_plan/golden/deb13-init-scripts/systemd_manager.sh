#!/bin/bash
# x-pxe 初始化模块 systemd_manager：systemd 启停超时
set -euo pipefail

mkdir -p /etc/systemd/system.conf.d
cat > /etc/systemd/system.conf.d/timeout.conf << 'XPXE_EOF'
[Manager]
DefaultTimeoutStartSec=30s
DefaultTimeoutStopSec=30s
XPXE_EOF
