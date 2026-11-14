#!/usr/bin/env bash
# 安装 Debian 软件包。优先 apt-get；当本机 apt 无法访问镜像源（例如受限出口代理拒绝 apt 的请求）
# 时，退回到“curl 下载索引与 .deb + dpkg -i”的方式。只在实验室主机上使用。
set -euo pipefail
pkgs=("$@")
[ "${#pkgs[@]}" -gt 0 ] || { echo "用法：apt-install.sh <包…>" >&2; exit 2; }
missing=()
for p in "${pkgs[@]}"; do dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q 'install ok installed' || missing+=("$p"); done
[ "${#missing[@]}" -eq 0 ] && exit 0
export DEBIAN_FRONTEND=noninteractive
if sudo apt-get update -qq >/dev/null 2>&1 && sudo apt-get install -y -qq --no-install-recommends "${missing[@]}" >/dev/null 2>&1; then
  exit 0
fi
echo "apt-install: apt-get 失败，改用 curl 回退" >&2
mirror="${XPXE_LAB_DEBIAN_MIRROR:-http://deb.debian.org/debian}"
lists=/var/lib/apt/lists
host="$(sed -E 's#^[a-z]+://##; s#/#_#g' <<<"${mirror}")"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT
for suite in trixie trixie-updates; do
  curl -fsSL "${mirror}/dists/${suite}/InRelease" -o "${tmp}/InRelease"
  sudo cp "${tmp}/InRelease" "${lists}/${host}_dists_${suite}_InRelease"
  curl -fsSL "${mirror}/dists/${suite}/main/binary-amd64/Packages.xz" | xz -d > "${tmp}/Packages"
  sudo cp "${tmp}/Packages" "${lists}/${host}_dists_${suite}_main_binary-amd64_Packages"
done
sudo apt-get install -y -qq --no-install-recommends --print-uris "${missing[@]}" \
  | sed -nE "s/^'([^']+)'.*/\1/p" > "${tmp}/uris"
mkdir -p "${tmp}/debs"
(cd "${tmp}/debs" && xargs -r -P8 -n1 curl -fsSLO < "${tmp}/uris")
if ls "${tmp}/debs"/*.deb >/dev/null 2>&1; then
  sudo dpkg -i "${tmp}/debs"/*.deb >/dev/null
fi
