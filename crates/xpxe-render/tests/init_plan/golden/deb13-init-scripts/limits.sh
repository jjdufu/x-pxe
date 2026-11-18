#!/bin/bash
# x-pxe 初始化模块 limits：资源限制
set -euo pipefail

cat > /etc/security/limits.d/90-nofile.conf << 'XPXE_EOF'
* soft nofile 655350
* hard nofile 655350
* soft nproc  unlimited
* hard nproc  unlimited
* soft core   unlimited
* hard core   unlimited
# End of file
XPXE_EOF
