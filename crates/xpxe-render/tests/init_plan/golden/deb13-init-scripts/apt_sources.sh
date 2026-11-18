#!/bin/bash
# x-pxe 初始化模块 apt_sources：APT 源整理
set -euo pipefail

# 经典 sources.list 与 *.list：注释掉 deb cdrom: 行；deb822 指向 cdrom 的 *.sources 改名禁用
shopt -s nullglob
for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.list; do
  [[ -f "$f" ]] || continue
  sed -i -E 's/^[[:space:]]*(deb(-src)?[[:space:]]+cdrom:)/# \1/' "$f"
done
for f in /etc/apt/sources.list.d/*.sources; do
  if grep -qiE '^URIs:[[:space:]]*cdrom:' "$f"; then
    mv -f "$f" "${f}.disabled-cdrom"
  fi
done
shopt -u nullglob
# 只恢复已有的被注释 http(s) 行，不写入新地址
shopt -s nullglob
for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.list; do
  [[ -f "$f" ]] || continue
  sed -i -E 's/^#[[:space:]]*(deb(-src)?[[:space:]]+https?:)/\1/' "$f"
done
shopt -u nullglob
has_net_src=0
if grep -RqhE '^[[:space:]]*deb(-src)?[[:space:]]+https?:' /etc/apt/sources.list /etc/apt/sources.list.d 2>/dev/null; then
  has_net_src=1
fi
shopt -s nullglob
for f in /etc/apt/sources.list.d/*.sources; do
  if grep -qiE '^URIs:[[:space:]]*https?:' "$f"; then
    has_net_src=1
    break
  fi
done
shopt -u nullglob
if [[ "$has_net_src" -eq 0 ]]; then
  echo "未检测到网络 APT 源，写入兜底 deb822 源"
  cat > /etc/apt/sources.list.d/debian.sources << 'XPXE_EOF'
Types: deb
URIs: https://deb.debian.org/debian
Suites: trixie trixie-updates trixie-backports
Components: main contrib non-free non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg

Types: deb
URIs: https://deb.debian.org/debian-security
Suites: trixie-security
Components: main contrib non-free non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
XPXE_EOF
fi
apt-get autoclean -y || true
apt-get update
