#!/usr/bin/env bash
# scripts/accept.sh 的公共函数：被测部署（compose）、管理 API、场景框架。由 accept.sh source。
# 约定：任何 secret（初始密码、管理员新密码、API 令牌、BMC 密码）只存放在 ${ACC}/ 下 0600 文件中，绝不 echo 到日志。
ACC="${WORK}/accept"
DEPLOY="${REPO_DIR}/deploy"
ENV_FILE="${ACC}/env"
API="http://127.0.0.1:8088/api/v1"
BASE="http://${LAB_IP}:8080"
COMPOSE=(docker compose --project-name x-pxe-lab --env-file "${ENV_FILE}" -f "${DEPLOY}/compose.yaml" -f "${DEPLOY}/compose.lab.yaml")
mkdir -p "${ACC}"
chmod 0700 "${ACC}"

# ---------- APT 缓存（与 lab/run.sh 相同：优先 acng 容器，回源失败时换 apt-proxy.py） ----------
LAB_MIRROR_DIR=/deb.debian.org/debian
apt_ok() { curl -s -m 30 -o /dev/null -w '%{http_code}' "http://${LAB_IP}:3142${LAB_MIRROR_DIR}/dists/trixie/Release" | grep -qx 200; }
acng_up() {
  apt_ok && return 0
  docker rm -f lab-acng >/dev/null 2>&1 || true
  docker run -d --name lab-acng -p "${LAB_IP}:3142:3142" xpxe-lab/acng >/dev/null
  sleep 2
  apt_ok && return 0
  log "lab-acng 回源失败，改用 apt-proxy.py"
  docker rm -f lab-acng >/dev/null 2>&1 || true
  svc_start_user apt-proxy python3 "${LAB_DIR}/lib/apt-proxy.py" "${LAB_IP}" 3142 "${WORK}/apt-cache"
  apt_ok || die "APT 缓存不可用"
}
svc_start_user() { # <名字> <命令…>
  local name="$1"; shift
  svc_running "${name}" && return 0
  : > "$(svc_log "${name}")"
  setsid "$@" >> "$(svc_log "${name}")" 2>&1 < /dev/null &
  echo $! > "$(svc_pid "${name}")"
  sleep 1
  svc_running "${name}" || { cat "$(svc_log "${name}")" >&2; die "服务 ${name} 启动失败"; }
}

# ---------- 被测部署 ----------
write_env() { # [offline=false]
  cat > "${ENV_FILE}" <<ENV
XPXE_VERSION=${XPXE_VERSION}
XPXE_BOOT_BASE_URL=http://${LAB_IP}:8080
XPXE_BOOTNET_ALLOWED_IFACES=${BR}
XPXE_OFFLINE=${1:-false}
XPXE_LOG=info
ENV
}
deploy_up() { # [offline]
  docker image inspect "x-pxe:${XPXE_VERSION}" >/dev/null 2>&1 || die "缺少镜像 x-pxe:${XPXE_VERSION}（先 docker build）"
  write_env "${1:-false}"
  "${REPO_DIR}/scripts/init-secrets.sh" "${DEPLOY}/secrets" > /dev/null
  "${COMPOSE[@]}" up -d --wait --wait-timeout 180 >> "${ACC}/compose.log" 2>&1 || {
    "${COMPOSE[@]}" ps >&2; "${COMPOSE[@]}" logs --tail 50 >&2; die "compose up 失败"; }
  wait_http "${API%/api/v1}/" 60 || die "管理端不可达"
}
deploy_down() { "${COMPOSE[@]}" down >> "${ACC}/compose.log" 2>&1 || true; }
deploy_wipe() { "${COMPOSE[@]}" down -v >> "${ACC}/compose.log" 2>&1 || true; rm -f "${ACC}/token" "${ACC}/admin-password" "${ACC}/ids.json"; }
wait_http() { # <url> <秒>
  local deadline=$((SECONDS + $2))
  while [ "${SECONDS}" -lt "${deadline}" ]; do curl -s -m 3 -o /dev/null "$1" && return 0; sleep 2; done
  return 1
}
cid() { "${COMPOSE[@]}" ps -q "$1"; }
ctl() { docker exec "$(cid control)" /usr/local/bin/xpxe-control "$@"; }

