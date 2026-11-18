#!/bin/bash
# x-pxe 初始化模块 time_sync：时间同步
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
if ! dpkg-query -W -f='${Status}' systemd-timesyncd 2>/dev/null | grep -q 'install ok installed'; then
  apt-get install -y systemd-timesyncd
fi
mkdir -p /etc/systemd/timesyncd.conf.d
cat > /etc/systemd/timesyncd.conf.d/ntp.conf << 'XPXE_EOF'
[Time]
NTP=ntp.aliyun.com
XPXE_EOF
systemctl enable --now systemd-timesyncd
systemctl restart systemd-timesyncd
