#!/bin/bash
# x-pxe 初始化模块 ssh_generator_fix：ssh generator 刷屏修复
set -euo pipefail

mkdir -p /etc/systemd/system-generators
ln -sfn /dev/null /etc/systemd/system-generators/systemd-ssh-generator
systemctl mask 'sshd-unix-local.socket' 2>/dev/null || true
systemctl mask 'sshd-vsock.socket' 2>/dev/null || true
