#!/bin/bash
# x-pxe 初始化模块 timezone：时区
set -euo pipefail

timedatectl set-timezone 'Asia/Shanghai'
timedatectl set-local-rtc 0
echo 'Asia/Shanghai' > /etc/timezone
ln -sf /usr/share/zoneinfo/'Asia/Shanghai' /etc/localtime
