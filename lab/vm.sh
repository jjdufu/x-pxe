#!/usr/bin/env bash
# QEMU 启动封装（非 root，经 KVM 属组运行）。
# lab/vm.sh --name <n> --fw seabios|ovmf|ovmf-http --nic <model> --mac <mac> --tap <tap>
#           [--nic2 <model> --mac2 <mac> --tap2 <tap>] [--boot net-first|disk-first|nic2-first]
#           [--disks <sys>,<data>] [--kernel <f> --initrd <f>] [--cdrom <iso>] [--mem <MiB>]
source "$(dirname "$0")/lib/common.sh"
name="" fw=seabios nic=virtio-net-pci mac="" tap="" nic2="" mac2="" tap2="" boot=net-first
disks="20G,60G" kernel="" initrd="" cdrom="" mem=2048
while [ $# -gt 0 ]; do
  case "$1" in
    --name) name="$2" ;; --fw) fw="$2" ;; --nic) nic="$2" ;; --mac) mac="$2" ;; --tap) tap="$2" ;;
    --nic2) nic2="$2" ;; --mac2) mac2="$2" ;; --tap2) tap2="$2" ;; --boot) boot="$2" ;;
    --disks) disks="$2" ;; --kernel) kernel="$2" ;; --initrd) initrd="$2" ;; --cdrom) cdrom="$2" ;;
    --mem) mem="$2" ;;
    *) die "未知参数 $1" ;;
  esac
  shift 2
done
[ -n "${name}" ] && [ -n "${mac}" ] && [ -n "${tap}" ] || die "需要 --name --mac --tap"
VM="${WORK}/vm/${name}"; rm -rf "${VM}"; mkdir -p "${VM}"
case "${nic}" in virtio|virtio-net) nic=virtio-net-pci ;; esac
case "${nic2}" in virtio|virtio-net) nic2=virtio-net-pci ;; esac
qemu-img create -q -f qcow2 "${VM}/sys.qcow2" "${disks%%,*}"
qemu-img create -q -f qcow2 "${VM}/data.qcow2" "${disks##*,}"
truncate -s 1G "${VM}/vusb.raw"
nb1=1 nb2=3 db=2
case "${boot}" in disk-first) nb1=2; db=1 ;; nic2-first) nb1=3; nb2=1 ;; esac
args=(-name "${name}" -accel kvm -cpu host -smp 2 -m "${mem}" -machine q35)
case "${fw}" in
  seabios) ;;
  ovmf|ovmf-http)
    cp /usr/share/OVMF/OVMF_VARS_4M.fd "${VM}/vars.fd"
    args+=(-drive "if=pflash,format=raw,unit=0,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd"
           -drive "if=pflash,format=raw,unit=1,file=${VM}/vars.fd")
    if [ "${fw}" = ovmf-http ]; then
      args+=(-fw_cfg name=opt/org.tianocore/IPv4PXESupport,string=n
             -fw_cfg name=opt/org.tianocore/IPv6PXESupport,string=n)
    fi ;;
  *) die "未知固件 ${fw}" ;;
esac
args+=(-netdev "tap,id=n0,ifname=${tap},script=no,downscript=no"
       -device "${nic},netdev=n0,mac=${mac},bootindex=${nb1}")
if [ -n "${nic2}" ]; then
  args+=(-netdev "tap,id=n1,ifname=${tap2},script=no,downscript=no"
         -device "${nic2},netdev=n1,mac=${mac2},bootindex=${nb2}")
fi
args+=(-drive "if=none,id=sys,file=${VM}/sys.qcow2" -device "virtio-blk-pci,drive=sys,serial=SYS0001,bootindex=${db}"
       -drive "if=none,id=data,file=${VM}/data.qcow2" -device virtio-blk-pci,drive=data,serial=DATA0001
       -device qemu-xhci -drive "if=none,id=vusb,format=raw,file=${VM}/vusb.raw"
       -device usb-storage,drive=vusb,removable=on,serial=VUSB0001)
[ -n "${kernel}" ] && args+=(-kernel "${kernel}")
[ -n "${initrd}" ] && args+=(-initrd "${initrd}")
[ -n "${cdrom}" ] && args+=(-drive "if=none,id=cd,format=raw,readonly=on,file=${cdrom}" -device "ide-cd,drive=cd,bootindex=0")
args+=(-serial "file:${VM}/serial.log" -display none -monitor "unix:${VM}/mon.sock,server,nowait"
       -daemonize -pidfile "${VM}/qemu.pid")
printf '%q ' qemu-system-x86_64 "${args[@]}" > "${VM}/cmdline"
as_kvm "$(cat "${VM}/cmdline")"
log "VM ${name} 已启动（${fw}/${nic} ${mac} @${tap}${nic2:+ + ${nic2} ${mac2} @${tap2}}）"
echo "${VM}"
