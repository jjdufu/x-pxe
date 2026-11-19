#!/usr/bin/env bash
# 场景编排：lab/run.sh [-j N] [--keep] [--www <目录>] [--bootnet <xpxe-bootnet>] [--iso <boot.iso>] <场景…>|all|capture|services|stop-services
# --bootnet：由正式 xpxe-bootnet（+ xpxe-lab control-stub 充当控制面）代替 proxydhcp/tftp 探针应答（Step 8）
# Step 8 场景（需 --bootnet）：rogue-proxy（先 lab/dhcp.sh start rogue）| redfish-only-ports
# Step 9 场景：R1/R2（--iso <boot.iso> 覆盖 R1 的会话 ISO）| redfish-bios-reject | redfish-pin-change
# Step 4 装机场景：install-uefi | install-bios | install-static（完整跑完 d-i 并从系统盘重启）
# Step 5 初始化等价性：init-equivalence [--plan <plan.json>] [--legacy <脚本>]（lab/lib/init-equivalence.sh）
# 每个场景输出 PASS/FAIL、耗时与日志目录（lab/out/<场景>/）。
source "$(dirname "$0")/lib/common.sh"
source "$(dirname "$0")/lib/init-equivalence.sh"
JOBS=1 KEEP=0 WWW_OVERRIDE="" BOOTNET="" ISO_OVERRIDE=""
MODE_FILE="${WORK}/bootnet-mode"
TOKEN_FILE="${WORK}/bootnet-token"
BASE="http://${LAB_IP}:8080"
mac_of() { echo "52:54:00:77:01:$1"; }
# ProxyDHCP 允许列表：除 N1（f1）以外的全部实验 MAC
ALLOW_IDS=(b1 b2 b3 b4 b5 b6 c1 c2 c3 c4 c5 c6 c7 d1 d2 e1 e2 f2 f3 91 92 a1 a2 14 15 16)
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
  if [ -n "${BOOTNET}" ]; then bootnet_services "$1"; return; fi
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

# 正式 bootnet：control-stub 提供内部 API，bootnet 以 UID 10001 + 仅 NET_BIND_SERVICE 运行
bootnet_services() { # proxy|noproxy
  local T="${WORK}/tftp" args=() i mode=proxy_dhcp
  [ "$1" = proxy ] || mode=external_dhcp
  for s in proxydhcp tftp bootnet control; do svc_stop "$s"; done
  sleep 1
  echo "${mode}" > "${MODE_FILE}"
  [ -s "${TOKEN_FILE}" ] || head -c 24 /dev/urandom | base64 | tr -d '/+=\n' > "${TOKEN_FILE}"
  chmod 0644 "${TOKEN_FILE}"
  for i in "${ALLOW_IDS[@]}"; do
    args+=(--allow-mac "$(mac_of "$i")")
    # control-stub 的 boot_token = lab-<MAC 去冒号>，iPXE 取 <BASE>/p/<token>/boot.ipxe
    mkdir -p "${WORK}/www/p/lab-$(mac_of "$i" | tr -d :)"
    cp "${WORK}/www/boot.ipxe" "${WORK}/www/p/lab-$(mac_of "$i" | tr -d :)/boot.ipxe"
  done
  for i in "${SNPONLY_IDS[@]}"; do args+=(--efi-file-for "$(mac_of "$i")=snponly.efi"); done
  svc_start control user "${LAB_BIN}" control-stub --listen 127.0.0.1:8081 --token-file "${TOKEN_FILE}" \
    --iface "${BR}" --boot-base "${BASE}" --mode-file "${MODE_FILE}" \
    --undionly "${T}/undionly.kpxe" --ipxe-efi "${T}/ipxe.efi" --snponly "${T}/snponly.efi" "${args[@]}"
  sudo install -m 0755 "${BOOTNET}" "${LAB_BIN_DIR}/xpxe-bootnet"
  svc_start bootnet root "${SETPRIV[@]}" env XPXE_CONTROL_URL=http://127.0.0.1:8081 \
    XPXE_BOOTNET_TOKEN_FILE="${TOKEN_FILE}" XPXE_BOOTNET_ALLOWED_IFACES="${BR}" "${LAB_BIN_DIR}/xpxe-bootnet" run
  wait_heartbeat "${mode}" 60 || die "bootnet 60 秒内未报告 ${mode} 心跳（见 $(svc_log bootnet)）"
  log "bootnet 就绪：$(last_heartbeat | jq -c '{mode,health,listening,detail}')"
}
last_heartbeat() { jq -R -c 'fromjson? | select(.event=="heartbeat") | .heartbeat' "$(svc_log control)" | tail -1; }
wait_heartbeat() { # <mode> <秒> [health]
  local deadline=$((SECONDS + $2)) hb
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    hb="$(last_heartbeat)"
    if [ -n "${hb}" ] && [ "$(jq -r .mode <<< "${hb}")" = "$1" ] && { [ -z "${3:-}" ] || [ "$(jq -r .health <<< "${hb}")" = "$3" ]; }; then return 0; fi
    sleep 2
  done
  return 1
}

