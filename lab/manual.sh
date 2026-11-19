#!/usr/bin/env bash
# Step 14 手动 ISO 网络引导实验室场景：lab/manual.sh [--version <镜像标签>] [--fresh] [--keep] <manual-sanboot|manual-pve>…
#   manual-sanboot：hybrid ISO（Debian ipxe 包的 ipxe.iso）经 sanboot 在 OVMF 与 SeaBIOS 下启动
#                   （判定：选中条目后 ISO 内的 iPXE 再次联网并命中菜单 → 第二次 manual.menu_hit）。
#   manual-pve：Proxmox VE 9.2 ISO（--pve-iso <文件>，默认 ~/labruns/isos/proxmox-ve_9.2-1.iso，校验 SHA-256）
#               上传后识别为 PVE 并预填路径，kernel_initrd + embed_iso；OVMF 与 SeaBIOS 各一台进入安装器，串口无 “no cdrom found”。
# 引导链路按计划 3.2 节：bootnet 为 external_dhcp，现网 DHCP（lab-dhcp + 产品 Kea 片段）把 iPXE 指向 /boot.ipxe；
# 目标机 MAC 不登记为节点（未武装），只列入菜单白名单。结果写 lab/out/accept/<场景>/（与 scripts/accept.sh 同格式）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "${ROOT}/lab/lib/common.sh"
XPXE_VERSION="$(cd "${ROOT}" && git describe --always)"
FRESH=0 PVE_ISO="${HOME}/labruns/isos/proxmox-ve_9.2-1.iso"
PVE_SHA=4e88fe416df9b527624a175f24c9aa07c714d3332afb1ee3dbf3879573ef2c6c
export KEEP=0 XPXE_LAB_ACCEL="${XPXE_LAB_ACCEL:-kvm}"
while [ $# -gt 0 ]; do
  case "$1" in
    --version) XPXE_VERSION="$2"; shift 2 ;;
    --fresh) FRESH=1; shift ;;
    --keep) KEEP=1; shift ;;
    --pve-iso) PVE_ISO="$2"; shift 2 ;;
    *) break ;;
  esac
