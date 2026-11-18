#!/bin/bash
# x-pxe 初始化模块 packages：软件包
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
apt-get install -y \
  'sudo' \
  'vim' \
  'git' \
  'curl' \
  'wget' \
  'rsync' \
  'jq' \
  'tree' \
  'tmux' \
  'btop' \
  'bash-completion' \
  'man-db' \
  'cron' \
  'dialog' \
  'dos2unix' \
  'zip' \
  'xz-utils' \
  'p7zip-full' \
  'psmisc' \
  'ncdu' \
  'ripgrep' \
  'rsyslog' \
  'iputils-ping' \
  'gpg' \
  'expect'