# ---------- 管理 API ----------
# 首次：初始密码登录 → 改密 → 签发 API 令牌（Bearer，免 CSRF）。令牌与新密码只写入 0600 文件。
api_bootstrap() {
  [ -s "${ACC}/token" ] && api GET /me > /dev/null 2>&1 && return 0
  local jar="${ACC}/cookies" init csrf new
  ( umask 077; : > "${jar}" )
  init="$(sudo cat "${DEPLOY}/secrets/admin_initial_password")"
  if [ ! -s "${ACC}/admin-password" ]; then
    ( umask 077; printf 'Lab-%s-Aa1!' "$(head -c 18 /dev/urandom | od -An -vtx1 | tr -d ' \n')" > "${ACC}/admin-password" )
  fi
  new="$(cat "${ACC}/admin-password")"
  local pw="${init}"
  for attempt in initial changed; do
    csrf="$(curl -s -c "${jar}" -b "${jar}" -H 'Content-Type: application/json' \
      --data "$(jq -n --arg p "${pw}" '{username:"admin",password:$p}')" "${API}/auth/login" | jq -r '.csrf_token // empty')"
    [ -n "${csrf}" ] && break
    pw="${new}"
  done
  [ -n "${csrf}" ] || die "管理员登录失败"
  if [ "${pw}" = "${init}" ]; then
    curl -s -f -o /dev/null -c "${jar}" -b "${jar}" -H 'Content-Type: application/json' -H "X-CSRF-Token: ${csrf}" \
      --data "$(jq -n --arg o "${init}" --arg n "${new}" '{old_password:$o,new_password:$n}')" "${API}/me/password" || die "改密失败"
  fi
  ( umask 077
    curl -s -f -c "${jar}" -b "${jar}" -H 'Content-Type: application/json' -H "X-CSRF-Token: ${csrf}" \
      --data '{"name":"accept","scopes":["read","operate","admin"],"ttl_days":7}' "${API}/me/tokens" | jq -r .token > "${ACC}/token" )
  [ -s "${ACC}/token" ] && [ "$(cat "${ACC}/token")" != null ] || die "签发 API 令牌失败"
}
# api <方法> <路径> [JSON]：成功打印响应体；HTTP ≥ 400 时打印响应体并返回 1
api() {
  local out code
  out="$(mktemp)"
  code="$(curl -s -o "${out}" -w '%{http_code}' -X "$1" -H "Authorization: Bearer $(cat "${ACC}/token")" \
    -H 'Content-Type: application/json' ${3:+--data "$3"} "${API}$2")"
  cat "${out}"; rm -f "${out}"
  [ "${code}" -lt 400 ]
}
api_upload() { # <kind> <文件>
  curl -s -f -X POST -H "Authorization: Bearer $(cat "${ACC}/token")" -H 'Content-Type: application/octet-stream' \
    --data-binary "@$2" "${API}/assets/upload?kind=$1"
}

# ---------- 场景框架（与 lab/run.sh 同格式：PASS/FAIL、耗时、日志目录） ----------
begin() { # <id>
  D="${OUT}/accept/$1"; rm -rf "${D}"; mkdir -p "${D}"; CHECKS=(); T0=${SECONDS}
  sudo tcpdump -i "${BR}" -U -s0 -w "${D}/capture.pcap" udp > /dev/null 2>&1 &
  TCPDUMP=$!; sleep 1
  log "== 场景 $1 开始"
}
check() { # <名字> <0|1> <说明>
  local verdict=PASS; [ "$2" = 0 ] || verdict=FAIL
  CHECKS+=("${verdict} $1: $3")
  log "  ${verdict} $1: $3"
}
ok_if() { # <名字> <说明> <命令…>：命令成功则 PASS
  local name="$1" what="$2"; shift 2
  if "$@"; then check "${name}" 0 "${what}"; else check "${name}" 1 "${what}"; fi
}
finish() { # <id>
  sudo kill "${TCPDUMP}" 2>/dev/null || true; sleep 1
  sudo chown "$(id -un)" "${D}/capture.pcap" 2>/dev/null || true
  api GET "/audit/export?since=0" > "${D}/audit-export.jsonl" 2>/dev/null || true
  "${COMPOSE[@]}" logs --no-color --since "$((SECONDS - T0 + 5))s" > "${D}/compose.log" 2>&1 || true
  local r=PASS c
  [ "${#CHECKS[@]}" -gt 0 ] || r=FAIL
  for c in "${CHECKS[@]}"; do if [[ "$c" == FAIL* ]]; then r=FAIL; fi; done
  { echo "$1 ${r} $((SECONDS - T0))s ${D#"${REPO_DIR}/"}"; printf '  %s\n' "${CHECKS[@]}"; } > "${D}/result"
  cat "${D}/result"
}

