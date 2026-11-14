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
  svc_stop sushy
  svc_start sushy root /opt/xpxe-lab/sushy/bin/sushy-emulator --config "${WORK}/sushy/emulator.conf"
  local i; for i in $(seq 1 30); do rf GET /redfish/v1/ >/dev/null 2>&1 && break; sleep 1; done
  RF_UUID="${uuid}"
}

rf_cleanup() {
  sudo cp "${RF_VM}/serial.log" "${D}/serial-redfish.log" 2>/dev/null && sudo chown "$(id -un)" "${D}/serial-redfish.log"
  sudo virsh -q -c qemu:///system destroy vm-redfish >/dev/null 2>&1 || true
  sudo virsh -q -c qemu:///system undefine --nvram vm-redfish >/dev/null 2>&1 || true
  svc_stop sushy
  [ "${KEEP}" = 1 ] || sudo rm -rf "${RF_VM}"
}

sc_R() { # <R1|R2> <tap>
  local id="$1" mid; [ "${id}" = R1 ] && mid=a1 || mid=a2
  local mac; mac="$(mac_of "${mid}")"
  local script="${WORK}/www/autoexec-${id}.ipxe"
  if [ "${id}" = R1 ]; then
    printf '#!ipxe\necho x-pxe-lab %s autoexec.ipxe\ndhcp\nchain %s/boot.ipxe\n' "${id}" "${BASE}" > "${script}"
  else
    local ip=10.77.0.60
    cat > "${script}" <<AUTO
#!ipxe
echo x-pxe-lab R2 autoexec.ipxe static ${ip}
set net0/ip ${ip}
set net0/netmask 255.255.255.0
set net0/gateway ${LAB_IP}
ifopen net0
kernel ${BASE}/d-i/linux initrd=initrd.gz auto=true priority=critical url=${BASE}/preseed.cfg?mac=\${net0/mac:hexhyp} netcfg/disable_autoconfig=true netcfg/get_ipaddress=${ip} netcfg/get_netmask=255.255.255.0 netcfg/get_gateway=${LAB_IP} netcfg/get_nameservers=${LAB_IP} netcfg/confirm_static=true console=ttyS0,115200n8 ---
initrd ${BASE}/d-i/initrd.gz
boot
AUTO
  fi
  "${LAB_BIN}" iso --efi "${WORK}/www/fw/x64/ipxe.efi" --script "${script}" --out "${WORK}/www/boot-${id}.iso" > "${D}/iso.log"
  rf_prepare "${mac}"
  rf GET /redfish/v1/Systems > "${D}/systems.json" && check "H17" 0 "手动拉起的 libvirtd + sushy-tools 提供 Redfish（$(jq -r '.Members|length' "${D}/systems.json") 个系统）" || check "H17" 1 "sushy-tools 不可用"
  local sys="/redfish/v1/Systems/${RF_UUID}"
  rf GET "${sys}" > "${D}/system.json"
  rf GET "${sys}/VirtualMedia" > "${D}/vmedia.json" || true
  local dev
  dev="$(jq -r '.Members[0]."@odata.id" // empty' "${D}/vmedia.json" 2>/dev/null || true)"
  [ -n "${dev}" ] || dev="${sys}/VirtualMedia/Cd"
  rf POST "${dev}/Actions/VirtualMedia.InsertMedia" \
    "{\"Image\":\"${BASE}/boot-${id}.iso\",\"Inserted\":true,\"WriteProtected\":true}" > "${D}/insert.json" \
    && check "InsertMedia" 0 "${dev}（Systems 下）" || check "InsertMedia" 1 "$(cat "${D}/insert.json")"
  rf PATCH "${sys}" '{"Boot":{"BootSourceOverrideEnabled":"Once","BootSourceOverrideTarget":"Cd","BootSourceOverrideMode":"UEFI"}}' > "${D}/boot.json" \
    && check "BootOverride" 0 "Once/Cd/UEFI" || check "BootOverride" 1 "$(cat "${D}/boot.json")"
  rf POST "${sys}/Actions/ComputerSystem.Reset" '{"ResetType":"On"}' > "${D}/reset.json" \
    && check "Reset" 0 "ResetType=On" || check "Reset" 1 "$(cat "${D}/reset.json")"
  if wait_early "${mac}" 300; then check "进入d-i(${mac})" 0 "ISO 内 autoexec.ipxe → HTTP → d-i，preseed 已取走"
  else check "进入d-i(${mac})" 1 "超时"; fi
  rf_cleanup
  if [ "${id}" = R2 ]; then
    "${LAB_BIN}" capture --pcap "${D}/capture.pcap" --out "${D}/all" --label r2 --mac "${mac}" > /dev/null 2>&1 || true
    local n; n="$(ls "${D}"/all/*.bin 2>/dev/null | wc -l || true)"
    [ "${n}" = 0 ] && check "H6" 0 "iPXE 与 d-i 均未发 DHCP（抓包中 ${mac} 的 DHCP 报文 0 个），静态参数生效" || check "H6" 1 "${mac} 发出 ${n} 个 DHCP 报文"
  fi
}