stop_services() { for s in proxydhcp tftp bootnet control serve sushy apt-proxy; do svc_stop "$s"; done; }

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
# 观测断言：探针模式读 proxydhcp/tftp 日志；--bootnet 模式读 control-stub 收到的观测
proxy_replied() {
  if [ -n "${BOOTNET}" ]; then jsel "$(svc_log control)" '.event=="observation" and .kind=="dhcp_reply" and .request.mac==$m' "$1" | wc -l
  else jsel "$(svc_log proxydhcp)" '.event=="reply" and .request.mac==$m' "$1" | wc -l; fi
}
tftp_done() {
  if [ -n "${BOOTNET}" ]; then jsel "$(svc_log control)" '.event=="observation" and .kind=="tftp" and .stage=="completed" and (.file|endswith($m))' "$1" | wc -l
  else jsel "$(svc_log tftp)" '.fields.path==$m and .fields.completed==true' "$1" | wc -l; fi
}
replied_file() { # <子串>：是否有下发该文件的应答
  if [ -n "${BOOTNET}" ]; then jsel "$(svc_log control)" '.event=="observation" and .kind=="dhcp_reply" and (.file|contains($m))' "$1" | grep -q .
  else jsel "$(svc_log proxydhcp)" '.event=="reply" and (.target|contains($m))' "$1" | grep -q .; fi
}
ignored_unarmed() { # <mac>
  if [ -n "${BOOTNET}" ]; then jsel "$(svc_log control)" '.event=="observation" and .kind=="dhcp_ignore" and .reason=="not_armed" and .request.mac==$m' "$1" | grep -q .
  else jsel "$(svc_log proxydhcp)" '.event=="ignore" and .reason=="mac-not-allowed" and .request.mac==$m' "$1" | grep -q .; fi
}

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
  sleep 3 # bootnet 观测每 2 秒批量上报
  replied_file snponly.efi && check "snponly" 0 "ProxyDHCP 按 MAC 下发 snponly.efi" || check "snponly" 1 "未下发 snponly.efi"
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
  # 只统计服务端应答（源端口 67/4011）；bootnet 的现网探测 DISCOVER 源端口为 68，不算
  from_box="$(grep -lE '"src": "10\.77\.0\.1:(67|4011)"' "${D}"/all/*.json 2>/dev/null | wc -l || true)"
  [ "${from_box}" = 0 ] && check "无DHCP外发" 0 "抓包 ${n} 个 DHCP 报文中 box(10.77.0.1) 发出 0 个" || check "无DHCP外发" 1 "box 发出 ${from_box} 个 DHCP 报文"
}

