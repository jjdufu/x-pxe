#!/bin/sh
set -eu
wget -q -O /tmp/xpxe-disk-select.sh 'http://10.77.0.1:8080/p/boot0001/disk-select.sh'
if ! sh /tmp/xpxe-disk-select.sh > /tmp/xpxe-disk-select.out; then
    inv=$(cat /tmp/xpxe-disk-select.out 2>/dev/null || true)
    wget -q -O /dev/null --post-data 'event=installer.failed event_id=ev-00000001 reason=disk_selection inventory='"${inv}" 'http://10.77.0.1:8080/events/installer.failed' || true
    exit 1
fi
disk=$(sed -n 's/^SELECTED=//p' /tmp/xpxe-disk-select.out)
if [ -z "${disk}" ]; then
    exit 1
fi
echo "${disk}" > /tmp/xpxe-disk-selected
debconf-set partman-auto/disk "${disk}"
debconf-set grub-installer/bootdev "${disk}"

if [ -e /sys/firmware/efi ]; then
    debconf-set partman-auto/expert_recipe 'xpxe-custom :: 512 512 512 fat32 $primary{ } method{ efi } format{ } . 1024 1024 1024 ext4 $primary{ } method{ format } format{ } use_filesystem{ } filesystem{ ext4 } mountpoint{ /boot } . 4096 4096 4096 linux-swap $lvmok{ } method{ swap } format{ } . 100 10000 -1 ext4 $lvmok{ } method{ format } format{ } use_filesystem{ } filesystem{ ext4 } mountpoint{ / } .'
else
    debconf-set partman-auto/expert_recipe 'xpxe-custom :: 1 1 1 free $iflabel{ gpt } method{ biosgrub } . 1024 1024 1024 ext4 $primary{ } method{ format } format{ } use_filesystem{ } filesystem{ ext4 } mountpoint{ /boot } . 4096 4096 4096 linux-swap $lvmok{ } method{ swap } format{ } . 100 10000 -1 ext4 $lvmok{ } method{ format } format{ } use_filesystem{ } filesystem{ ext4 } mountpoint{ / } .'
fi

exit 0