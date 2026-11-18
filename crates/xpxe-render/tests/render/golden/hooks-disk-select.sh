#!/bin/sh
# x-pxe 确定性选盘：从 ${XPXE_SYSFS:-/sys} 读取，供 d-i busybox 与实验室 /bin/sh 执行。
set -eu
SYSFS="${XPXE_SYSFS:-/sys}"
DEVROOT="${XPXE_DEV:-/dev}"
BYID="${XPXE_BY_ID:-/dev/disk/by-id}"
MIN_GIB=16
MAX_GIB=
PREFER='unique'
# 排除型号特征，一行一条
EXCLUDE=$(cat <<'XPXE_EXCLUDE'
Virtual
iDRAC
iLO
AMI
Cypress
Linux File-Stor
RMM
Virtual CDROM
Virtual Floppy
XPXE_EXCLUDE
)

is_skipped_name() {
    case "$1" in
        loop*|ram*|sr*|fd*|dm-*|md*|zram*) return 0 ;;
        *) return 1 ;;
    esac
}

read_file() {
    if [ -f "$1" ]; then
        tr -d '\n' < "$1" | tr -s ' '
    fi
}

device_path() {
    if [ -L "$SYSFS/block/$1" ]; then
        readlink "$SYSFS/block/$1" || true
    fi
    if [ -L "$SYSFS/block/$1/device" ]; then
        readlink "$SYSFS/block/$1/device" || true
    fi
}

transport_of() {
    path=$(device_path "$1")
    case "$path" in
        *nvme*) echo nvme ;;
        *usb*) echo usb ;;
        *virtio*) echo virtio ;;
        *sas*|*sata*|*ata*) echo sata ;;
        *scsi*) echo scsi ;;
        *) echo unknown ;;
    esac
}

# 打印：name|sectors|gib|transport|vendor|model|serial|reason
inventory_line() {
    echo "$1|$2|$3|$4|$5|$6|$7|$8"
}

is_excluded_model() {
    blob="$1 $2"
    # 不用管道，避免 while 在子 shell 中无法把命中传回函数。
    printf '%s\n' "$EXCLUDE" > /tmp/xpxe-exclude.$$
    while IFS= read -r pat; do
        [ -z "$pat" ] && continue
        case "$blob" in
            *"$pat"*)
                rm -f /tmp/xpxe-exclude.$$
                return 0
                ;;
        esac
    done < /tmp/xpxe-exclude.$$
    rm -f /tmp/xpxe-exclude.$$
    return 1
}

# include 规则由生成器展开为函数
include_ok() {
    name="$1"
    serial="$2"
    model="$3"
    transport="$4"
    return 0
}

candidates=""
inventory=""
# 遍历块设备
for dir in "$SYSFS"/block/*; do
    [ -d "$dir" ] || continue
    name=$(basename "$dir")
    if is_skipped_name "$name"; then
        inventory="$inventory
$(inventory_line "$name" 0 0 unknown "" "" "" "prefix")"
        continue
    fi
    rem=$(read_file "$dir/removable")
    ro=$(read_file "$dir/ro")
    sectors=$(read_file "$dir/size")
    sectors=${sectors:-0}
    vendor=$(read_file "$dir/device/vendor")
    model=$(read_file "$dir/device/model")
    [ -n "$model" ] || model=$(read_file "$dir/model")
    serial=$(read_file "$dir/serial")
    [ -n "$serial" ] || serial=$(read_file "$dir/device/serial")
    [ -n "$serial" ] || serial=$(read_file "$dir/device/wwid")
    transport=$(transport_of "$name")
    path=$(device_path "$name")
    gib=0
    if [ "$sectors" -gt 0 ]; then
        gib=$((sectors / 2097152))
    fi
    reason=""
    if [ "${rem:-0}" = "1" ]; then
        reason=removable
    elif [ "${ro:-0}" = "1" ]; then
        reason=readonly
    elif [ "$sectors" -eq 0 ]; then
        reason=zero_size
    elif echo "$path" | grep -q '/usb'; then
        reason=usb
    elif is_excluded_model "$vendor" "$model"; then
        reason=exclude_model
    elif [ "$gib" -lt "$MIN_GIB" ]; then
        reason=too_small
    elif [ -n "$MAX_GIB" ] && [ "$gib" -gt "$MAX_GIB" ]; then
        reason=too_large
    elif ! include_ok "$name" "$serial" "$model" "$transport"; then
        reason=include
    fi
    inventory="$inventory
$(inventory_line "$name" "$sectors" "$gib" "$transport" "$vendor" "$model" "$serial" "${reason:-ok}")"
    if [ -z "$reason" ]; then
        candidates="$candidates $name:$gib"
    fi
done

count=0
for _item in $candidates; do
    count=$((count + 1))
done

selected=""
if [ "$PREFER" = "unique" ]; then
    if [ "$count" -eq 1 ]; then
        selected=${candidates# }
        selected=${selected%%:*}
    else
        echo "ERROR=disk_selection"
        echo "SELECTED="
        echo "INVENTORY:"
        echo "$inventory"
        exit 1
    fi
elif [ "$PREFER" = "smallest" ] || [ "$PREFER" = "largest" ]; then
    if [ "$count" -lt 1 ]; then
        echo "ERROR=disk_selection"
        echo "SELECTED="
        echo "INVENTORY:"
        echo "$inventory"
        exit 1
    fi
    best=""
    best_gib=""
    for item in $candidates; do
        nm=${item%%:*}
        sz=${item##*:}
        if [ -z "$best" ]; then
            best=$nm
            best_gib=$sz
        elif [ "$PREFER" = "smallest" ] && [ "$sz" -lt "$best_gib" ]; then
            best=$nm
            best_gib=$sz
        elif [ "$PREFER" = "largest" ] && [ "$sz" -gt "$best_gib" ]; then
            best=$nm
            best_gib=$sz
        fi
    done
    selected=$best
else
    echo "ERROR=disk_selection"
    echo "SELECTED="
    echo "INVENTORY:"
    echo "$inventory"
    exit 1
fi

echo "SELECTED=${DEVROOT}/${selected}"
echo "INVENTORY:"
echo "$inventory"
exit 0