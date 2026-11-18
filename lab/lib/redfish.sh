#!/usr/bin/env bash
# R1/R2：sushy-tools（libvirt 驱动）+ libvirt OVMF 域。由 lab/run.sh source。
# sushy-tools 2.2.0 把 VirtualMedia 挂在 Systems 下（Managers 资源里的链接也指向同一路径）。
RF_VM=/var/lib/libvirt/images/xpxe-lab/vm-redfish
RF_URL="https://${LAB_IP}:8000"
rf() { # <方法> <路径> [JSON]
  local pw; pw="$(cat "${WORK}/sushy/password")"
  curl -sS -k --fail-with-body -u "admin:${pw}" -X "$1" -H 'Content-Type: application/json' \
    ${3:+--data "$3"} "${RF_URL}$2"
}

rf_prepare() { # <mac>
  local mac="$1" uuid
  sudo virsh -q -c qemu:///system destroy vm-redfish >/dev/null 2>&1 || true
  sudo virsh -q -c qemu:///system undefine --nvram vm-redfish >/dev/null 2>&1 || true
  sudo virsh -c qemu:///system pool-info default >/dev/null 2>&1 || sudo virsh -c qemu:///system pool-define-as default dir --target /var/lib/libvirt/images >/dev/null
  sudo virsh -c qemu:///system pool-start default >/dev/null 2>&1 || true
  sudo rm -rf "${RF_VM}"; sudo mkdir -p "${RF_VM}"
  sudo qemu-img create -q -f qcow2 "${RF_VM}/sys.qcow2" 20G
  uuid="$(cat /proc/sys/kernel/random/uuid)"
  sed -e "s#@UUID@#${uuid}#" -e "s#@VM@#${RF_VM}#g" -e "s#@MAC@#${mac}#" \
    "${LAB_DIR}/libvirt/vm-redfish.xml.in" > "${WORK}/sushy/vm-redfish.xml"
  sudo virsh -q -c qemu:///system define "${WORK}/sushy/vm-redfish.xml" >/dev/null
  sed -e "s#@UUID@#${uuid}#" -e "s#@WORK@#${WORK}#g" "${LAB_DIR}/sushy/emulator.conf.in" > "${WORK}/sushy/emulator.conf"
  rf_start_sushy
  RF_UUID="${uuid}"
}

rf_start_sushy() {
  svc_stop sushy
  svc_start sushy root /opt/xpxe-lab/sushy/bin/sushy-emulator --config "${WORK}/sushy/emulator.conf"
  local i; for i in $(seq 1 30); do rf GET /redfish/v1/ >/dev/null 2>&1 && break; sleep 1; done
}

# xpxe-redfish 客户端（xpxe-lab redfish）：密码只从文件读
rf_client() { # <probe|deploy|eject> [参数…]
  local action="$1"; shift
  if [ "${action}" = probe ]; then "${LAB_BIN}" redfish probe --url "${RF_URL}" "$@"
  else "${LAB_BIN}" redfish "${action}" --url "${RF_URL}" --user admin --password-file "${WORK}/sushy/password" "$@"; fi
}

# 访问日志（从第 <行> 行起）中依次出现给定路径子串
http_in_order() { # <起始行> <子串…>
  local from="$1"; shift
  tail -n "+${from}" "$(svc_log serve)" | jq -R -r 'fromjson? | select(.event=="http" and .method=="GET") | .path' \
    | awk -v want="$*" 'BEGIN { n = split(want, w, " "); i = 1 } i <= n && index($0, w[i]) { i++ } END { exit (i > n ? 0 : 1) }'
}

rf_cleanup() {
  sudo cp "${RF_VM}/serial.log" "${D}/serial-redfish.log" 2>/dev/null && sudo chown "$(id -un)" "${D}/serial-redfish.log"
  sudo virsh -q -c qemu:///system destroy vm-redfish >/dev/null 2>&1 || true
  sudo virsh -q -c qemu:///system undefine --nvram vm-redfish >/dev/null 2>&1 || true
  svc_stop sushy
  [ "${KEEP}" = 1 ] || sudo rm -rf "${RF_VM}"
}

