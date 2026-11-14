#!/usr/bin/env bash
# CI 冒烟：编译并运行与引导链路相关的单元测试；若本机已有 QEMU/OVMF/iPXE 且当前用户能访问 /dev/kvm，
# 再做一次无网卡的 UEFI ISO 引导（验证写出器，不搭实验网、不发 DHCP）。
# 完整矩阵只在开发机上跑 lab/run.sh，结论见 docs/lab/。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
cargo test -q -p xpxe-dhcp -p xpxe-tftp -p xpxe-image -p xpxe-lab
if [ ! -e /dev/kvm ] || [ ! -r /dev/kvm ] || [ ! -w /dev/kvm ]; then
  echo "ci: 跳过 ISO 引导（当前环境无法读写 /dev/kvm）"
  exit 0
fi
for f in /usr/bin/qemu-system-x86_64 /usr/share/OVMF/OVMF_CODE_4M.fd /usr/lib/ipxe/ipxe.efi; do
  if [ ! -e "$f" ]; then
    echo "ci: 跳过 ISO 引导（缺少 $f，完整实验室依赖见 lab/setup.sh）"
    exit 0
  fi
done
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT
printf '#!ipxe\necho XPXE-CI-AUTOEXEC\npoweroff\n' > "${tmp}/autoexec.ipxe"
cargo build -q --release -p xpxe-lab
target/release/xpxe-lab iso --efi /usr/lib/ipxe/ipxe.efi --script "${tmp}/autoexec.ipxe" --out "${tmp}/boot.iso"
cp /usr/share/OVMF/OVMF_VARS_4M.fd "${tmp}/vars.fd"
timeout 60 qemu-system-x86_64 -accel kvm -machine q35 -m 512 \
  -drive if=pflash,format=raw,unit=0,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd \
  -drive if=pflash,format=raw,unit=1,file="${tmp}/vars.fd" \
  -cdrom "${tmp}/boot.iso" -nic none -display none -serial "file:${tmp}/serial.log" \
  || true
if strings "${tmp}/serial.log" | grep -q XPXE-CI-AUTOEXEC; then
  echo "ci: ISO 引导看到 XPXE-CI-AUTOEXEC"
else
  echo "ci: ISO 引导未看到脚本输出" >&2
  strings "${tmp}/serial.log" | tail -20 >&2 || true
  exit 1
fi
