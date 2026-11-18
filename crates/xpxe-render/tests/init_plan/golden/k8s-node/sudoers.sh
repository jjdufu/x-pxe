#!/bin/bash
# x-pxe 初始化模块 sudoers：sudoers
set -euo pipefail

cp -a /etc/sudoers /etc/sudoers.ori 2>/dev/null || true
tmp="$(mktemp)"
trap 'rm -f "${tmp}"' EXIT
cat > "${tmp}" << 'XPXE_EOF'
# 环境与审计（模块化；不替换整份 /etc/sudoers）
Defaults    env_reset
Defaults    env_keep += "HOME"
Defaults    env_delete = "HTTP_PROXY HTTPS_PROXY"
Defaults    passwd_tries=3
Defaults    timestamp_timeout=10
Defaults    secure_path="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
Defaults    use_pty
Defaults    logfile="/var/log/sudo.log"

# 常用运维命令免密；完整管理仍走 Debian 默认 %sudo ALL=(ALL:ALL) ALL（需密码）
Cmnd_Alias SYSADMIN = /usr/sbin/service, /usr/bin/systemctl, /usr/bin/apt-get, /usr/bin/apt, /usr/sbin/reboot, /usr/sbin/shutdown
%sudo   ALL=(ALL:ALL) NOPASSWD: SYSADMIN
jwczjzl ALL=(ALL:ALL) ALL
XPXE_EOF
visudo -cf "${tmp}"
install -m 0440 -o root -g root "${tmp}" /etc/sudoers.d/jwczjzl
visudo -cf /etc/sudoers.d/jwczjzl
visudo -cf /etc/sudoers
cat > /etc/logrotate.d/sudo << 'XPXE_EOF'
/var/log/sudo.log {
    weekly
    rotate 4
    compress
    missingok
    create 640 root root
}
XPXE_EOF