# ---------- 产品初始化：资产、Profile、计划、引导网络（结果 ID 存 ${ACC}/ids.json） ----------
LAB_KEY="${ACC}/id_ed25519"
ids() { jq -r ".$1" "${ACC}/ids.json"; }
set_id() { local t; t="$(mktemp)"; jq --arg k "$1" --argjson v "$2" '.[$k]=$v' "${ACC}/ids.json" > "$t" && mv "$t" "${ACC}/ids.json"; }
activate() { # <kind> <asset_id> [理由]
  api POST /assets/activate "$(jq -n --arg k "$1" --argjson id "$2" --arg r "${3:-实验室验收：金丝雀未实现，跳过}" \
    '{kind:$k,asset_id:$id,skip_canary_reason:$r}')" > /dev/null
}
upload_ipxe() { # <kind> <文件> → 资产 ID
  api_upload "$1" "$2" | jq -r .id
}
profile_doc() { # <名字> [jq 过滤器]：nexus-lvm 预设 + 实验室 APT 缓存镜像源
  api GET /presets | jq --arg n "$1" --arg hp "${LAB_IP}:3142" --arg dir "${LAB_MIRROR_DIR}" \
    ".profile[\"nexus-lvm\"] | .name=\$n | .mirror={scheme:\"http\",host_port:\$hp,directory:\$dir,proxy:null} | .ntp=[\"${LAB_IP}\"] | .disk.prefer=\"smallest\" | ${2:-.}"
}
plan_doc() { # <名字>：deb13-init-scripts 预设，root 追加实验室 SSH 公钥
  api GET /presets | jq --arg n "$1" --arg key "$(cat "${LAB_KEY}.pub")" \
    '.init_plan["deb13-init-scripts"] | .name=$n | .modules.users_keys.settings.root_authorized_keys += [$key]'
}
create_config() { # <kind> <name> <document JSON> → ID
  api POST "/configs/$1" "$(jq -n --arg n "$2" --argjson d "$3" '{name:$n,document:$d}')" | jq -r '.id // .object_id'
}
save_config() { # <kind> <id> <document> <说明>：基于当前头版本保存新版本
  local head; head="$(api GET "/configs/$1/$2" | jq -r '.version.version')"
  api PUT "/configs/$1/$2" "$(jq -n --argjson d "$3" --argjson h "${head}" --arg c "$4" '{document:$d,expected_head:$h,comment:$c}')"
}
set_bootnet() { # proxy_dhcp|external_dhcp|redfish_only
  local doc
  case "$1" in redfish_only) doc='{"mode":"redfish_only"}' ;; *) doc="$(jq -n --arg m "$1" --arg i "${BR}" '{mode:$m,iface:$i}')" ;; esac
  local id; id="$(jq -r '.boot_network // empty' "${ACC}/ids.json")"
  if [ -z "${id}" ]; then set_id boot_network "$(create_config boot_network default "${doc}")"
  else save_config boot_network "${id}" "${doc}" "accept: $1" > /dev/null; fi
  wait_bootnet_mode "$1" 90
}
wait_bootnet_mode() { # <mode> <秒>
  local deadline=$((SECONDS + $2)) want="$1"
  case "$1" in proxy_dhcp) want=proxy ;; external_dhcp) want=external ;; esac
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    local hb; hb="$(api GET /bootnet/status | jq -r '.heartbeat.heartbeat | "\(.mode) \(.health)"' 2>/dev/null)"
    case "${hb}" in "${want} ok"|"$1 ok") return 0 ;; esac
    sleep 3
  done
  log "bootnet 未进入 $1（最后：${hb:-无}）"; return 1
}
prepare_product() {
  acng_up
  [ -f "${LAB_KEY}" ] || ssh-keygen -q -t ed25519 -N '' -C xpxe-accept -f "${LAB_KEY}"
  [ -s "${ACC}/ids.json" ] && return 0
  echo '{}' > "${ACC}/ids.json"
  local k f id
  for k in ipxe_efi_x64:ipxe.efi ipxe_undionly:undionly.kpxe ipxe_snponly_x64:snponly.efi; do
    id="$(upload_ipxe "${k%%:*}" "/usr/lib/ipxe/${k##*:}")"; activate "${k%%:*}" "${id}"
  done
  id="$(api POST /assets/fetch '{"kind":"debian_netboot_amd64","mirror":"http://deb.debian.org/debian"}' | jq -r .id)"
  activate debian_netboot_amd64 "${id}"
  set_id netboot "${id}"
  set_id profile "$(create_config profile lab-dhcp "$(profile_doc lab-dhcp)")"
  set_id plan "$(create_config init_plan lab-deb13 "$(plan_doc lab-deb13)")"
  set_id dhcp_rules "$(create_config dhcp_rules default '{"uefi_firmware":"ipxe_efi","http_boot_enabled":true,"mirror_bootfile_options":false}')"
  set_bootnet proxy_dhcp
}

# ---------- 节点 ----------
mac_of() { echo "52:54:00:77:13:$1"; }
register_node() { # <hostname> <mac> <uefi|bios> [network JSON] [channel JSON] [profile id] → 节点 ID
  local spec
  spec="$(jq -n --arg h "$1" --arg m "$2" --arg f "$3" --argjson net "${4:-{\"type\":\"dhcp\"\}}" --argjson ch "${5:-{\"type\":\"lan\"\}}" \
    '{hostname:$h,fqdn:null,mac:$m,os_family:"debian13",firmware:$f,channel:$ch,network:$net}')"
  api POST /nodes "$(jq -n --argjson s "${spec}" --argjson p "${6:-$(ids profile)}" --argjson q "$(ids plan)" \
    '{spec:$s,profile_id:$p,init_plan_id:$q}')" | jq -r .id
}
node_state() { api GET "/nodes/$1" | jq -r '.state // .node.state'; }
wait_state() { # <节点> <秒> <状态…>：任一目标状态出现即返回 0；进入 Failed/Expired 类状态提前返回 1
  local node="$1" deadline=$((SECONDS + $2)) s; shift 2
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    s="$(node_state "${node}")"
    for want in "$@"; do [ "${s}" = "${want}" ] && return 0; done
    case "${s}" in *[Ff]ailed*|*[Ee]xpired*|*[Tt]imed*) [[ " $* " == *" ${s} "* ]] || { log "节点 ${node} 进入 ${s}"; return 1; } ;; esac
    sleep 10
  done
  log "节点 ${node} 超时，最后状态 ${s}"; return 1
}
node_ip() { # <mac>：br-lab 邻居表 / Kea 租约
  ip neigh show dev "${BR}" | awk -v m="$1" 'tolower($0) ~ m {print $1; exit}'
}
tssh() { # <ip> <命令>
  ssh -n -i "${LAB_KEY}" -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 \
    -o LogLevel=ERROR "root@$1" "$2"
}
wait_ssh() { # <mac> <秒> → 打印 IP
  local deadline=$((SECONDS + $2)) ip
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    ip="$(node_ip "$1")"
    if [ -n "${ip}" ] && tssh "${ip}" true 2>/dev/null; then echo "${ip}"; return 0; fi
    ping -c1 -W1 -b 10.77.0.255 >/dev/null 2>&1 || true
    sleep 10
  done
  return 1
}
