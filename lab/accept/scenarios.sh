#!/usr/bin/env bash
# scripts/accept.sh 的验收场景 A–R（计划 docs/plan/step-13-deploy-and-acceptance.md 第 6.1 节）。由 accept.sh source。
INSTALL_TIMEOUT="${INSTALL_TIMEOUT:-5400}"   # 单台从 arm 到 Ready 的上限（TCG 下更慢）
BOOT_TIMEOUT="${BOOT_TIMEOUT:-900}"          # 开机到 installer.started 的上限

vm_start() { # <名字> <固件> <网卡> <mac> <tap> [vm.sh 额外参数…] → 打印 VM 目录
  local name="$1" fw="$2" nic="$3" mac="$4" tap="$5"; shift 5
  "${LAB_DIR}/vm.sh" --name "acc-${name}" --fw "${fw}" --nic "${nic}" --mac "${mac}" --tap "${tap}" "$@" 2>> "${D}/vm.log" | tail -1
}
vm_stop() { local d="$1"; [ -f "${d}/qemu.pid" ] && kill "$(cat "${d}/qemu.pid")" 2>/dev/null || true; sleep 2; }
vm_keep_serial() { cp "$1/serial.log" "${D}/serial-$2.log" 2>/dev/null || true; }
vm_reboot() { # <VM 目录>：经 QEMU 监视器 system_reset
  echo system_reset | socat - "unix-connect:$1/mon.sock" > /dev/null 2>&1
}
timeline() { api GET "/nodes/$1/timeline" | jq -c '.[]?' | tac; }
has_event() { timeline "$1" | jq -e --arg k "$2" 'select(.kind==$k)' > /dev/null; }

# 计划里 packages 模块要装的包（预览脚本中的 apt-get install 列表）
plan_packages() {
  api POST /configs/init_plan/preview "$(jq -n --argjson d "$(api GET "/configs/init_plan/$(ids plan)" | jq .document)" \
    --argjson v "$(api GET "/configs/init_plan/$(ids plan)" | jq .version.version)" --argjson o "$(ids plan)" \
    '{document:$d,base:{object_id:$o,version:$v}}')" \
    | jq -r '.modules[] | select(.name=="packages") | .text' | sed -nE "s/^  '([^']+)'.*/\1/p" | sort -u
}
check_packages() { # <ip> <标签>
  plan_packages > "${D}/plan-packages.txt"
  tssh "$1" "dpkg-query -W -f='\${Package} \${db:Status-Status}\n'" | awk '$2=="installed"{print $1}' | sort -u > "${D}/dpkg-$2.txt"
  local missing; missing="$(comm -23 "${D}/plan-packages.txt" "${D}/dpkg-$2.txt" | tr '\n' ' ')"
  [ -s "${D}/plan-packages.txt" ] && [ -z "${missing}" ] \
    && check "包清单($2)" 0 "计划 $(wc -l < "${D}/plan-packages.txt") 个包全部已安装（dpkg-query）" \
    || check "包清单($2)" 1 "缺少：${missing:-计划包清单为空}"
}