sc_R() { # <R1|R2> <tap>：会话 ISO（xpxe-image）+ xpxe-redfish 部署 + sushy-tools/OVMF
  local id="$1" mid fixture token
  if [ "${id}" = R1 ]; then mid=a1 fixture=redfish-uefi.json token=lab-redfish-r1
  else mid=a2 fixture=redfish-uefi-static.json token=lab-redfish-r2; fi
  local mac dir iso ip=10.77.0.60
  mac="$(mac_of "${mid}")"; dir="${WORK}/www/p/${token}"; iso="${dir}/boot.iso"
  mkdir -p "${dir}"
  if [ "${id}" = R1 ]; then cp "${WORK}/www/boot.ipxe" "${dir}/boot.ipxe"
  else
    cat > "${dir}/boot.ipxe" <<NODE
#!ipxe
echo x-pxe-lab R2 session boot.ipxe static ${ip}
kernel ${BASE}/d-i/linux initrd=initrd.gz auto=true priority=critical url=${BASE}/preseed.cfg?mac=\${netX/mac:hexhyp} netcfg/disable_autoconfig=true netcfg/get_ipaddress=${ip} netcfg/get_netmask=255.255.255.0 netcfg/get_gateway=${LAB_IP} netcfg/get_nameservers=${LAB_IP} netcfg/confirm_static=true console=ttyS0,115200n8 ---
initrd ${BASE}/d-i/initrd.gz
boot
NODE
  fi
  if [ -n "${ISO_OVERRIDE}" ] && [ "${id}" = R1 ]; then cp "${ISO_OVERRIDE}" "${iso}"
  else "${LAB_BIN}" iso --session-fixture "${LAB_DIR}/sessions/${fixture}" --out "${iso}" > "${D}/iso.log"; fi
  "${LAB_BIN}" iso --session-fixture "${LAB_DIR}/sessions/${fixture}" --out "${D}/again.iso" > /dev/null
  local size; size="$(stat -c %s "${iso}")"
  [ "${size}" -lt 5242880 ] && check "ISO大小" 0 "${size} 字节 < 5 MB" || check "ISO大小" 1 "${size} 字节"
  cmp -s "${iso}" "${D}/again.iso" && check "确定性" 0 "同一会话重复生成字节一致" || check "确定性" 1 "两次生成不同"
  local d1 d2
  d1="$(curl -sf "${BASE}/p/${token}/boot.iso" | sha256sum | cut -d' ' -f1)"
  d2="$(curl -sf "${BASE}/p/${token}/boot.iso" | sha256sum | cut -d' ' -f1)"
  [ -n "${d1}" ] && [ "${d1}" = "${d2}" ] && check "重复下载" 0 "两次下载 sha256 一致 ${d1:0:16}…" || check "重复下载" 1 "${d1} / ${d2}"
  rf_prepare "${mac}"
  rf_client probe > "${D}/probe.log" && check "H17" 0 "sushy-tools 探测：$(jq -r '"\(.vendor // "-") 指纹 \(.fingerprint[0:16])…"' "${D}/probe.log")" || check "H17" 1 "探测失败：$(cat "${D}/probe.log")"
  local pin from; pin="$(jq -r .fingerprint "${D}/probe.log")"
  from="$(( $(wc -l < "$(svc_log serve)") + 1 ))"
  if rf_client deploy --pin "${pin}" --image "${BASE}/p/${token}/boot.iso" > "${D}/deploy.log" 2> "${D}/deploy.err"; then
    check "部署" 0 "$(jq -r -s '(.[]|select(.event=="target")|"\(.profile) \(.vmedia | split("/") | last) token=\(.token_session) "), ([.[]|select(.event=="step")|.step]|join("→"))' "${D}/deploy.log" | tr '\n' ' ')"
  else check "部署" 1 "$(tail -3 "${D}/deploy.log" "${D}/deploy.err")"; fi
  if SERVE_OFF="${from}" wait_early "${mac}" 300; then check "进入d-i(${mac})" 0 "ISO autoexec.ipxe → p/${token}/boot.ipxe → d-i，preseed 已取走"
  else check "进入d-i(${mac})" 1 "超时"; fi
  http_in_order "${from}" "/p/${token}/boot.iso" "/p/${token}/boot.ipxe" "/d-i/linux" "/d-i/initrd.gz" "/preseed.cfg" \
    && check "访问顺序" 0 "boot.iso → boot.ipxe → linux → initrd.gz → preseed.cfg" \
    || check "访问顺序" 1 "$(tail -n "+${from}" "$(svc_log serve)" | jq -R -r 'fromjson? | select(.event=="http") | .path' | tr '\n' ' ')"
  rf_cleanup
  if [ "${id}" = R2 ]; then
    "${LAB_BIN}" capture --pcap "${D}/capture.pcap" --out "${D}/all" --label r2 --mac "${mac}" > /dev/null 2>&1 || true
    local n; n="$(ls "${D}"/all/*.bin 2>/dev/null | wc -l || true)"
    [ "${n}" = 0 ] && check "H6" 0 "iPXE 与 d-i 均未发 DHCP（抓包中 ${mac} 的 DHCP 报文 0 个），静态参数生效" || check "H6" 1 "${mac} 发出 ${n} 个 DHCP 报文"
  fi
}

# 登记“BIOS + Redfish”节点必须被拒绝（redfish_requires_uefi），UEFI + Redfish 通过
sc_bios_reject() {
  "${LAB_BIN}" node-check --spec "${LAB_DIR}/nodes/redfish-bios.json" > "${D}/bios.log"
  jq -e '.ok == false and any(.errors[]; .code == "redfish_requires_uefi" and .path == "/channel")' "${D}/bios.log" > /dev/null \
    && check "BIOS拒绝" 0 "$(jq -r '.errors[0] | "\(.path) \(.code)：\(.message)"' "${D}/bios.log")" || check "BIOS拒绝" 1 "$(cat "${D}/bios.log")"
  "${LAB_BIN}" node-check --spec "${LAB_DIR}/nodes/redfish-uefi.json" > "${D}/uefi.log"
  jq -e '.ok == true' "${D}/uefi.log" > /dev/null && check "UEFI通过" 0 "UEFI + Redfish 登记通过" || check "UEFI通过" 1 "$(cat "${D}/uefi.log")"
}

# 更换 sushy-tools 证书后，用旧指纹部署必须失败并提示证书变化；重新探测确认后恢复
sc_pin_change() {
  local S="${WORK}/sushy" old new
  rf_prepare "$(mac_of a1)"
  rf_client probe > "${D}/probe-old.log"; old="$(jq -r .fingerprint "${D}/probe-old.log")"
  cp "${S}/cert.pem" "${D}/cert.pem.orig"; cp "${S}/key.pem" "${D}/key.pem.orig"
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 30 -subj "/CN=${LAB_IP}/O=x-pxe lab rotated" \
    -addext "subjectAltName=IP:${LAB_IP}" -keyout "${S}/key.pem" -out "${S}/cert.pem" 2>/dev/null
  rf_start_sushy
  local mark; mark="$(wc -l < "$(svc_log sushy)")"
  if rf_client deploy --pin "${old}" --image "${BASE}/p/lab-redfish-r1/boot.iso" > "${D}/deploy-old.log" 2> "${D}/deploy-old.err"; then
    check "旧指纹拒绝" 1 "用旧指纹部署竟然成功"
  else
    jq -e 'select(.event=="error") | .code == "cert_changed"' "${D}/deploy-old.log" > /dev/null \
      && check "旧指纹拒绝" 0 "$(jq -r 'select(.event=="error") | .message' "${D}/deploy-old.log" | cut -c1-120)" \
      || check "旧指纹拒绝" 1 "$(cat "${D}/deploy-old.log")"
  fi
  local hits; hits="$(tail -n "+$((mark + 1))" "$(svc_log sushy)" | grep -c 'InsertMedia\|PATCH' || true)"
  [ "${hits}" = 0 ] && check "未触碰BMC" 0 "证书不符时 sushy 未收到 InsertMedia/PATCH" || check "未触碰BMC" 1 "sushy 收到 ${hits} 个写请求"
  rf_client probe > "${D}/probe-new.log"; new="$(jq -r .fingerprint "${D}/probe-new.log")"
  [ "${new}" != "${old}" ] && rf_client eject --pin "${new}" > "${D}/eject-new.log" 2> "${D}/eject-new.err" \
    && check "重新确认" 0 "重新探测得到新指纹 ${new:0:16}… 后调用成功" || check "重新确认" 1 "$(cat "${D}/eject-new.log" 2>/dev/null)"
  cp "${D}/cert.pem.orig" "${S}/cert.pem"; cp "${D}/key.pem.orig" "${S}/key.pem"
  rf_cleanup
}
