INTERFACE="ens192"
IP_START="192.168.2.110"
IP_END="192.168.2.120"
NETMASK="255.255.255.0"
GATEWAY="192.168.2.254"
DNS="192.168.2.2,1.1.1.1"
PXE_HOSTIP="192.168.2.3"
REPO_URL="192.168.2.5:8081"
NTP_SRV="192.168.2.2"

apt install dnsmasq -y

mkdir -p /tftpboot/{menu,debian,iso,files}

# [手动操作] 上传 ipxe.efi 和 undionly.kpxe
# scp undionly.kpxe ipxe.efi root@192.168.2.3:/tftpboot



tee <<EOF> /etc/dnsmasq.d/ipxe-debian.conf
# ----- DHCP CONFIG -----
interface=$INTERFACE
dhcp-range=$IP_START,$IP_END,12h
# 禁止记录 mac
dhcp-ignore-names
# 唯一的 DHCP 服务器
dhcp-authoritative

dhcp-option=1,$NETMASK
dhcp-option=3,$GATEWAY
dhcp-option=6,$DNS

# ----- TFTP CONFIG -----
enable-tftp
tftp-root=/tftpboot

# ----- PXE CONFIG -----
dhcp-match=set:ipxe,175
dhcp-vendorclass=BIOS,PXEClient:Arch:00000
dhcp-boot=tag:!ipxe,tag:BIOS,undionly.kpxe,$PXE_HOSTIP
dhcp-boot=tag:!ipxe,tag:!BIOS,ipxe.efi,$PXE_HOSTIP
dhcp-boot=tag:ipxe,menu/boot.ipxe

# ----- ENABLE LOGGING -----
log-queries
log-facility=/var/log/dnsmasq.log
EOF

echo "port=0" >> /etc/dnsmasq.conf

  tee <<EOF> /tftpboot/debian/preseed.cfg
d-i debian-installer/locale string en_US.UTF-8
d-i debian-installer/language string en
d-i debian-installer/country string US
d-i console-setup/ask_detect boolean false
d-i keyboard-configuration/xkb-keymap select us

d-i netcfg/disable_autoconfig boolean true

d-i passwd/root-login boolean true
d-i passwd/make-user boolean false
d-i passwd/root-password password REDACTED
d-i passwd/root-password-again password REDACTED
d-i user-setup/allow-password-weak boolean true

d-i mirror/country string manual
d-i mirror/http/hostname string $REPO_URL
d-i mirror/http/directory string /repository/debian
d-i mirror/http/proxy string
d-i mirror/no-default-repositories boolean true
d-i mirror/suite string bookworm
d-i mirror/udeb/suite string bookworm
d-i apt-setup/security_host string
d-i apt-setup/security_path string


d-i clock-setup/utc boolean true
d-i time/zone string Asia/Shanghai
d-i clock-setup/ntp boolean true
d-i clock-setup/ntp-server string $NTP_SRV


d-i partman-auto/method string lvm
d-i partman-lvm/device_remove_lvm boolean true
d-i partman-md/device_remove_md boolean true
d-i partman-lvm/confirm boolean true
d-i partman-auto/choose_recipe select atomic
d-i partman-auto-lvm/guided_size string max
# Dynamically detect disk name
d-i preseed/early_command string \
    echo "d-i partman-auto/disk string /dev/$(lsblk -dno NAME | head -n1)" > /tmp/disk-preseed.cfg; \
    debconf-set-selections /tmp/disk-preseed.cfg
d-i partman-lvm/confirm_nooverwrite boolean true
d-i partman/choose_partition select finish
d-i partman/confirm boolean true
d-i partman/confirm_nooverwrite boolean true


tasksel tasksel/first multiselect ssh-server, standard
popularity-contest popularity-contest/participate boolean false
d-i finish-install/reboot_in_progress note

d-i preseed/late_command string \
    in-target wget -O /tmp/init.sh http://$PXE_HOSTIP/init-debian.sh; \
    in-target chmod +x /tmp/init.sh; \
    in-target /bin/bash /tmp/init.sh
EOF
EOF



tee <<EOF> /tftpboot/menu/boot.ipxe
#!ipxe

:start
menu PXE Boot Options
item --gap -- ---- Network Boot ----
item debian12-net Debian 12 installation
item --gap -- ---- Utility ----
item shell iPXE shell
item exit  Exit to BIOS

choose --default debian12-net --timeout 10000 option && goto \${option}

:shell
shell
goto start

:debian12-net
set server_root tftp://$PXE_HOSTIP/debian
kernel \${server_root}/linux auto=true priority=high interface=auto preseed/url=\${server_root}/preseed.cfg
initrd \${server_root}/initrd.gz
boot

:exit
exit

:failed
echo Boot failed! Press any key to reboot...
prompt
reboot

EOF


systemctl restart dnsmasq
systemctl enable dnsmasq
systemctl status dnsmasq


cp /tmp/nginx-debian-1261.7z /usr/local/
7z x nginx-debian-1261.7z
cd nginx
./install
cd

systemctl restart cyberngx

# 自编译 ipxe 
# apt install build-essential liblzma-dev isolinux mkisofs -y
# git clone https://github.com/ipxe/ipxe.git /tmp && cd /tmp/ipxe/src
# make
# 复制
# cp -v /tmp/ipxe/src/bin/undionly.kpxe /tftpboot
# cd /tftpboot && wget https://boot.ipxe.org/ipxe.efi


# 下载

# https://mirrors.pku.edu.cn/debian/dists/bookworm/main/installer-amd64/current/images/netboot/debian-installer/amd64/linux
# https://mirrors.pku.edu.cn/debian/dists/bookworm/main/installer-amd64/current/images/netboot/debian-installer/amd64/initrd.gz


# 清空 dnsmaq 已分配的
# >/var/lib/misc/dnsmasq.leases
# systemctl restart dnsmasq

