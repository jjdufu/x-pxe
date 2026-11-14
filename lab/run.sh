#!/usr/bin/env bash
# 场景编排：lab/run.sh [-j N] [--keep] <场景…>|all|capture|services|stop-services
# 每个场景输出 PASS/FAIL、耗时与日志目录（lab/out/<场景>/）。
source "$(dirname "$0")/lib/common.sh"
JOBS=1 KEEP=0
BASE="http://${LAB_IP}:8080"
mac_of() { echo "52:54:00:77:01:$1"; }
# ProxyDHCP 允许列表：除 N1（f1）以外的全部实验 MAC
ALLOW_IDS=(b1 b2 b3 b4 b5 b6 c1 c2 c3 c4 c5 c6 c7 d1 d2 e1 e2 f2 f3 91 92 a1 a2)
SNPONLY_IDS=(c6 c7)
SETPRIV=(setpriv --reuid "${PROBE_UID}" --regid "${PROBE_UID}" --clear-groups
         --inh-caps +net_bind_service --ambient-caps +net_bind_service)

svc_start() { # <名字> <root|user> <命令…>
  local name="$1" mode="$2"; shift 2
  svc_running "${name}" && return 0
  : > "$(svc_log "${name}")"
  if [ "${mode}" = root ]; then
    sudo setsid "$@" >> "$(svc_log "${name}")" 2>&1 < /dev/null &
  else
    setsid "$@" >> "$(svc_log "${name}")" 2>&1 < /dev/null &
  fi
  local launcher=$!
  sleep 0.5
  local child
  child="$(pgrep -P "${launcher}" | tail -1 || true)"
  echo "${child:-$launcher}" > "$(svc_pid "${name}")"
  svc_running "${name}" || { cat "$(svc_log "${name}")" >&2; die "服务 ${name} 启动失败"; }
}

services() { # proxy|noproxy
  ip link show "${BR}" >/dev/null 2>&1 || die "先运行 lab/net.sh up"
  svc_start serve user "${LAB_BIN}" serve --dir "${WORK}/www" --listen "${LAB_IP}:8080"
  local T="${WORK}/tftp"
  svc_start tftp root "${SETPRIV[@]}" "${LAB_BIN}" tftp --listen "${LAB_IP}:69" \
    --root-file "ipxe.efi=${T}/ipxe.efi" --root-file "snponly.efi=${T}/snponly.efi" \
    --root-file "undionly.kpxe=${T}/undionly.kpxe"
  if [ "$1" = proxy ]; then
    local args=() i
    for i in "${ALLOW_IDS[@]}"; do args+=(--allow-mac "$(mac_of "$i")"); done
    for i in "${SNPONLY_IDS[@]}"; do args+=(--efi-file-for "$(mac_of "$i")=snponly.efi"); done
    svc_start proxydhcp root "${SETPRIV[@]}" "${LAB_BIN}" proxydhcp --iface "${BR}" \
      --server-ip "${LAB_IP}" --boot-base "${BASE}" "${args[@]}"
  else
    svc_stop proxydhcp
  fi
}

stop_services() { for s in proxydhcp tftp serve sushy; do svc_stop "$s"; done; }

# 等待 d-i 的 early_command 事件（证明进入 d-i 且 preseed 被取走并执行）
wait_early() { # <mac> <秒>
  local mac="$1" deadline=$((SECONDS + $2))
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    if tail -n "+${SERVE_OFF:-1}" "$(svc_log serve)" | jq -R -r 'fromjson? | select(.event=="post") | .body' | grep -q "mac=${mac}"; then return 0; fi
    sleep 3
  done
  return 1
}

vm_stop() { local d="$1"; [ -f "${d}/qemu.pid" ] && kill "$(cat "${d}/qemu.pid")" 2>/dev/null || true; sleep 1; }

