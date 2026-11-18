#!/bin/bash
# x-pxe 初始化模块 sshd：SSH 服务配置
set -euo pipefail

mkdir -p /etc/ssh/sshd_config.d
cat > /etc/ssh/sshd_config.d/jwczjzl.conf << 'XPXE_EOF'
PermitRootLogin no
X11Forwarding no
X11UseLocalhost yes
AllowTcpForwarding yes
AllowAgentForwarding yes
IgnoreRhosts yes
GSSAPIAuthentication no
UseDNS no
Compression no
TCPKeepAlive yes
PasswordAuthentication no
Banner /etc/issue
XPXE_EOF
sed -i 's/^#\?X11Forwarding.*/X11Forwarding no/' /etc/ssh/sshd_config 2>/dev/null || true
if ! grep -qiE '^\s*Banner\s+' /etc/ssh/sshd_config; then
  echo 'Banner /etc/issue' >> /etc/ssh/sshd_config
else
  sed -ri 's#^\s*(banner|Banner)\s+.*#Banner /etc/issue#' /etc/ssh/sshd_config
fi
sshd -t
systemctl reload ssh 2>/dev/null || systemctl restart ssh
