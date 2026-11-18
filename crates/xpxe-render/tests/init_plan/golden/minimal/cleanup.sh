#!/bin/bash
# x-pxe 初始化模块 cleanup：清理
set -euo pipefail

keep='/var/log/xpxe-firstboot.log'
find / -maxdepth 3 -name hosts.equiv -print0 2>/dev/null | xargs -0r rm -rf
find / -maxdepth 3 -name .netrc -print0 2>/dev/null | xargs -0r rm -rf
find / -maxdepth 3 -name .rhosts -print0 2>/dev/null | xargs -0r rm -rf
apt-get clean
apt-get autoclean -y
DEBIAN_FRONTEND=noninteractive apt-get autoremove -y
find /var/log/apt/ -type f -delete 2>/dev/null || true
find /var/log \( -name "vmware-*.*.log" -o -name "*.log-*" -o -name "*.gz" -o -name "*log.*" \) ! -path "${keep}" 2>/dev/null | while read -r f; do rm -f "$f"; done || true
find /var/log -type f -name "*log" ! -path "${keep}" -exec truncate -s 0 {} \; 2>/dev/null || true
sync
find /tmp -mindepth 1 -delete 2>/dev/null || true