sc_N1() {
  local tap="$1" mac; mac="$(mac_of f1)"
  local vmdir; vmdir="$("${LAB_DIR}/vm.sh" --name N1 --fw ovmf --nic virtio --mac "${mac}" --tap "${tap}")"
  sleep 120
  vm_stop "${vmdir}"; cp "${vmdir}/serial.log" "${D}/serial-f1.log"; [ "${KEEP}" = 1 ] || rm -rf "${vmdir}"
  fixtures ovmf-not-allowed "${mac}"
  [ "$(proxy_replied "${mac}")" = 0 ] && check "不应答" 0 "ProxyDHCP 对 ${mac} 应答 0 次" || check "不应答" 1 "ProxyDHCP 应答了 ${mac}"
  ignored_unarmed "${mac}" && check "记录忽略" 0 "日志有未武装忽略记录" || check "记录忽略" 1 "无忽略记录"
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
  sudo chown -R "$(id -u):$(id -g)" "${D}/fixtures" # root 写入的夹具，避免下次 begin 清理失败
  if jq -e 'select(.phase=="offer" and .reply.proxy and (.reply.file=="undionly.kpxe" or (.reply.file|test("^fw/[0-9a-f]+/undionly\\.kpxe$"))) and .reply.siaddr=="10.77.0.1")' "${D}/pxe-client.log" > /dev/null; then
    check "B6①" 0 "构造的 BIOS 非 iPXE DISCOVER 得到 file=[fw/<摘要>/]undionly.kpxe、siaddr=10.77.0.1"
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


# ---------- Step 4：渲染结果真实装机 ----------
# 实验室把 nexus-lvm 预设的镜像源改为 APT 缓存（虚拟机没有外网），其余条目保持预设。
LAB_MIRROR='/mirror={"scheme":"http","host_port":"10.77.0.1:3142","directory":"/deb.debian.org/debian"}'
apt_ok() { curl -s -m 30 -o /dev/null -w '%{http_code}' "http://${LAB_IP}:3142/deb.debian.org/debian/dists/trixie/Release" | grep -qx 200; }
acng_up() { # 优先 apt-cacher-ng 容器；宿主网络下它回源失败（HTTP 500）时换成 lab/lib/apt-proxy.py
  if docker ps --format '{{.Names}}' | grep -qx lab-acng; then acng_matches && apt_ok && return 0
  elif [ -z "$(acng_upstream)" ]; then apt_ok && return 0; fi
  acng_run
  sleep 2
  apt_ok && return 0
  log "lab-acng 回源失败，改用 apt-proxy.py"
  docker rm -f lab-acng >/dev/null 2>&1 || true
  svc_start apt-proxy user python3 "${LAB_DIR}/lib/apt-proxy.py" "${LAB_IP}" 3142 "${WORK}/apt-cache"
  apt_ok || die "APT 缓存不可用"
}
post_seen() { # <路径> <event_id>；只看本场景开始之后的日志，避免同一 event_id 的旧记录误判
  tail -n "+${SERVE_OFF:-1}" "$(svc_log serve)" | jq -R -c --arg m "$1" 'fromjson? | select(.event=="post" and .path==$m)' \
    | grep -F -- "$2" | grep -q .
}
wait_post() { # <路径> <event_id> <秒>
  local deadline=$((SECONDS + $3))
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    post_seen "$1" "$2" && return 0
    post_seen /events/installer.failed "$2" && return 1
    sleep 5
  done
  return 1
}
sc_I() { # <install-uefi|install-bios|install-static> <tap>
  local id="$1" tap="$2" node fw mid
  case "${id}" in
    install-uefi) node=vm-uefi fw=ovmf mid=14 ;;
    install-bios) node=vm-bios fw=seabios mid=15 ;;
    install-static) node=vm-static fw=ovmf mid=16 ;;
  esac
  local mac hy token agent_token ev r
  mac="$(mac_of "${mid}")" hy="$(mac_of "${mid}" | tr ':' '-')"
  token="labboot${mid}" agent_token="labagent${mid}" ev="lab-labboot${mid}"
  r="${WORK}/www/p/${token}"
  rm -rf "${r}" "${WORK}/www/p/${agent_token}"; mkdir -p "${r}" "${WORK}/www/p/${agent_token}"
  if [ -n "${WWW_OVERRIDE}" ]; then
    cp -a "${WWW_OVERRIDE}/." "${r}/"
  else
    "${LAB_BIN}" render --preset nexus-lvm --node "${LAB_DIR}/nodes/${node}.json" --boot-base "${BASE}" \
      --token "${token}" --agent-token "${agent_token}" --set "${LAB_MIRROR}" --out "${r}"
  fi
  mv "${r}/plan.json" "${WORK}/www/p/${agent_token}/plan.json"
  ln -sf ../../d-i/linux "${r}/linux"; ln -sf ../../d-i/initrd.gz "${r}/initrd.gz"
  cp "${r}/boot.ipxe" "${WORK}/www/node/${hy}.ipxe"
  if [ "${DI_SYSLOG:-0}" = 1 ]; then # 排障：partman 阶段起每 20 秒把 d-i syslog 尾部 POST 回来（只改实验室文件，不影响 preseed MD5）
    printf '(while sleep 20; do wget -q -O /dev/null --post-data "$(tail -n 300 /var/log/syslog)" %s/events/di-syslog-%s; done) >/dev/null 2>&1 &\n' \
      "${BASE}" "${mid}" | sed -i '1r /dev/stdin' "${r}/partman-early.sh"
  fi
  cp -a "${r}" "${D}/rendered"
  # 系统盘 20G；数据盘 8G 低于下限；1G BMC 虚拟 U 盘（usb-storage，removable）必须被排除
  local vmdir; vmdir="$("${LAB_DIR}/vm.sh" --name "${id}" --fw "${fw}" --nic virtio --mac "${mac}" --tap "${tap}" \
    --boot disk-first --disks 20G,8G)"
  if wait_post /events/installer.started "${ev}" "${BOOT_TIMEOUT:-300}"; then check "installer.started" 0 "收到 POST（${ev}）"
  else check "installer.started" 1 "超时未收到"; fi
  if wait_post /events/installer.finished "${ev}" "${INSTALL_TIMEOUT:-3600}"; then check "installer.finished" 0 "d-i 完成（选盘、分区、late_command 均成功）"
  elif post_seen /events/installer.failed "${ev}"; then check "installer.finished" 1 "收到 installer.failed：$(jsel "$(svc_log serve)" '.path=="/events/installer.failed"' | tail -1)"
  else check "installer.finished" 1 "超时"; fi
  rm -f "${WORK}/www/node/${hy}.ipxe"
  # 重启后从系统盘起来：sshd（tasksel ssh-server）应答横幅即视为进入装好的系统。
  # 静态场景直接连节点规格地址；DHCP 场景从 br-lab 邻居表按 MAC 找地址。串口另需出现登录提示（--- 后的 console= 被带入目标系统）。
  local deadline=$((SECONDS + 600)) up=1 ip=""
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    if [ "${id}" = install-static ]; then ip=10.77.0.16
    else ip="$(ip neigh show dev "${BR}" | awk -v m="${mac}" 'tolower($0) ~ m {print $1; exit}')"; fi
    if [ -n "${ip}" ] && timeout 5 bash -c "exec 3<>/dev/tcp/${ip}/22 && head -c 7 <&3" 2>/dev/null | grep -q '^SSH-2.0'; then up=0; break; fi
    sleep 10
  done
  check "重启进系统" "${up}" "从系统盘启动，${ip:-?}:22 返回 SSH 横幅"
  if strings "${vmdir}/serial.log" | grep -q "lab-${id#install-} login:"; then check "串口控制台" 0 "装好的系统串口出现 lab-${id#install-} login:"
  else check "串口控制台" 1 "串口未见登录提示"; fi
  [ "${id}" = install-static ] && check "静态地址" "${up}" "10.77.0.16（节点规格地址）在重启后可达，/etc/network/interfaces 为静态配置"
  local usb sys
  usb="$(du -k "${vmdir}/vusb.raw" | cut -f1)" sys="$(du -k "${vmdir}/sys.qcow2" | cut -f1)"
  [ "${usb}" = 0 ] && [ "${sys}" -gt 500000 ] && check "选盘" 0 "系统盘写入 ${sys} KiB，BMC 虚拟 U 盘未写入" \
    || check "选盘" 1 "sys=${sys}KiB vusb=${usb}KiB"
  vm_stop "${vmdir}"; cp "${vmdir}/serial.log" "${D}/serial-${mid}.log" 2>/dev/null || true
  [ "${KEEP}" = 1 ] || rm -rf "${vmdir}"
}