# 登记 + arm + 开机 + 等 Ready。全局输出：NODE、VMDIR、IP
# 两段式：install_start 登记 + arm + 开机后立即返回；install_wait 等到 Ready 并 SSH。
# 同一场景的几台机器先全部 install_start 再逐台 install_wait，TCG 下可以并行装机。
declare -A N_NODE N_VM N_MAC N_T0
install_start() { # <标签> <主机名> <uefi|bios> <固件> <网卡> <mac> <tap> [vm.sh 额外参数…]
  local tag="$1" host="$2" fwmode="$3" fw="$4" nic="$5" mac="$6" tap="$7"; shift 7
  local node armed
  node="$(register_node "${host}" "${mac}" "${fwmode}")"
  armed="$(api POST "/nodes/${node}/arm" '{}' | jq -r .to)"
  [ "${armed}" = armed ] && check "arm(${tag})" 0 "节点 ${node} ${host} ${mac} → Armed" || { check "arm(${tag})" 1 "arm 失败：${armed}"; return 1; }
  N_NODE[${tag}]="${node}"; N_MAC[${tag}]="${mac}"; N_T0[${tag}]=${SECONDS}
  N_VM[${tag}]="$(vm_start "${tag}" "${fw}" "${nic}" "${mac}" "${tap}" "$@")"
}
install_wait() { # <标签> → NODE VMDIR IP
  local tag="$1"; NODE="${N_NODE[${tag}]:-}"; VMDIR="${N_VM[${tag}]:-}"; IP=""
  [ -n "${NODE}" ] || return 1
  local left=$(( INSTALL_TIMEOUT - (SECONDS - N_T0[${tag}]) )); [ "${left}" -gt 0 ] || left=1
  if wait_state "${NODE}" "${left}" ready; then
    check "Ready(${tag})" 0 "无人工输入：$(timeline "${NODE}" | jq -r .kind | grep -vE '^effect' | tr '\n' ' ')（$((SECONDS - N_T0[${tag}]))s）"
  else
    check "Ready(${tag})" 1 "最后状态 $(node_state "${NODE}")：$(timeline "${NODE}" | jq -r '"\(.kind) \(.payload.reason // "")"' | tr '\n' ';' | cut -c1-300)"
    vm_keep_serial "${VMDIR}" "${tag}"; return 1
  fi
  IP="$(wait_ssh "${N_MAC[${tag}]}" 600 || true)"
  [ -n "${IP}" ] && check "SSH(${tag})" 0 "${IP} 以实验室密钥登录 root" || check "SSH(${tag})" 1 "Ready 后 600s 内无法 SSH"
}
install_node() { # 单台：install_start + install_wait
  install_start "$@" && install_wait "$1"
}

# ---------- A / B：LAN 装机 ----------
sc_lan() { # <A|B>
  local id="$1" pairs p i=0 ips=()
  if [ "${id}" = A ]; then pairs="seabios:virtio:11 seabios:e1000:12"; else pairs="ovmf:e1000e:21 ovmf:vmxnet3:22"; fi
  set_bootnet proxy_dhcp || check "引导网络" 1 "bootnet 未进入 proxy_dhcp"
  local tags=() macs=()
  for p in ${pairs}; do
    IFS=: read -r fw nic m <<< "${p}"
    local mode=bios; [ "${fw}" = seabios ] || mode=uefi
    install_start "${id}-${nic}" "acc-${id,,}-${nic}" "${mode}" "${fw}" "${nic}" "$(mac_of "${m}")" "${TAPS[i]}" \
      && tags+=("${id}-${nic}") && macs+=("$(mac_of "${m}")")
    i=$((i + 1))
  done
  for i in "${!tags[@]}"; do
    if install_wait "${tags[i]}"; then
      [ -n "${IP}" ] && check_packages "${IP}" "${tags[i]#*-}"
      [ -n "${IP}" ] && tssh "${IP}" 'echo xpxe-accept > /root/xpxe-accept-marker'
      echo "${NODE} ${VMDIR} ${IP} ${macs[i]}" >> "${ACC}/installed-${id}"
    fi
  done
  if [ "${id}" = B ]; then
    local r4011 tftp
    r4011="$(tcpdump -nr "${D}/capture.pcap" 'udp port 4011' 2>/dev/null | wc -l)"
    tftp="$(tcpdump -nr "${D}/capture.pcap" -A 'udp port 69' 2>/dev/null | grep -c 'ipxe.efi' || true)"
    [ "${r4011}" -ge 2 ] && check "4011" 0 "抓包中 4011 端口报文 ${r4011} 个（REQUEST/ACK）" || check "4011" 1 "4011 报文 ${r4011} 个"
    [ "${tftp}" -ge 1 ] && check "TFTP ipxe.efi" 0 "抓包可见 TFTP RRQ ipxe.efi" || check "TFTP ipxe.efi" 1 "未见 ipxe.efi 的 TFTP 读请求"
  fi
}

# ---------- C：NIC 矩阵（只到 d-i 取走 preseed，然后关机并 Reset 回 Idle） ----------
boot_to_installing() { # <标签> <bios|uefi> <固件> <网卡> <mac> <tap> [vm 额外参数…]
  local tag="$1" mode="$2" fw="$3" nic="$4" mac="$5" tap="$6"; shift 6
  local node; node="$(register_node "acc-c-${tag,,}" "${mac}" "${mode}")"
  api POST "/nodes/${node}/arm" '{}' > /dev/null
  local vm; vm="$(vm_start "c-${tag}" "${fw}" "${nic}" "${mac}" "${tap}" "$@")"
  if wait_state "${node}" "${BOOT_TIMEOUT}" installing installed; then check "${tag}" 0 "${fw}/${nic}：preseed 已取走（installer.started）"
  else check "${tag}" 1 "${fw}/${nic}：$(node_state "${node}")"; vm_keep_serial "${vm}" "${tag}"; fi
  vm_stop "${vm}"
  api POST "/nodes/${node}/reset" '{}' > /dev/null
  [ "$(node_state "${node}")" = idle ] || check "${tag} Reset" 1 "Reset 后不是 idle"
  [ "${KEEP:-0}" = 1 ] || rm -rf "${vm}"
}
sc_C() {
  set_bootnet proxy_dhcp > /dev/null
  local spec tag fw nic m mode
  for spec in B1:seabios:e1000:31 B2:seabios:e1000e:32 B3:seabios:virtio:33 B4:seabios:rtl8139:34 B5:seabios:vmxnet3:35 \
              U1:ovmf:e1000:41 U2:ovmf:e1000e:42 U3:ovmf:virtio:43 U4:ovmf:rtl8139:44 U5:ovmf:vmxnet3:45; do
    IFS=: read -r tag fw nic m <<< "${spec}"; mode=bios; [ "${fw}" = seabios ] || mode=uefi
    boot_to_installing "${tag}" "${mode}" "${fw}" "${nic}" "$(mac_of "${m}")" tap0
  done
  # B6：非 iPXE 的 BIOS PXE 客户端（构造报文）对已 arm 的 MAC 得到 undionly.kpxe
  local node; node="$(register_node acc-c-b6 "$(mac_of 36)" bios)"; api POST "/nodes/${node}/arm" '{}' > /dev/null
  sudo "${LAB_BIN}" pxe-client --iface "${BR}" --mac "$(mac_of 36)" --legacy > "${D}/b6.log" 2>&1 || true
  grep -q 'undionly.kpxe' "${D}/b6.log" && check "B6" 0 "BIOS 非 iPXE DISCOVER 得到 undionly.kpxe" || check "B6" 1 "$(tail -2 "${D}/b6.log")"
  api POST "/nodes/${node}/disarm" '{}' > /dev/null
  # U6：DHCP 规则改为 snponly.efi 后，OVMF virtio / e1000e 经 snponly 进入 d-i
  local rules; rules="$(ids dhcp_rules)"
  save_config dhcp_rules "${rules}" '{"uefi_firmware":"snp_only_efi","http_boot_enabled":true,"mirror_bootfile_options":false}' "accept U6" > /dev/null
  sleep 5
  boot_to_installing U6-virtio uefi ovmf virtio "$(mac_of 46)" tap0
  boot_to_installing U6-e1000e uefi ovmf e1000e "$(mac_of 47)" tap0
  tcpdump -nr "${D}/capture.pcap" -A 'udp port 69' 2>/dev/null | grep -q snponly.efi && check "U6 snponly" 0 "抓包可见 TFTP RRQ snponly.efi" || check "U6 snponly" 1 "未见 snponly.efi"
  save_config dhcp_rules "${rules}" '{"uefi_firmware":"ipxe_efi","http_boot_enabled":true,"mirror_bootfile_options":false}' "accept U6 恢复" > /dev/null
}

# ---------- D：UEFI HTTP Boot ----------
sc_D() {
  set_bootnet proxy_dhcp > /dev/null
  local spec nic m i=0
  for spec in virtio:51 e1000e:52; do
    IFS=: read -r nic m <<< "${spec}"
    install_start "D-${nic}" "acc-d-${nic}" uefi ovmf-http "${nic}" "$(mac_of "${m}")" "${TAPS[i]}"
    i=$((i + 1))
  done
  for nic in virtio e1000e; do install_wait "D-${nic}" && [ -n "${IP}" ] && check_packages "${IP}" "${nic}"; done
  api GET /bootnet/observations | jq -c '.[]' > "${D}/observations.jsonl"
  grep -q 'HTTPClient' "${D}/observations.jsonl" && check "HTTPClient" 0 "bootnet 观测到 HTTPClient 请求并应答 ipxe.efi URL" \
    || check "HTTPClient" 1 "观测中没有 HTTPClient"
}

# ---------- E：复用现有 DHCP ----------
sc_E() {
  set_bootnet external_dhcp || check "引导网络" 1 "bootnet 未进入 external_dhcp"
  api GET /bootnet/snippets > "${D}/snippets.json"
  jq -r '.snippets[] | select(.kind=="kea") | .text' "${D}/snippets.json" > "${D}/kea-snippet.txt"
  # 把产品给出的 client-classes 片段并入 existing.json，作为 lab-dhcp 的配置
  python3 - "${LAB_DIR}/kea/existing.json" "${D}/kea-snippet.txt" "${D}/kea-product.json" <<'PY'
import json, sys
conf = json.load(open(sys.argv[1]))
snippet = open(sys.argv[2]).read().strip().rstrip(',')
classes = json.loads("{" + snippet + "}")["client-classes"]
d4 = conf["Dhcp4"]
d4["client-classes"] = classes
for sub in d4.get("subnet4", []):
    sub.pop("boot-file-name", None); sub.pop("next-server", None)
    sub["option-data"] = [o for o in sub.get("option-data", []) if o.get("name") not in ("tftp-server-name", "boot-file-name")]
d4.pop("boot-file-name", None); d4.pop("next-server", None)
json.dump(conf, open(sys.argv[3], "w"), indent=2)
PY
  if [ -s "${D}/kea-product.json" ]; then
    cp "${D}/kea-product.json" "${LAB_DIR}/kea/product.json"
    "${LAB_DIR}/dhcp.sh" stop > /dev/null; "${LAB_DIR}/dhcp.sh" start product > /dev/null
    check "Kea 片段" 0 "lab-dhcp 使用产品片段（$(jq '.Dhcp4["client-classes"] | length' "${D}/kea-product.json") 个 client-class）"
  else check "Kea 片段" 1 "无法从 /bootnet/snippets 生成配置"; fi
  local spec fw nic m mode i=0
  for spec in seabios:virtio:61 ovmf:e1000e:62; do
    IFS=: read -r fw nic m <<< "${spec}"; mode=bios; [ "${fw}" = seabios ] || mode=uefi
    install_start "E-${fw}" "acc-e-${fw}" "${mode}" "${fw}" "${nic}" "$(mac_of "${m}")" "${TAPS[i]}"
    i=$((i + 1))
  done
  install_wait E-seabios; install_wait E-ovmf
  local from_us; from_us="$(tcpdump -nr "${D}/capture.pcap" "src host ${LAB_IP} and (udp src port 67 or udp src port 4011)" 2>/dev/null | wc -l)"
  [ "${from_us}" = 0 ] && check "bootnet 不发 DHCP" 0 "抓包中 ${LAB_IP} 发出的 67/4011 报文 0 个" || check "bootnet 不发 DHCP" 1 "${from_us} 个"
  "${LAB_DIR}/dhcp.sh" stop > /dev/null; "${LAB_DIR}/dhcp.sh" start coexist > /dev/null
  set_bootnet proxy_dhcp > /dev/null
}

# ---------- F / G：Redfish ----------
rf_setup() { # <mac> → 全局 BMC
  source "${LAB_DIR}/lib/redfish.sh"
  rf_prepare "$1"
  local pw; pw="$(cat "${WORK}/sushy/password")"
  BMC_NAME="lab-sushy-$(date +%s)"
  BMC="$(api POST /bmcs "$(jq -n --arg n "${BMC_NAME}" --arg u "https://${LAB_IP}:8000" --arg p "${pw}" --arg s "${RF_UUID}" \
    '{name:$n,endpoint:$u,username:"admin",password:$p,system_id:$s}')" | jq -r .version.object_id)"
}
sc_F() {
  set_bootnet redfish_only > /dev/null || true
  rf_setup "$(mac_of 71)"
  local probe fp
  probe="$(api POST "/bmcs/${BMC}/probe" '{}')"; fp="$(jq -r '.fingerprint // .probe.fingerprint // empty' <<< "${probe}")"
  [ -n "${fp}" ] && check "探测" 0 "指纹 ${fp:0:16}…" || check "探测" 1 "${probe}"
  api POST "/bmcs/${BMC}/trust" "$(jq -n --arg f "${fp}" '{fingerprint:$f}')" > /dev/null && check "确认指纹" 0 "已信任" || check "确认指纹" 1 "trust 失败"
  local ch; ch="$(jq -n --arg b "${BMC_NAME}" '{type:"redfish",bmc:$b}')"
  local bad; bad="$(api POST /nodes "$(jq -n --argjson ch "${ch}" --arg m "$(mac_of 72)" --argjson p "$(ids profile)" --argjson q "$(ids plan)" \
    '{spec:{hostname:"acc-f-bios",fqdn:null,mac:$m,os_family:"debian13",firmware:"bios",channel:$ch,network:{type:"dhcp"}},profile_id:$p,init_plan_id:$q}')" || true)"
  grep -q redfish_requires_uefi <<< "${bad}" && check "BIOS+Redfish 拒绝" 0 "redfish_requires_uefi" || check "BIOS+Redfish 拒绝" 1 "${bad:0:200}"
  NODE="$(register_node acc-f-uefi "$(mac_of 71)" uefi '{"type":"dhcp"}' "${ch}")"
  api POST "/nodes/${NODE}/arm" '{}' > /dev/null
  api POST "/nodes/${NODE}/redfish/deploy" '{}' > "${D}/deploy.json" && check "部署" 0 "$(jq -c . "${D}/deploy.json" | cut -c1-160)" || check "部署" 1 "$(cat "${D}/deploy.json")"
  if wait_state "${NODE}" "${INSTALL_TIMEOUT}" ready; then check "Ready" 0 "UEFI Redfish 节点装完"
  else check "Ready" 1 "最后状态 $(node_state "${NODE}")"; fi
  has_event "${NODE}" effect.eject_media && check "弹出介质" 0 "installer.started 后执行 EjectMedia" || check "弹出介质" 1 "未见 effect.eject_media"
  # 更换 sushy 证书后部署被拒并告警
  local S="${WORK}/sushy"
  cp "${S}/cert.pem" "${D}/cert.orig"; cp "${S}/key.pem" "${D}/key.orig"
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 30 -subj "/CN=${LAB_IP}/O=rotated" \
    -addext "subjectAltName=IP:${LAB_IP}" -keyout "${S}/key.pem" -out "${S}/cert.pem" 2>/dev/null
  rf_start_sushy
  api POST "/nodes/${NODE}/reset" '{}' > /dev/null 2>&1 || true
  api POST "/nodes/${NODE}/arm" '{}' > /dev/null 2>&1 || true
  local again; again="$(api POST "/nodes/${NODE}/redfish/deploy" '{}' || true)"
  grep -qi 'cert\|fingerprint\|指纹' <<< "${again}" && check "证书变化拒绝" 0 "$(jq -r '.message // .error.message // .' <<< "${again}" | cut -c1-120)" \
    || check "证书变化拒绝" 1 "${again:0:200}"
  cp "${D}/cert.orig" "${S}/cert.pem"; cp "${D}/key.orig" "${S}/key.pem"
  rf_cleanup
}
sc_G() {
  set_bootnet redfish_only > /dev/null || true
  "${LAB_DIR}/dhcp.sh" stop > /dev/null
  rf_setup "$(mac_of 81)"
  local fp; fp="$(api POST "/bmcs/${BMC}/probe" '{}' | jq -r '.fingerprint // .probe.fingerprint // empty')"
  api POST "/bmcs/${BMC}/trust" "$(jq -n --arg f "${fp}" '{fingerprint:$f}')" > /dev/null
  local ch net
  ch="$(jq -n --arg b "${BMC_NAME}" '{type:"redfish",bmc:$b}')"
  net='{"type":"static","address":"10.77.0.81/24","gateway":"10.77.0.1","dns":["10.77.0.1"]}'
  NODE="$(register_node acc-g-static "$(mac_of 81)" uefi "${net}" "${ch}")"
  api POST "/nodes/${NODE}/arm" '{}' > /dev/null
  api POST "/nodes/${NODE}/redfish/deploy" '{}' > "${D}/deploy.json" || check "部署" 1 "$(cat "${D}/deploy.json")"
  wait_state "${NODE}" "${INSTALL_TIMEOUT}" ready && check "Ready" 0 "静态地址节点装完" || check "Ready" 1 "$(node_state "${NODE}")"
  local dhcp; dhcp="$(tcpdump -nr "${D}/capture.pcap" "ether src $(mac_of 81) and udp port 67" 2>/dev/null | wc -l)"
  [ "${dhcp}" = 0 ] && check "无 DHCP" 0 "iPXE 与 d-i 均未发 DHCP" || check "无 DHCP" 1 "${dhcp} 个 DHCP 报文"
  local addr; addr="$(tssh 10.77.0.81 'ip -4 -o addr show scope global' 2>/dev/null | awk '{print $4}')"
  [ "${addr}" = 10.77.0.81/24 ] && check "ip addr" 0 "装完后 ip addr 为 10.77.0.81/24" || check "ip addr" 1 "得到 ${addr:-无法登录}"
  rf_cleanup
  "${LAB_DIR}/dhcp.sh" start coexist > /dev/null
}

# ---------- H：装完后网络优先重启 3 次不重装 ----------
sc_H() {
  set_bootnet proxy_dhcp > /dev/null
  local f line n=0
  for f in "${ACC}/installed-A" "${ACC}/installed-B"; do
    [ -f "$f" ] || continue
    while read -r node vm ip mac; do
      [ -S "${vm}/mon.sock" ] || continue
      local k
      for k in 1 2 3; do
        local before; before="$(api GET /bootnet/observations | jq --arg m "${mac}" '[.[] | select(.mac==$m)] | length')"
        vm_reboot "${vm}"; sleep 30
        ip="$(wait_ssh "${mac}" 900 || true)"
        local seen; seen="$(api GET /bootnet/observations | jq -r --arg m "${mac}" '[.[] | select(.mac==$m) | .detail] | .[]' | grep -c 'not_armed\|NotArmed' || true)"
        [ -n "${ip}" ] && tssh "${ip}" 'test -f /root/xpxe-accept-marker' && [ "$(node_state "${node}")" = ready ] \
          && check "重启${k}(${mac})" 0 "本地盘启动，标记文件仍在，状态 ready，NotArmed 观测 ${seen} 条（重启前 ${before}）" \
          || check "重启${k}(${mac})" 1 "ip=${ip:-无} 状态 $(node_state "${node}")"
      done
      n=$((n + 1))
    done < "$f"
  done
  [ "${n}" -gt 0 ] || check "前置" 1 "没有 A/B 装好的虚拟机（先运行 A、B，并保留虚拟机）"
}

# ---------- I：选盘 ----------
sc_I() {
  set_bootnet proxy_dhcp > /dev/null
  install_node I-std acc-i-std uefi ovmf virtio "$(mac_of 91)" tap0 --disks 20G,60G
  if [ -n "${VMDIR}" ]; then
    local usb data; usb="$(du -k "${VMDIR}/vusb.raw" | cut -f1)"; data="$(du -k "${VMDIR}/data.qcow2" | cut -f1)"
    [ "${usb}" = 0 ] && [ "${data}" -lt 1024 ] && check "第一轮" 0 "装到 SYS0001；USB 盘未写入（${usb} KiB）、数据盘未分区（${data} KiB）" \
      || check "第一轮" 1 "usb=${usb}KiB data=${data}KiB"
    vm_stop "${VMDIR}"
  fi
  # 第二轮：两块 20G 盘 + Unique → Failed，事件带三块盘清单与排除原因
  local pid; pid="$(create_config profile lab-unique "$(profile_doc lab-unique '.disk.prefer="unique"')")"
  local node; node="$(register_node acc-i-unique "$(mac_of 92)" uefi '{"type":"dhcp"}' '{"type":"lan"}' "${pid}")"
  api POST "/nodes/${node}/arm" '{}' > /dev/null
  local vm; vm="$(vm_start I-unique ovmf virtio "$(mac_of 92)" tap1 --disks 20G,20G)"
  wait_state "${node}" "${BOOT_TIMEOUT}" failed install_failed > /dev/null || true
  local ev; ev="$(timeline "${node}" | jq -c 'select(.kind=="installer_failed") | .payload' | tail -1)"
  [ "$(node_state "${node}")" = failed ] && grep -q disk_selection <<< "${ev}" \
    && check "第二轮" 0 "进入 Failed（disk_selection）：$(jq -r .inventory <<< "${ev}" | tr '\n' ' ' | cut -c1-200)" \
    || check "第二轮" 1 "状态 $(node_state "${node}") 事件 ${ev:-无}"
  vm_stop "${vm}"
}

# ---------- J：超时 ----------
set_timeouts() { # <arm> <install> <firstboot> <configure>
  local doc; doc="$(jq -n --argjson a "$1" --argjson i "$2" --argjson f "$3" --argjson c "$4" '{arm_secs:$a,install_secs:$i,firstboot_secs:$f,configure_secs:$c}')"
  local id; id="$(api GET /configs/timeouts | jq -r '.[0].id // .items[0].id // empty' 2>/dev/null)"
  if [ -z "${id}" ]; then create_config timeouts default "${doc}" > /dev/null; else save_config timeouts "${id}" "${doc}" "accept J" > /dev/null; fi
}
sc_J() {
  set_bootnet proxy_dhcp > /dev/null
  set_timeouts 60 300 1800 3600 || check "设置超时" 1 "保存 timeouts 失败"
  local n1; n1="$(register_node acc-j-arm "$(mac_of a1)" uefi)"; api POST "/nodes/${n1}/arm" '{}' > /dev/null
  sleep 90
  local s1; s1="$(node_state "${n1}")"
  timeline "${n1}" | grep -q 'DeadlineExpired\|deadline' && check "arm_ttl" 0 "arm 后不开机 → ${s1}，事件含 DeadlineExpired" || check "arm_ttl" 1 "状态 ${s1}"
  local n2; n2="$(register_node acc-j-install "$(mac_of a2)" uefi)"; api POST "/nodes/${n2}/arm" '{}' > /dev/null
  local vm; vm="$(vm_start J-install ovmf virtio "$(mac_of a2)" tap3)"
  if wait_state "${n2}" "${BOOT_TIMEOUT}" installing; then sudo ip link set tap3 down; fi
  sleep 360
  local s2; s2="$(node_state "${n2}")"
  timeline "${n2}" | grep -q 'DeadlineExpired\|deadline' && check "install_ttl" 0 "安装中断开 tap → ${s2}，事件含 DeadlineExpired" || check "install_ttl" 1 "状态 ${s2}"
  sudo ip link set tap3 up; vm_stop "${vm}"
  set_timeouts 7200 5400 1800 3600 > /dev/null
}

# ---------- K：重启与失联 ----------
sc_K() {
  set_bootnet proxy_dhcp > /dev/null
  local node; node="$(register_node acc-k "$(mac_of b1)" uefi)"; api POST "/nodes/${node}/arm" '{}' > /dev/null
  local vm; vm="$(vm_start K ovmf virtio "$(mac_of b1)" tap0)"
  wait_state "${node}" "${BOOT_TIMEOUT}" installing > /dev/null || true
  local before; before="$(api GET "/nodes/${node}" | jq -c '{state,deadline_at}')"
  "${COMPOSE[@]}" restart control >> "${ACC}/compose.log" 2>&1
  wait_http "${API%/api/v1}/" 60
  local after; after="$(api GET "/nodes/${node}" | jq -c '{state,deadline_at}')"
  [ "${before}" = "${after}" ] && check "重启保持" 0 "重启 control 前后 ${after}" || check "重启保持" 1 "${before} → ${after}"
  wait_state "${node}" "${INSTALL_TIMEOUT}" ready && check "继续完成" 0 "重启后装完" || check "继续完成" 1 "$(node_state "${node}")"
  vm_stop "${vm}"
  # 停止 control > 30 s：bootnet fail closed（停止应答），恢复后继续
  local n2; n2="$(register_node acc-k2 "$(mac_of b2)" uefi)"; api POST "/nodes/${n2}/arm" '{}' > /dev/null
  "${COMPOSE[@]}" stop control >> "${ACC}/compose.log" 2>&1
  sleep 45
  sudo "${LAB_BIN}" pxe-client --iface "${BR}" --mac "$(mac_of b2)" --uefi > "${D}/failclosed.log" 2>&1 || true
  grep -qi 'ipxe.efi\|filename' "${D}/failclosed.log" && check "fail closed" 1 "控制面停止 45 s 后仍收到应答" || check "fail closed" 0 "控制面停止 45 s 后 bootnet 不应答"
  "${COMPOSE[@]}" start control >> "${ACC}/compose.log" 2>&1
  wait_http "${API%/api/v1}/" 60; sleep 20
  sudo "${LAB_BIN}" pxe-client --iface "${BR}" --mac "$(mac_of b2)" --uefi > "${D}/recovered.log" 2>&1 || true
  grep -qi 'ipxe.efi' "${D}/recovered.log" && check "恢复" 0 "控制面恢复后 bootnet 重新应答" || check "恢复" 1 "$(tail -2 "${D}/recovered.log")"
  api POST "/nodes/${n2}/disarm" '{}' > /dev/null
}

# ---------- L：伪造 ProxyDHCP ----------
sc_L() {
  set_bootnet redfish_only > /dev/null || true
  "${LAB_DIR}/dhcp.sh" start rogue > /dev/null
  api POST /bootnet/scan '{}' > "${D}/scan.json" || true
  sleep 10
  local conflicts; conflicts="$(api GET /bootnet/status | jq -c '.heartbeat.heartbeat.probe.conflicts // []')"
  grep -q 10.77.0.3 <<< "${conflicts}${scan:-}$(cat "${D}/scan.json")" && check "冲突告警" 0 "检测到 10.77.0.3 下发 next-server/filename" || check "冲突告警" 1 "${conflicts}"
  local id; id="$(ids boot_network)" head; head="$(api GET "/configs/boot_network/${id}" | jq .version.version)"
  local refused; refused="$(api PUT "/configs/boot_network/${id}" "$(jq -n --arg i "${BR}" --argjson h "${head}" '{document:{mode:"proxy_dhcp",iface:$i},expected_head:$h,comment:"accept L"}')" || true)"
  grep -qi 'conflict\|冲突' <<< "${refused}" && check "未确认拒绝切换" 0 "$(jq -r '.message // .error.message // .' <<< "${refused}" | cut -c1-120)" \
    || check "未确认拒绝切换" 1 "${refused:0:200}"
  api POST /bootnet/acknowledge '{"servers":["10.77.0.3"]}' > /dev/null && check "确认" 0 "acknowledge 10.77.0.3" || check "确认" 1 "acknowledge 失败"
  api GET "/audit?action=bootnet.acknowledge" | grep -q acknowledge && check "审计" 0 "确认动作写入审计" || check "审计" 1 "审计中未见 acknowledge"
  "${LAB_DIR}/dhcp.sh" stop rogue > /dev/null
  set_bootnet proxy_dhcp > /dev/null || true
}

# ---------- M：离线 ----------
sc_M() {
  local CHAIN=(DOCKER-USER -s 172.30.77.0/24 ! -d 10.77.0.0/24 -m conntrack --ctstate NEW -j REJECT)
  deploy_down; deploy_up true
  sudo iptables -I "${CHAIN[@]}" 2>/dev/null || sudo iptables -I DOCKER-USER -s 172.30.77.0/24 ! -d 10.77.0.0/24 -m conntrack --ctstate NEW -j REJECT
  sudo iptables -Z DOCKER-USER
  local fetch; fetch="$(api POST /assets/fetch '{"kind":"ipxe_efi_x64"}' || true)"
  grep -qi 'offline\|离线' <<< "${fetch}" && check "官方获取禁用" 0 "$(jq -r '.message // .error.message // .' <<< "${fetch}" | cut -c1-100)" || check "官方获取禁用" 1 "${fetch:0:200}"
  local id; id="$(upload_ipxe ipxe_efi_x64 /usr/lib/ipxe/ipxe.efi)"
  activate ipxe_efi_x64 "${id}" "离线验收" && check "上传可激活" 0 "离线模式上传 ipxe.efi 并激活（资产 ${id}）" || check "上传可激活" 1 "激活失败"
  install_node M-uefi acc-m-uefi uefi ovmf e1000e "$(mac_of c1)" tap0 && [ -n "${IP}" ] && check_packages "${IP}" m
  local rejects; rejects="$(sudo iptables -L DOCKER-USER -v -n -x | awk '/172.30.77.0\/24/ && /REJECT/ {print $1}' | head -1)"
  [ "${rejects:-x}" = 0 ] && check "零出站" 0 "DOCKER-USER REJECT 计数 0" || check "零出站" 1 "REJECT 计数 ${rejects:-规则缺失}"
  sudo iptables -D DOCKER-USER -s 172.30.77.0/24 ! -d 10.77.0.0/24 -m conntrack --ctstate NEW -j REJECT 2>/dev/null || true
  deploy_down; deploy_up false
}

# ---------- N：ABI 不一致 ----------
sc_N() {
  # 旧 netboot（6.12.38）与镜像源当前 kernel-image-*-di ABI 不一致：激活后 arm 必须被拒绝
  local old; old="$(api POST /assets/fetch '{"kind":"debian_netboot_amd64","mirror":"http://snapshot.debian.org/archive/debian/20250815T000000Z"}' | jq -r '.id // empty' || true)"
  if [ -z "${old}" ]; then check "旧 netboot" 1 "无法获取旧版 netboot（snapshot.debian.org）"; return; fi
  activate debian_netboot_amd64 "${old}" "ABI 不一致验收"
  local node; node="$(register_node acc-n "$(mac_of d1)" uefi)"
  local r; r="$(api POST "/nodes/${node}/arm" '{}' || true)"
  grep -qi 'ABI\|ALIGN' <<< "${r}" && grep -q "${LAB_IP}:3142\|mirror\|镜像" <<< "${r}" \
    && check "arm 拒绝" 0 "$(jq -c '[.fields[]? | {path,code}]' <<< "${r}")" || check "arm 拒绝" 1 "${r:0:300}"
  activate debian_netboot_amd64 "$(ids netboot)" "恢复对齐的 netboot"
}

# ---------- O：iPXE 回滚 ----------
sc_O() {
  set_bootnet proxy_dhcp > /dev/null
  local good bad path_before path_after
  good="$(api GET /assets | jq -r '.channels.channels.ipxe_efi_x64.sha256')"
  # “可通过格式检查但无法引导”：snponly.efi 作为 ipxe.efi 上传（同为合法 EFI 映像），在 e1000（无 SNP 驱动以外的原生驱动）上无法联网
  bad="$(upload_ipxe ipxe_efi_x64 "${REPO_DIR}/crates/xpxe-assets/tests/fixtures/ipxe/snponly.efi")"
  activate ipxe_efi_x64 "${bad}" "回滚验收：模拟坏版本"
  local n1; n1="$(register_node acc-o-canary "$(mac_of e1)" uefi)"; api POST "/nodes/${n1}/arm" '{}' > /dev/null
  path_before="$(api GET "/nodes/${n1}" | jq -r '.active_session_id')"
  sudo "${LAB_BIN}" pxe-client --iface "${BR}" --mac "$(mac_of e1)" --uefi > "${D}/bad.log" 2>&1 || true
  api POST /assets/rollback '{"kind":"ipxe_efi_x64"}' > "${D}/rollback.json" && check "一键回滚" 0 "$(jq -c . "${D}/rollback.json" | cut -c1-120)" || check "一键回滚" 1 "$(cat "${D}/rollback.json")"
  local now; now="$(api GET /assets | jq -r '.channels.channels.ipxe_efi_x64.sha256')"
  [ "${now}" = "${good}" ] && check "回滚目标" 0 "当前激活 ${now:0:12}" || check "回滚目标" 1 "当前 ${now:0:12}，期望 ${good:0:12}"
  local n2; n2="$(register_node acc-o-after "$(mac_of e2)" uefi)"; api POST "/nodes/${n2}/arm" '{}' > /dev/null
  sudo "${LAB_BIN}" pxe-client --iface "${BR}" --mac "$(mac_of e2)" --uefi > "${D}/good.log" 2>&1 || true
  grep -q "fw/${good:0:12}/" "${D}/good.log" && check "fw 路径" 0 "回滚后新 arm 节点得到 fw/${good:0:12}/ipxe.efi" || check "fw 路径" 1 "$(grep -o 'fw/[^ ]*' "${D}/good.log" "${D}/bad.log" | head -2 | tr '\n' ' ')"
  [ "$(node_state "${n1}")" = armed ] && check "进行中会话" 0 "回滚前 arm 的节点会话不受影响（仍 armed）" || check "进行中会话" 1 "$(node_state "${n1}")"
  api POST "/nodes/${n1}/disarm" '{}' > /dev/null; api POST "/nodes/${n2}/disarm" '{}' > /dev/null
}

# ---------- P：安全 ----------
sc_P() {
  local open; open="$(sudo ss -Hltnu | awk '{print $1, $5}' | grep -E ':(8088|8080|8081|67|69|4011)$' | sort -u | tr '\n' ';')"
  echo "${open}" > "${D}/ports.txt"
  grep -q '127.0.0.1:8088' <<< "${open}" && grep -q '127.0.0.1:8081' <<< "${open}" && ! grep -qE '0.0.0.0:808[18]|\*:808[18]' <<< "${open}" \
    && check "端口" 0 "${open}" || check "端口" 1 "${open}"
  local c
  for c in control bootnet; do
    docker inspect "$(cid "${c}")" | jq -e '.[0].HostConfig | .ReadonlyRootfs == true and (.CapDrop | index("ALL")) and (.SecurityOpt | index("no-new-privileges:true"))' > /dev/null \
      && check "容器 ${c}" 0 "只读根、cap_drop ALL、no-new-privileges" || check "容器 ${c}" 1 "$(docker inspect "$(cid "${c}")" | jq -c '.[0].HostConfig | {ReadonlyRootfs,CapDrop,SecurityOpt}')"
  done
  local capeff; capeff="$(grep CapEff "/proc/$(docker inspect -f '{{.State.Pid}}' "$(cid bootnet)")/status" | awk '{print $2}')"
  [ "${capeff}" = 0000000000000400 ] && check "bootnet CapEff" 0 "0x400" || check "bootnet CapEff" 1 "${capeff}"
  docker run --rm --entrypoint /bin/sh "x-pxe:${XPXE_VERSION}" -c true > /dev/null 2>&1 && check "无 shell" 1 "镜像里有 /bin/sh" || check "无 shell" 0 "docker run --entrypoint /bin/sh 失败"
  local vol; vol="$(docker volume inspect -f '{{.Mountpoint}}' x-pxe-lab_xpxe-data)"
  local mk; mk="$(sudo base64 -w0 "${DEPLOY}/secrets/master_key")"
  if sudo grep -rqaF "${mk}" "${vol}" || sudo grep -rqaF -f <(sudo cat "${DEPLOY}/secrets/master_key") "${vol}" 2>/dev/null; then check "数据卷无主密钥" 1 "数据卷中检索到主密钥"
  else check "数据卷无主密钥" 0 "主密钥（原文与 base64）不在数据卷中"; fi
  if [ -f "${WORK}/sushy/password" ] && sudo grep -rqaF "$(cat "${WORK}/sushy/password")" "${vol}"; then check "数据卷无 BMC 明文" 1 "检索到 BMC 密码"
  else check "数据卷无 BMC 明文" 0 "BMC 密码不以明文出现在数据卷中"; fi
  local code; code="$(curl -s -o /dev/null -w '%{http_code}' "${API}/nodes")"
  [ "${code}" = 401 ] && check "未认证 401" 0 "GET /api/v1/nodes → 401" || check "未认证 401" 1 "${code}"
  local cookie; cookie="$(curl -s -D - -o /dev/null -H 'Content-Type: application/json' --data "$(jq -n --arg p "$(cat "${ACC}/admin-password")" '{username:"admin",password:$p}')" "${API}/auth/login" | grep -i '^set-cookie: xpxe_session' | tr -d '\r')"
  grep -qi httponly <<< "${cookie}" && grep -qi 'samesite=strict' <<< "${cookie}" && ! grep -qi secure <<< "${cookie}" \
    && check "Cookie(HTTP)" 0 "HttpOnly; SameSite=Strict" || check "Cookie(HTTP)" 1 "$(sed 's/=[^;]*;/=…;/' <<< "${cookie}")"
  # 挂入测试证书：同一端口 HTTPS，Cookie 带 Secure
  local T="${ACC}/tls"; mkdir -p "${T}"
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 7 -subj /CN=127.0.0.1 -addext "subjectAltName=IP:127.0.0.1" \
    -keyout "${T}/key.pem" -out "${T}/cert.pem" 2>/dev/null
  sudo chown 10001:10001 "${T}"/*.pem; sudo chmod 0400 "${T}"/*.pem
  cat > "${ACC}/compose.tls.yaml" <<YAML
services:
  control:
    environment:
      XPXE_TLS_CERT_FILE: /run/secrets/xpxe_tls_cert
      XPXE_TLS_KEY_FILE: /run/secrets/xpxe_tls_key
    secrets: [xpxe_master_key, xpxe_admin_initial_password, xpxe_bootnet_token, xpxe_tls_cert, xpxe_tls_key]
secrets:
  xpxe_tls_cert: { file: ${T}/cert.pem }
  xpxe_tls_key: { file: ${T}/key.pem }
YAML
  "${COMPOSE[@]}" -f "${ACC}/compose.tls.yaml" up -d --wait control >> "${ACC}/compose.log" 2>&1
  local tls; tls="$(curl -sk -D - -o /dev/null -H 'Content-Type: application/json' --data "$(jq -n --arg p "$(cat "${ACC}/admin-password")" '{username:"admin",password:$p}')" "https://127.0.0.1:8088/api/v1/auth/login" | grep -i '^set-cookie: xpxe_session' | tr -d '\r')"
  grep -qi secure <<< "${tls}" && check "HTTPS" 0 "同一端口 8088 为 HTTPS，Cookie 带 Secure" || check "HTTPS" 1 "${tls:-无应答}"
  "${COMPOSE[@]}" up -d --wait control >> "${ACC}/compose.log" 2>&1
  # 一次性 preseed：arm 后取两次，第二次 410
  local node tok; node="$(register_node acc-p "$(mac_of f1)" uefi)"; api POST "/nodes/${node}/arm" '{}' > /dev/null
  tok="$(curl -s "${BASE}/boot/mac/$(mac_of f1 | tr ':' '-')" | sed -nE 's#.*(/p/[^/ ]+)/.*#\1#p' | head -1)"
  local c1 c2
  c1="$(curl -s -o /dev/null -w '%{http_code}' "${BASE}${tok}/preseed.cfg")"; c2="$(curl -s -o /dev/null -w '%{http_code}' "${BASE}${tok}/preseed.cfg")"
  [ "${c1}" = 200 ] && [ "${c2}" = 410 ] && check "preseed 一次性" 0 "第一次 200，第二次 410" || check "preseed 一次性" 1 "${c1} / ${c2}（会话路径 ${tok:-无}）"
  api POST "/nodes/${node}/reset" '{}' > /dev/null 2>&1 || api POST "/nodes/${node}/disarm" '{}' > /dev/null 2>&1 || true
  ctl audit verify > "${D}/audit-verify.txt" 2>&1 && check "audit verify" 0 "$(cat "${D}/audit-verify.txt")" || check "audit verify" 1 "$(cat "${D}/audit-verify.txt")"
}

# ---------- Q：WebUI ----------
sc_Q() {
  ( cd "${REPO_DIR}" && scripts/web-gate.sh ) > "${D}/web-gate.log" 2>&1 \
    && check "web-gate" 0 "$(grep -E 'Tests +[0-9]+ passed|[0-9]+ passed' "${D}/web-gate.log" | tr '\n' ' ')" \
    || check "web-gate" 1 "$(grep -E 'failed|✘' "${D}/web-gate.log" | head -5 | tr '\n' ' ')"
  ( cd "${REPO_DIR}/web" && npx playwright test e2e/integration.spec.ts ) > "${D}/integration.log" 2>&1 \
    && check "联调 8 步" 0 "$(grep -E 'passed' "${D}/integration.log" | tail -1)" || check "联调 8 步" 1 "$(tail -5 "${D}/integration.log" | tr '\n' ' ')"
  # 对被测镜像：容器内 /web 与本次构建产物一致，且页面可登录（API 冒烟）
  local idx; idx="$(curl -s "${API%/api/v1}/" | grep -o 'assets/main-[^"]*\.js' | head -1)"
  [ -n "${idx}" ] && curl -s -o /dev/null -w '%{http_code}' "${API%/api/v1}/${idx}" | grep -qx 200 \
    && check "被测镜像 WebUI" 0 "管理端提供 /${idx}，登录与 API 由 A–P 覆盖" || check "被测镜像 WebUI" 1 "首页或主包不可取"
}

# ---------- R：门禁 ----------
sc_R() {
  ( cd "${REPO_DIR}" && scripts/gate.sh ) > "${D}/gate.log" 2>&1 && check "gate.sh" 0 "$(tail -1 "${D}/gate.log")" || check "gate.sh" 1 "$(tail -5 "${D}/gate.log" | tr '\n' ' ')"
  if [ -s "${OUT}/accept/Q/web-gate.log" ] && grep -q 'PASS web-gate' "${OUT}/accept/Q/result" 2>/dev/null; then check "web-gate.sh" 0 "见场景 Q"
  else ( cd "${REPO_DIR}" && scripts/web-gate.sh ) > "${D}/web-gate.log" 2>&1 && check "web-gate.sh" 0 "通过" || check "web-gate.sh" 1 "$(tail -5 "${D}/web-gate.log" | tr '\n' ' ')"; fi
  if command -v cargo-deny > /dev/null 2>&1 || cargo deny --version > /dev/null 2>&1; then
    ( cd "${REPO_DIR}" && cargo deny check ) > "${D}/deny.log" 2>&1 && check "cargo deny" 0 "无违规" || check "cargo deny" 1 "$(grep -E '^error' "${D}/deny.log" | head -5 | tr '\n' ' ')"
  else check "cargo deny" 1 "未安装 cargo-deny"; fi
  local size; size="$(docker image inspect -f '{{.Size}}' "x-pxe:${XPXE_VERSION}")"
  check "镜像大小" 0 "x-pxe:${XPXE_VERSION} $((size / 1024 / 1024)) MiB（$(docker image inspect -f '{{.Id}}' "x-pxe:${XPXE_VERSION}" | cut -c1-19)）"
}
