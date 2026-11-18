#!/bin/bash
# x-pxe 初始化模块 locale：语言环境
set -euo pipefail

cat > /etc/default/locale << 'XPXE_EOF'
LANG="en_US.UTF-8"
LC_ALL="en_US.UTF-8"
LANGUAGE="en_US.UTF-8"
XPXE_EOF
touch /etc/locale.gen
sed -i 's/^# *zh_CN.UTF-8 UTF-8/zh_CN.UTF-8 UTF-8/' /etc/locale.gen
sed -i 's/^# *en_US.UTF-8 UTF-8/en_US.UTF-8 UTF-8/' /etc/locale.gen
grep -qxF 'zh_CN.UTF-8 UTF-8' /etc/locale.gen || echo 'zh_CN.UTF-8 UTF-8' >> /etc/locale.gen
grep -qxF 'en_US.UTF-8 UTF-8' /etc/locale.gen || echo 'en_US.UTF-8 UTF-8' >> /etc/locale.gen
locale-gen
update-locale LANG='en_US.UTF-8' LC_ALL='en_US.UTF-8' LANGUAGE='en_US.UTF-8'