# 冒充 ProxyDHCP 的 lab-rogue（10.77.0.3）在线：bootnet 必须进入 conflict 并暂停应答
sc_rogue() {
  [ -n "${BOOTNET}" ] || { check "前提" 1 "需要 --bootnet"; return; }
  docker inspect -f '{{.State.Running}}' lab-rogue 2>/dev/null | grep -q true || { check "前提" 1 "先运行 lab/dhcp.sh start rogue"; return; }
  if wait_heartbeat proxy_dhcp 60 conflict; then
    local hb; hb="$(last_heartbeat)"
    check "冲突" 0 "health=conflict detail=$(jq -r .detail <<< "${hb}")"
    jq -e '.probe.conflicts | index("10.77.0.3")' <<< "${hb}" > /dev/null && check "冲突地址" 0 "心跳报告冲突服务器 10.77.0.3" || check "冲突地址" 1 "心跳未列出 10.77.0.3：$(jq -c .probe <<< "${hb}")"
  else check "冲突" 1 "60 秒内未见 conflict 心跳：$(last_heartbeat)"; fi
  local mac before after; mac="$(mac_of b1)"
  before="$(proxy_replied "${mac}")"
  sudo "${LAB_BIN}" pxe-client --legacy --iface "${BR}" --mac "${mac}" > "${D}/pxe-client.log" 2>&1 || true
  sleep 4; after="$(proxy_replied "${mac}")"
  [ "${before}" = "${after}" ] && check "暂停应答" 0 "冲突期间对已武装 ${mac} 应答 0 次" || check "暂停应答" 1 "冲突期间仍应答 ${mac}"
}

