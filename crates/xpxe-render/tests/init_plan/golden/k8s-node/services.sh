#!/bin/bash
# x-pxe 初始化模块 services：系统服务
set -euo pipefail

systemctl disable --now apt-daily.timer apt-daily-upgrade.timer
systemctl unmask 'ssh' 2>/dev/null || true
systemctl enable --now 'ssh'
systemctl mask 'cryptdisks-early.service'
systemctl mask 'cryptdisks.service'
systemctl mask 'x11-common.service'
systemctl mask 'nftables.service'
systemctl daemon-reload
update-alternatives --set editor '/usr/bin/vim.basic'
