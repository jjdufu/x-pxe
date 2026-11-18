#!/bin/sh
set -eu
wget -q -O /tmp/xpxe-disk-select.sh 'http://10.77.0.1:8080/p/boot0001/disk-select.sh'
if ! sh /tmp/xpxe-disk-select.sh > /tmp/xpxe-disk-select.out; then
    inv=$(cat /tmp/xpxe-disk-select.out 2>/dev/null || true)
    wget -q -O /dev/null --post-data 'event=installer.failed event_id=ev-00000001 reason=disk_selection inventory='"${inv}" 'http://10.77.0.1:8080/p/boot0001/events/installer.failed' || true
    exit 1
fi
disk=$(sed -n 's/^SELECTED=//p' /tmp/xpxe-disk-select.out)
if [ -z "${disk}" ]; then
    exit 1
fi
echo "${disk}" > /tmp/xpxe-disk-selected
debconf-set partman-auto/disk "${disk}"
debconf-set grub-installer/bootdev "${disk}"
# 回读确认：引导器必须装到选中的盘，绝不回落到 grub-installer 的“第一块盘”（可能是 BMC 虚拟 U 盘）
if [ "$(debconf-get partman-auto/disk)" != "${disk}" ] || [ "$(debconf-get grub-installer/bootdev)" != "${disk}" ]; then
    wget -q -O /dev/null --post-data 'event=installer.failed event_id=ev-00000001 reason=disk_selection inventory='"debconf-set-failed ${disk}" 'http://10.77.0.1:8080/p/boot0001/events/installer.failed' || true
    exit 1
fi

exit 0