done
[ $# -gt 0 ] || { sed -n '2,8p' "$0" >&2; exit 2; }
source "${ROOT}/lab/accept/lib.sh"
source "${ROOT}/lab/accept/scenarios.sh"
CHUNK=$((8 * 1024 * 1024))

# ---------- 现网 DHCP：产品 Kea 片段并入 existing.json（同验收场景 E） ----------
kea_product() {
  set_bootnet external_dhcp || die "bootnet 未进入 external_dhcp"
  api GET /bootnet/snippets | jq -r '.snippets[] | select(.kind=="kea") | .text' > "${ACC}/kea-snippet.txt"
  python3 - "${LAB_DIR}/kea/existing.json" "${ACC}/kea-snippet.txt" "${LAB_DIR}/kea/product.json" <<'PY'
import json, sys
conf = json.load(open(sys.argv[1]))
snippet = open(sys.argv[2]).read().strip().rstrip(',')
d4 = conf["Dhcp4"]
d4["client-classes"] = json.loads("{" + snippet + "}")["client-classes"]
for sub in d4.get("subnet4", []):
    sub.pop("boot-file-name", None); sub.pop("next-server", None)
    sub["option-data"] = [o for o in sub.get("option-data", []) if o.get("name") not in ("tftp-server-name", "boot-file-name")]
d4.pop("boot-file-name", None); d4.pop("next-server", None)
json.dump(conf, open(sys.argv[3], "w"), indent=2)
PY
  "${LAB_DIR}/dhcp.sh" stop > /dev/null; "${LAB_DIR}/dhcp.sh" start product > /dev/null
}

# ---------- ISO 分块上传（与 WebUI 相同的 API）→ 打印 complete 响应 ----------
iso_upload() { # <文件> <标签> [期望 SHA-256]
  local file="$1" size begun id offset=0 n=0
  size="$(stat -c %s "${file}")"
  begun="$(api POST /isos/uploads "$(jq -n --arg l "$2" --argjson s "${size}" --arg e "${3:-}" \
    '{label:$l,size:$s,expected_sha256:(if $e=="" then null else $e end)}')")" || { echo "${begun}" >&2; return 1; }
  id="$(jq -r .id <<< "${begun}")"
  while [ "${offset}" -lt "${size}" ]; do
    offset="$(dd if="${file}" bs="${CHUNK}" skip="${n}" count=1 status=none \
      | curl -s -f -X PUT -H "Authorization: Bearer $(cat "${ACC}/token")" -H 'Content-Type: application/octet-stream' \
          --data-binary @- "${API}/isos/uploads/${id}?offset=${offset}" | jq -r .received)" || return 1
    n=$((n + 1))
  done
  api POST "/isos/uploads/${id}/complete" '{}'
}
iso_sha_by_label() { api GET /isos | jq -r --arg l "$1" '.items[] | select(.iso.label==$l) | .iso.sha256' | head -1; }
ensure_iso() { # <文件> <标签> [期望] → 打印 SHA-256（同内容已存在时复用）
  local sha; sha="$(iso_sha_by_label "$2")"
  if [ -z "${sha}" ]; then iso_upload "$1" "$2" "${3:-}" > "${D}/upload-$2.json" && sha="$(jq -r .sha256 "${D}/upload-$2.json")"; fi
  echo "${sha}"
}

# ---------- 菜单配置 ----------
save_menu() { # <document JSON> <说明>
  local id; id="$(jq -r '.boot_menu // empty' "${ACC}/ids.json")"
  if [ -z "${id}" ]; then set_id boot_menu "$(create_config boot_menu default "$1")"
  else save_config boot_menu "${id}" "$1" "$2" > /dev/null; fi
}
wait_extracts() { # <秒>：所有启用条目都不再是“抽取中/待抽取”
  local deadline=$((SECONDS + $1)) s
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    s="$(api GET /boot-menu/status)"
    jq -e '[.entries[] | select(.enabled and .action.type=="unavailable" and (.action.reason | test("pending|running")))] | length == 0' <<< "${s}" > /dev/null \
      && { echo "${s}" > "${D}/menu-status.json"; return 0; }
    sleep 5
  done
  echo "${s}" > "${D}/menu-status.json"; return 1
}
audit_count() { # <action> <mac>
  api GET "/audit/export?since=0" | jq -s --arg a "$1" --arg t "mac:$2" '[.[] | select(.action==$a and .target==$t)] | length'
}
wait_audit() { # <action> <mac> <至少几条> <秒>
  local deadline=$((SECONDS + $4))
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    [ "$(audit_count "$1" "$2")" -ge "$3" ] && return 0
    sleep 5
  done
  return 1
}
screendump() { echo "screendump $1/screen.ppm" | socat - "unix-connect:$1/mon.sock" > /dev/null 2>&1 || true; }

sc_manual_sanboot() {
  local sha; sha="$(ensure_iso /usr/lib/ipxe/ipxe.iso ipxe-iso)"
  ok_if "上传" "ipxe.iso → ${sha:0:12}（识别：$(api GET /isos | jq -r --arg s "${sha}" '.items[] | select(.iso.sha256==$s) | .iso.detected_family')）" test -n "${sha}"
  local m1 m2; m1="$(mac_of e1)"; m2="$(mac_of e2)"
  save_menu "$(jq -n --arg s "${sha}" --arg a "${m1}" --arg b "${m2}" '{enabled:true,show_unknown:false,allow_macs:[$a,$b],
    default_entry:"ipxe-iso",timeout_s:3,chain_prefixes:[],entries:[{id:"ipxe-iso",title:"iPXE ISO (sanboot)",arch:"any",
    kind:{type:"sanboot",iso:$s}},{id:"local",title:"Local disk",arch:"any",kind:{type:"exit"}}]}')" "manual-sanboot"
  ok_if "菜单" "sanboot 条目就绪" wait_extracts 60
  local fw mac i=0 vm
  for fw in ovmf seabios; do
    mac="$(mac_of "e$((i + 1))")"
    vm="$(vm_start "ms-${fw}" "${fw}" virtio-net-pci "${mac}" "${TAPS[i]}" --disks 1G,1G)"
    if wait_audit manual.select "${mac}" 1 "${BOOT_TIMEOUT}" && wait_audit manual.menu_hit "${mac}" 2 300; then
      check "sanboot(${fw})" 0 "菜单命中 → 选中 ipxe-iso → ISO 内 iPXE 再次联网命中菜单（menu_hit $(audit_count manual.menu_hit "${mac}") 次）"
    else
      check "sanboot(${fw})" 1 "menu_hit $(audit_count manual.menu_hit "${mac}") / select $(audit_count manual.select "${mac}")"
    fi
    screendump "${vm}"; cp "${vm}/screen.ppm" "${D}/screen-${fw}.ppm" 2>/dev/null || true
    vm_keep_serial "${vm}" "${fw}"; vm_stop "${vm}"
    i=$((i + 1))
  done
}