# 场景目录、抓包与结果
begin() { # <id>
  D="${OUT}/$1"; rm -rf "${D}"; mkdir -p "${D}"; CHECKS=(); T0=${SECONDS}
  SERVE_OFF=$(( $(wc -l < "$(svc_log serve)") + 1 ))
  sudo tcpdump -i "${BR}" -U -s0 -w "${D}/capture.pcap" udp > /dev/null 2>&1 &
  TCPDUMP=$!; sleep 1
}
check() { # <名字> <0|1> <说明>
  local verdict=PASS; [ "$2" = 0 ] || verdict=FAIL
  CHECKS+=("${verdict} $1: $3")
}
finish() { # <id>
  sudo kill "${TCPDUMP}" 2>/dev/null || true; sleep 1
  sudo chown "$(id -un)" "${D}/capture.pcap" 2>/dev/null || true
  local r=PASS c
  for c in "${CHECKS[@]}"; do if [[ "$c" == FAIL* ]]; then r=FAIL; fi; done
  { echo "$1 ${r} $((SECONDS - T0))s"; printf '  %s\n' "${CHECKS[@]}"; } > "${D}/result"
  cat "${D}/result"
}
fixtures() { # <标签> <mac…>
  local label="$1"; shift
  local macs=() m; for m in "$@"; do macs+=(--mac "$m"); done
  "${LAB_BIN}" capture --pcap "${D}/capture.pcap" --out "${D}/fixtures" --label "${label}" "${macs[@]}" --first-per-phase > "${D}/capture.log"
}
has_stage() { compgen -G "${D}/fixtures/*-$1.json" > /dev/null; }
json_of() { ls "${D}"/fixtures/*-"$1".json 2>/dev/null | head -1; }

# 通用：启动 VM → 等 early 事件 → 停 VM → 提取夹具
boot_to_di() { # <id> <tap> <固件> <网卡> <mac 编号> [vm.sh 额外参数…]
  local id="$1" tap="$2" fw="$3" nic="$4" mid="$5"; shift 5
  local mac; mac="$(mac_of "${mid}")"
  local vmdir; vmdir="$("${LAB_DIR}/vm.sh" --name "${id}-${mid}" --fw "${fw}" --nic "${nic}" --mac "${mac}" --tap "${tap}" "$@")"
  if wait_early "${mac}" "${BOOT_TIMEOUT:-300}"; then check "进入d-i(${mac})" 0 "preseed 已取走，early_command POST 已收到"
  else check "进入d-i(${mac})" 1 "超时未收到 early 事件"; fi
  vm_stop "${vmdir}"; cp "${vmdir}/serial.log" "${D}/serial-${mid}.log" 2>/dev/null || true
  [ "${KEEP}" = 1 ] || rm -rf "${vmdir}"
}

check_ipxe_dhcp() { # 首个 iPXE DISCOVER 同时带 Opt77=iPXE 与 Opt175（H4）
  local j; j="$(json_of discover-67-ipxe)"
  if [ -n "$j" ] && [ "$(jq -r '.user_class' "$j")" = iPXE ] && [ "$(jq -r '.opt175' "$j")" = true ]; then
    check "H4" 0 "iPXE DISCOVER 同时带 Opt77=iPXE 与 Opt175（$(basename "$j")）"
  else check "H4" 1 "iPXE DISCOVER 缺 Opt77 或 Opt175（${j:-无}）"; fi
}

jsel() { jq -R -c --arg m "${3:-}" "fromjson? | select($2)" "$1"; }
proxy_replied() { jsel "$(svc_log proxydhcp)" '.event=="reply" and .request.mac==$m' "$1" | wc -l; }
tftp_done() { jsel "$(svc_log tftp)" '.fields.path==$m and .fields.completed==true' "$1" | wc -l; }

sc_B() { # B1–B5
  local id="$1" tap="$2" nic mid
  case "${id}" in B1) nic=e1000 mid=b1 ;; B2) nic=e1000e mid=b2 ;; B3) nic=virtio mid=b3 ;; B4) nic=rtl8139 mid=b4 ;; B5) nic=vmxnet3 mid=b5 ;; esac
  boot_to_di "${id}" "${tap}" seabios "${nic}" "${mid}"
  fixtures "seabios-${nic}" "$(mac_of "${mid}")"
  compgen -G "${D}/fixtures/*-01-discover-67-ipxe.json" > /dev/null && check "首包即iPXE" 0 "SeaBIOS 网卡 ROM 首个 DISCOVER 已是 iPXE（其后的非 iPXE DISCOVER 来自 d-i）" || check "首包即iPXE" 1 "首包不是 iPXE"
  check_ipxe_dhcp
}

sc_U() { # U1–U5
  local id="$1" tap="$2" nic mid
  case "${id}" in U1) nic=e1000 mid=c1 ;; U2) nic=e1000e mid=c2 ;; U3) nic=virtio mid=c3 ;; U4) nic=rtl8139 mid=c4 ;; U5) nic=vmxnet3 mid=c5 ;; esac
  boot_to_di "${id}" "${tap}" ovmf "${nic}" "${mid}"
  fixtures "ovmf-pxe-${nic}" "$(mac_of "${mid}")"
  local j; j="$(json_of discover-67)"
  [ -n "$j" ] && [ "$(jq -r .arch "$j")" = 7 ] && check "EDK2首包" 0 "Opt93=7 $(jq -r .vendor_class "$j")" || check "EDK2首包" 1 "未见 Opt93=7 DISCOVER"
  # H2 是待验证的假设：记录观察到的行为，不作为场景失败条件
  if has_stage request-4011; then check "H2(观察)" 0 "EDK2 在 4011 发 REQUEST"
  else check "H2(观察)" 0 "EDK2 未发 4011 REQUEST：直接采用带 43(0x08)/siaddr/file 的 Proxy OFFER"; fi
  has_stage discover-67-ipxe && check "H1" 0 "ProxyDHCP OFFER（60/54/97/43+siaddr/file）被接受，ipxe.efi 已运行" || check "H1" 1 "未见 iPXE 二次 DHCP"
  check_ipxe_dhcp
}

sc_U6() {
  local tap="$1" pair
  for pair in virtio:c6 e1000e:c7; do
    boot_to_di U6 "${tap}" ovmf "${pair%%:*}" "${pair##*:}"
  done
  fixtures "ovmf-snponly" "$(mac_of c6)" "$(mac_of c7)"
  jsel "$(svc_log proxydhcp)" '.event=="reply" and (.target|contains("snponly.efi"))' | grep -q . && check "snponly" 0 "ProxyDHCP 按 MAC 下发 snponly.efi" || check "snponly" 1 "未下发 snponly.efi"
  local n; n="$(tftp_done snponly.efi)"
  [ "${n}" -ge 2 ] && check "snponly-tftp" 0 "snponly.efi 完整传输 ${n} 次" || check "snponly-tftp" 1 "snponly.efi 完整传输 ${n} 次"
}

sc_W() {
  local id="$1" tap="$2" nic mid
  case "${id}" in W1) nic=virtio mid=d1 ;; W2) nic=e1000e mid=d2 ;; esac
  boot_to_di "${id}" "${tap}" ovmf-http "${nic}" "${mid}"
  fixtures "ovmf-http-${nic}" "$(mac_of "${mid}")"
  local j; j="$(json_of discover-67)"
  [ -n "$j" ] && [ "$(jq -r .arch "$j")" = 16 ] && check "HTTPBoot首包" 0 "Opt93=16 $(jq -r .vendor_class "$j")" || check "HTTPBoot首包" 1 "未见 Opt93=16"
  jsel "$(svc_log serve)" '.event=="http" and .path=="/fw/x64/ipxe.efi" and .status==200' | grep -q . && check "H3" 0 "EDK2 HttpBoot 接受 Proxy OFFER，经带端口 URL 取到 ipxe.efi" || check "H3" 1 "未经 HTTP 取 ipxe.efi"
  ! has_stage request-4011 && check "H16" 0 "关闭 PXE 后只剩 HTTP Boot（无 PXE 4011）" || check "H16" 1 "仍有 PXE 流程"
}

sc_X() {
  local id="$1" tap="$2"
  if [ "${id}" = X1 ]; then boot_to_di X1 "${tap}" seabios virtio e1; fixtures existing-seabios-virtio "$(mac_of e1)"
  else boot_to_di X2 "${tap}" ovmf e1000e e2; fixtures existing-ovmf-e1000e "$(mac_of e2)"; fi
  "${LAB_BIN}" capture --pcap "${D}/capture.pcap" --out "${D}/all" --label all > "${D}/all-capture.log"
  local n from_box
  n="$(jq -r 'select(.event=="fixture")|.name' "${D}/all-capture.log" | wc -l)"
  from_box="$(grep -l '"src": "10.77.0.1:' "${D}"/all/*.json 2>/dev/null | wc -l || true)"
  [ "${from_box}" = 0 ] && check "无DHCP外发" 0 "抓包 ${n} 个 DHCP 报文中 box(10.77.0.1) 发出 0 个" || check "无DHCP外发" 1 "box 发出 ${from_box} 个 DHCP 报文"
}

sc_N1() {
  local tap="$1" mac; mac="$(mac_of f1)"
  local vmdir; vmdir="$("${LAB_DIR}/vm.sh" --name N1 --fw ovmf --nic virtio --mac "${mac}" --tap "${tap}")"
  sleep 120
  vm_stop "${vmdir}"; cp "${vmdir}/serial.log" "${D}/serial-f1.log"; [ "${KEEP}" = 1 ] || rm -rf "${vmdir}"
  fixtures ovmf-not-allowed "${mac}"
  [ "$(proxy_replied "${mac}")" = 0 ] && check "不应答" 0 "ProxyDHCP 对 ${mac} 应答 0 次" || check "不应答" 1 "ProxyDHCP 应答了 ${mac}"
  jsel "$(svc_log proxydhcp)" '.event=="ignore" and .reason=="mac-not-allowed" and .request.mac==$m' "${mac}" | grep -q . && check "记录忽略" 0 "日志有 mac-not-allowed" || check "记录忽略" 1 "无忽略记录"
  local e16 next
  e16="$(strings "${D}/serial-f1.log" | grep -c 'PXE-E16: No valid offer received' || true)"
  next="$(strings "${D}/serial-f1.log" | grep -A3 'failed to load Boot' | grep -c -E 'Start PXE over IPv6|BdsDxe: loading|Start HTTP' || true)"
  if [ "${e16}" -ge 1 ] && [ "${next}" -ge 1 ]; then check "回落" 0 "PXE-E16 超时后固件转入下一启动项"; else check "回落" 1 "E16=${e16} next=${next}"; fi
}


sc_N2_real() { # <tap> [bootif]
  local tap="$1" variant="${2:-mac}" m1 m2; m1="$(mac_of f2)" m2="$(mac_of f3)"
  # 第一块网卡接孤立 tap（无 DHCP），第二块接实验网并优先引导；d-i 必须选中第二块。
  # 变体 bootif：只靠 BOOTIF（H7 原假设）；变体 mac：另加 netcfg/choose_interface=<MAC>（H7 不成立时的取舍）。
  local hy; hy="$(echo "${m2}" | tr ':' '-')"
  if [ "${variant}" = mac ]; then
    cat > "${WORK}/www/node/${hy}.ipxe" <<NODE
#!ipxe
kernel ${BASE}/d-i/linux initrd=initrd.gz auto=true priority=critical url=${BASE}/preseed.cfg?mac=\${netX/mac:hexhyp} BOOTIF=01-\${netX/mac:hexhyp} netcfg/choose_interface=\${netX/mac} console=ttyS0,115200n8 ---
initrd ${BASE}/d-i/initrd.gz
boot
NODE
  fi
  local vmdir; vmdir="$("${LAB_DIR}/vm.sh" --name N2 --fw ovmf --nic virtio --mac "${m1}" --tap "${TAP_ISOLATED}" \
    --nic2 virtio --mac2 "${m2}" --tap2 "${tap}" --boot nic2-first)"
  if wait_early "${m2}" 240; then check "双网卡(${variant})" 0 "d-i 在第二块网卡（${m2}）上完成网络配置并取到 preseed"
  else check "双网卡(${variant})" 1 "未在第二块网卡上进入 d-i（见 serial：是否出现 Primary network interface 提问）"; fi
  vm_stop "${vmdir}"; cp "${vmdir}/serial.log" "${D}/serial-f3-${variant}.log"; [ "${KEEP}" = 1 ] || rm -rf "${vmdir}"
  rm -f "${WORK}/www/node/${hy}.ipxe"
  fixtures "ovmf-dualnic-${variant}" "${m2}"
}

sc_B6() {
  local tap="$1" mac; mac="$(mac_of b6)"
  "${LAB_DIR}/net.sh" probe-ns
  sudo ip netns exec ns-probe "${LAB_BIN}" pxe-client --iface vp-probe --legacy --mac "${mac}" --wait 3 \
    --save "${D}/fixtures" --label constructed-bios > "${D}/pxe-client.log"
  if jq -e 'select(.phase=="offer" and .reply.proxy and .reply.file=="undionly.kpxe" and .reply.siaddr=="10.77.0.1")' "${D}/pxe-client.log" > /dev/null; then
    check "B6①" 0 "构造的 BIOS 非 iPXE DISCOVER 得到 file=undionly.kpxe、siaddr=10.77.0.1"
  else check "B6①" 1 "应答不符：$(cat "${D}/pxe-client.log")"; fi
  boot_to_di B6 "${tap}" seabios virtio b6 --kernel /usr/lib/ipxe/ipxe.lkrn --initrd "${LAB_DIR}/ipxe/legacy-sim.ipxe"
  [ "$(tftp_done undionly.kpxe)" -ge 1 ] && check "H15" 0 "ipxe.lkrn 经 TFTP 加载 undionly.kpxe 并运行到 d-i" || check "H15" 1 "undionly.kpxe 未完整传输"
}

sc_H9() { # preseed/url/checksum：H9a 正确 md5 应进入；H9b 错误 md5 应被拒绝
  local id="$1" tap="$2" mid sum
  if [ "${id}" = H9a ]; then mid=91; sum="$(md5sum "${WORK}/www/preseed.cfg" | cut -d' ' -f1)"
  else mid=92; sum="$(printf 'tampered' | md5sum | cut -d' ' -f1)"; fi
  local hy; hy="$(mac_of "${mid}" | tr ':' '-')"
  cat > "${WORK}/www/node/${hy}.ipxe" <<NODE
#!ipxe
kernel ${BASE}/d-i/linux initrd=initrd.gz auto=true priority=critical url=${BASE}/preseed.cfg?mac=\${netX/mac:hexhyp} preseed/url/checksum=${sum} console=ttyS0,115200n8 ---
initrd ${BASE}/d-i/initrd.gz
boot
NODE
  if [ "${id}" = H9a ]; then
    boot_to_di H9a "${tap}" seabios virtio "${mid}"
  else
    BOOT_TIMEOUT=180 boot_to_di H9b "${tap}" seabios virtio "${mid}"
    # 期望：未进入（上面的检查记为 FAIL，这里翻转）
    local last="${CHECKS[-1]}"; unset 'CHECKS[-1]'
    if [[ "${last}" == FAIL* ]] && jsel "$(svc_log serve)" '.event=="http" and .path=="/preseed.cfg" and ((.query//"")|contains($m))' "${hy}" | grep -q .; then check "H9" 0 "md5 不符时 d-i 取到文件但拒绝执行（无 early 事件）"
    else check "H9" 1 "篡改后仍执行或根本未取文件"; fi
  fi
  rm -f "${WORK}/www/node/${hy}.ipxe"
}

sc_G1() {
  "${LAB_DIR}/dhcp.sh" start rogue; sleep 3
  sudo "${LAB_BIN}" dhcp-scan --iface "${BR}" --wait 4 > "${D}/scan.log"
  "${LAB_DIR}/dhcp.sh" stop rogue
  local servers rogue legit
  servers="$(jq -r 'select(.phase=="offer")|.reply.server_id' "${D}/scan.log" | sort -u | tr '\n' ' ')"
  rogue="$(jq -r 'select(.phase=="offer" and .reply.server_id=="10.77.0.3")|.reply.boot_params' "${D}/scan.log")"
  legit="$(jq -r 'select(.phase=="offer" and .reply.server_id=="10.77.0.2")|.reply.boot_params' "${D}/scan.log")"
  [ "${servers}" = "10.77.0.2 10.77.0.3 " ] && check "两台服务器" 0 "dhcp-scan 列出 ${servers}" || check "两台服务器" 1 "列出：${servers}"
  [ "${rogue}" = true ] && [ "${legit}" = false ] && check "标记伪造" 0 "lab-rogue 被标记为下发引导参数，lab-dhcp 未下发" || check "标记伪造" 1 "rogue=${rogue} legit=${legit}"
}

run_one() { # <id> <tap>
  local id="$1" tap="$2"
  begin "${id}"
  case "${id}" in
    B[1-5]) sc_B "${id}" "${tap}" ;;
    B6) sc_B6 "${tap}" ;;
    U[1-5]) sc_U "${id}" "${tap}" ;;
    U6) sc_U6 "${tap}" ;;
    W[12]) sc_W "${id}" "${tap}" ;;
    X[12]) sc_X "${id}" "${tap}" ;;
    N1) sc_N1 "${tap}" ;;
    N2) sc_N2_real "${tap}" mac ;;
    N2b) sc_N2_real "${tap}" bootif ;;
    H9a|H9b) sc_H9 "${id}" "${tap}" ;;
    G1) sc_G1 ;;
    R1|R2) source "${LAB_DIR}/lib/redfish.sh"; sc_R "${id}" "${tap}" ;;
    *) check "未知场景" 1 "${id}" ;;
  esac
  finish "${id}"
}

mode_for() { case "$1" in X[12]|R2) echo noproxy ;; *) echo proxy ;; esac; }

run_many() {
  local ids=("$@") slots=() i id k
  for ((k = 0; k < JOBS; k++)); do slots[k]=""; done
  for id in "${ids[@]}"; do
    while :; do
      for ((k = 0; k < JOBS; k++)); do
        if [ -z "${slots[k]}" ] || ! kill -0 "${slots[k]}" 2>/dev/null; then break 2; fi
      done
      sleep 2
    done
    ( run_one "${id}" "${TAPS[k]}" ) > "${OUT}/${id}.log" 2>&1 &
    slots[k]=$!
    log "场景 ${id} 启动于 ${TAPS[k]}"
    sleep 5
  done
  for k in "${slots[@]}"; do [ -n "$k" ] && wait "$k" 2>/dev/null || true; done
  for id in "${ids[@]}"; do cat "${OUT}/${id}/result" 2>/dev/null || echo "${id} FAIL（无结果，见 ${OUT}/${id}.log）"; done
}

collect_fixtures() {
  local dst="${REPO_DIR}/crates/xpxe-dhcp/tests/fixtures" f
  mkdir -p "${dst}"
  for f in "${OUT}"/*/fixtures/*.bin; do
    [ -e "$f" ] || continue
    cp "$f" "${f%.bin}.json" "${dst}/"
  done
  log "夹具：$(ls "${dst}"/*.bin | wc -l) 个 → ${dst}"
}

while [ $# -gt 0 ]; do
  case "$1" in -j) JOBS="$2"; shift 2 ;; --keep) KEEP=1; shift ;; *) break ;; esac
done
[ $# -gt 0 ] || { echo "用法：lab/run.sh [-j N] [--keep] <场景…>|all|capture|services|stop-services" >&2; exit 2; }
case "$1" in
  capture) collect_fixtures; exit 0 ;;
  services) services "${2:-proxy}"; exit 0 ;;
  stop-services) stop_services; exit 0 ;;
  all) set -- B1 B2 B3 B4 B5 B6 U1 U2 U3 U4 U5 U6 W1 W2 N1 N2 H9a H9b ;;
esac
[ "${JOBS}" -le "${#TAPS[@]}" ] || die "-j 最多 ${#TAPS[@]}"
mode="$(mode_for "$1")"
for id in "$@"; do [ "$(mode_for "${id}")" = "${mode}" ] || die "场景 ${id} 与 $1 的服务模式不同，请分开运行"; done
services "${mode}"
run_many "$@"
