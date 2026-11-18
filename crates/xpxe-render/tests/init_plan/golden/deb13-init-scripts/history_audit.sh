#!/bin/bash
# x-pxe 初始化模块 history_audit：命令历史审计
set -euo pipefail

mkdir -vp '/var/log/.history'
chmod 1777 '/var/log/.history'
cat > /etc/profile.d/history-record.sh << 'XPXE_EOF'
# 审计：每次登录独立 HISTFILE；HISTSIZE/HISTFILESIZE 统一为大容量
export HISTSIZE=100000
export HISTFILESIZE=100000

LOGTIME=$(date +%Y%m%d-%H-%M-%S)
export HISTFILE="/var/log/.history/${USER}.${LOGTIME}.history"
if [ ! -f "${HISTFILE}" ]; then
  touch "${HISTFILE}"
fi
chmod 600 "${HISTFILE}"
chattr +a "${HISTFILE}" 2>/dev/null || true

HISTTIMEFORMAT="%F_%T $(whoami)#$(who -u am i 2>/dev/null| awk '{print $NF}'|sed -e 's/[()]//g'):"
export HISTTIMEFORMAT
XPXE_EOF
chmod 644 /etc/profile.d/history-record.sh