sc_manual_pve() {
  [ -s "${PVE_ISO}" ] || { check "ISO" 1 "缺少 ${PVE_ISO}"; return 0; }
  local sha up; sha="$(ensure_iso "${PVE_ISO}" proxmox-ve-9.2-1 "${PVE_SHA}")"
  ok_if "上传" "SHA-256 ${sha} 与官方 SHA256SUMS 一致" test "${sha}" = "${PVE_SHA}"
  up="$(api GET /isos | jq -c --arg s "${sha}" '.items[] | select(.iso.sha256==$s) | .iso.detection')"
  echo "${up}" > "${D}/detection.json"
  ok_if "识别" "PVE 系，预填 /boot/linux26 + /boot/initrd.img + embed_iso" \
    jq -e '.family=="pve" and .kernel_path=="/boot/linux26" and .initrd_path=="/boot/initrd.img" and .embed_iso==true' "${D}/detection.json"
  local m1 m2; m1="$(mac_of e3)"; m2="$(mac_of e4)"
  save_menu "$(jq -n --arg s "${sha}" --arg a "${m1}" --arg b "${m2}" --argjson d "${up}" '{enabled:true,show_unknown:false,allow_macs:[$a,$b],
    default_entry:"pve",timeout_s:3,chain_prefixes:[],entries:[{id:"pve",title:"Proxmox VE 9.2",arch:"any",
    kind:{type:"kernel_initrd",iso:$s,kernel_path:$d.kernel_path,initrd_path:$d.initrd_path,
      cmdline:"ramdisk_size=16777216 rw console=tty0 console=ttyS0,115200",embed_iso:true,iso_name:($d.iso_name // "proxmox.iso")}}]}')" "manual-pve"
  ok_if "抽取" "内核、initrd 与 initrd+ISO 产物就绪" wait_extracts 1800
  local fw mac i=0 vm
  for fw in ovmf seabios; do
    mac="$(mac_of "e$((i + 3))")"
    vm="$(vm_start "mp-${fw}" "${fw}" virtio-net-pci "${mac}" "${TAPS[i]}" --mem 8192 --disks 32G,1G)"
    i=$((i + 1))
  done
  i=0
  for fw in ovmf seabios; do
    mac="$(mac_of "e$((i + 3))")"; vm="${REPO_DIR}/lab/work/vm/acc-mp-${fw}"
    ok_if "选择(${fw})" "菜单命中并选中 pve" wait_audit manual.select "${mac}" 1 "${BOOT_TIMEOUT}"
    pve_installer "${vm}" "${fw}" 1800
    i=$((i + 1))
  done
  for fw in ovmf seabios; do vm="${REPO_DIR}/lab/work/vm/acc-mp-${fw}"; screendump "${vm}"; cp "${vm}/screen.ppm" "${D}/screen-${fw}.ppm" 2>/dev/null || true; vm_keep_serial "${vm}" "${fw}"; vm_stop "${vm}"; done
}
# 安装器判定：串口出现 PVE 安装环境的启动标志且没有 “no cdrom found”
PVE_MARK="${PVE_MARK:-Proxmox VE|proxmox-installer|Starting the installer|starting proxmox installer}"
pve_installer() { # <VM 目录> <固件> <秒>
  local deadline=$(($SECONDS + $3))
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    if grep -qai "no cdrom found" "$1/serial.log" 2>/dev/null; then check "安装器(${2})" 1 "串口出现 no cdrom found"; return; fi
    if grep -qaiE "${PVE_MARK}" "$1/serial.log" 2>/dev/null && grep -qa "Run /init" "$1/serial.log"; then
      sleep 30
      if grep -qai "no cdrom found" "$1/serial.log"; then check "安装器(${2})" 1 "串口出现 no cdrom found"; return; fi
      check "安装器(${2})" 0 "进入 PVE 安装环境：$(grep -aiE -m1 "${PVE_MARK}" "$1/serial.log" | tr -d '\r' | cut -c1-80)"; return
    fi
    sleep 10
  done
  check "安装器(${2})" 1 "$3 秒内串口无安装器标志（最后：$(tail -c 300 "$1/serial.log" 2>/dev/null | tr -d '\r' | tr '\n' ' ' | cut -c1-160)）"
}

log "被测镜像 x-pxe:${XPXE_VERSION}，场景 $*"
[ -x "${LAB_BIN}" ] || "${LAB_DIR}/setup.sh"
"${LAB_DIR}/net.sh" up
[ "${FRESH}" = 1 ] && deploy_wipe
deploy_up false
api_bootstrap
prepare_product
kea_product
mkdir -p "${OUT}/accept"
rc=0
for id in "$@"; do
  begin "${id}"
  case "${id}" in
    manual-sanboot) sc_manual_sanboot ;;
    manual-pve) sc_manual_pve ;;
    *) die "未知场景 ${id}" ;;
  esac || check "场景脚本" 1 "场景函数异常退出（$?）"
  finish "${id}" | tee -a "${OUT}/accept/summary.txt"
  grep -q "^${id} PASS" "${D}/result" || rc=1
done
vm_stop_all
exit "${rc}"
