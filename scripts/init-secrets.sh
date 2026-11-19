#!/usr/bin/env bash
# 在宿主机生成 x-pxe 的三个 secret 文件（只在文件不存在时生成，已有文件原样保留）。
# 用法：scripts/init-secrets.sh [目录，默认 deploy/secrets]
# 生成后属主改为容器用户 10001:10001、权限 0400（bootnet_token 为 0440，供 bootnet 以组 10001 读取）（需要 sudo；非 root 且无 sudo 时只告警）。
# 不在终端或日志中输出任何 secret 内容。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="${1:-${ROOT}/deploy/secrets}"
OWNER="${XPXE_SECRETS_OWNER:-10001:10001}"
mkdir -p "${DIR}"
umask 077

as_root() { if [ "$(id -u)" = 0 ]; then "$@"; elif command -v sudo >/dev/null 2>&1; then sudo "$@"; else return 1; fi; }
exists() { as_root test -e "$1" 2>/dev/null || [ -e "$1" ]; }

gen() { # <文件名> <生成函数> [权限，默认 0400]
  local path="${DIR}/$1" tmp mode="${3:-0400}"
  if exists "${path}"; then
    echo "init-secrets: ${path} 已存在，保留"
    return 0
  fi
  tmp="$(mktemp "${DIR}/.$1.XXXXXX")"
  "$2" > "${tmp}"
  mv "${tmp}" "${path}"
  if as_root chown "${OWNER}" "${path}" && as_root chmod "${mode}" "${path}"; then
    echo "init-secrets: 已生成 ${path}（${OWNER}，${mode}）"
  else
    chmod "${mode}" "${path}"
    echo "init-secrets: 已生成 ${path}；警告：无法 chown ${OWNER}，容器用户可能读不到" >&2
  fi
}

master_key() { head -c 32 /dev/urandom; }
admin_password() {
  local out=""
  while [ "${#out}" -lt 24 ]; do
    out="${out}$(head -c 256 /dev/urandom | LC_ALL=C tr -dc 'A-Za-z0-9!#%+,.:=@^_~-')"
  done
  printf '%s' "${out:0:24}"
}
bootnet_token() { head -c 32 /dev/urandom | od -An -vtx1 | tr -d ' \n'; }

gen master_key master_key
gen admin_initial_password admin_password
# bootnet 以 UID 0、组 10001 运行（见 deploy/compose.yaml），无 DAC_OVERRIDE，靠组读权限读取令牌
gen bootnet_token bootnet_token 0440
echo "init-secrets: 完成。首次登录用户名 admin，初始密码见 ${DIR}/admin_initial_password（需 sudo 读取）"