# 切换到 redfish_only 后 67/69/4011 全部释放，切回后恢复
sc_redfish_ports() {
  [ -n "${BOOTNET}" ] || { check "前提" 1 "需要 --bootnet"; return; }
  ports() { sudo ss -H -ulnp | grep -E ':(67|69|4011)\b' | grep -c xpxe-bootnet || true; }
  [ "$(ports)" -ge 3 ] && check "初始监听" 0 "proxy_dhcp：$(ports) 个引导端口" || check "初始监听" 1 "proxy_dhcp 只有 $(ports) 个端口"
  echo redfish_only > "${MODE_FILE}"
  if wait_heartbeat redfish_only 40; then
    sleep 1
    [ "$(ports)" = 0 ] && check "释放" 0 "redfish_only：ss -ulnp 中无 :67 :69 :4011" || check "释放" 1 "仍有 $(ports) 个端口：$(sudo ss -H -ulnp | grep xpxe-bootnet)"
  else check "释放" 1 "40 秒内未切换到 redfish_only"; fi
  jsel "$(svc_log control)" '.event=="observation" and .kind=="mode_changed" and .to=="redfish_only"' | grep -q . && check "切换观测" 0 "mode_changed → redfish_only 已上报" || check "切换观测" 1 "无 mode_changed 观测"
  echo proxy_dhcp > "${MODE_FILE}"
  wait_heartbeat proxy_dhcp 40 && [ "$(ports)" -ge 3 ] && check "恢复" 0 "切回 proxy_dhcp 后端口恢复" || check "恢复" 1 "切回后端口 $(ports) 个"
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
    redfish-bios-reject) source "${LAB_DIR}/lib/redfish.sh"; sc_bios_reject ;;
    redfish-pin-change) source "${LAB_DIR}/lib/redfish.sh"; sc_pin_change ;;
    install-uefi|install-bios|install-static) sc_I "${id}" "${tap}" ;;
    rogue-proxy) sc_rogue ;;
    redfish-only-ports) sc_redfish_ports ;;
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
  case "$1" in -j) JOBS="$2"; shift 2 ;; --bootnet) BOOTNET="$(realpath "$2")"; shift 2 ;; --iso) ISO_OVERRIDE="$(realpath "$2")"; shift 2 ;; --keep) KEEP=1; shift ;; --www) WWW_OVERRIDE="$2"; shift 2 ;; *) break ;; esac
done
[ $# -gt 0 ] || { echo "用法：lab/run.sh [-j N] [--keep] <场景…>|all|capture|services|stop-services" >&2; exit 2; }
case "$1" in
  capture) collect_fixtures; exit 0 ;;
  init-equivalence) shift; services noproxy; acng_up; ieq_main "$@"; exit 0 ;;
  services) services "${2:-proxy}"; exit 0 ;;
  stop-services) stop_services; exit 0 ;;
  install-all) set -- install-uefi install-bios install-static ;;
  all) set -- B1 B2 B3 B4 B5 B6 U1 U2 U3 U4 U5 U6 W1 W2 N1 N2 H9a H9b ;;
esac
[ "${JOBS}" -le "${#TAPS[@]}" ] || die "-j 最多 ${#TAPS[@]}"
mode="$(mode_for "$1")"
for id in "$@"; do [ "$(mode_for "${id}")" = "${mode}" ] || die "场景 ${id} 与 $1 的服务模式不同，请分开运行"; done
services "${mode}"
case "$1" in install-*) acng_up ;; esac
run_many "$@"
