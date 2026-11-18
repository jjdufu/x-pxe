#!/bin/sh
set -eu
mkdir -p /target/usr/local/sbin /target/var/lib/xpxe /target/etc/systemd/system
wget -O /target/usr/local/sbin/xpxe-agent 'http://10.77.0.1:8080/p/boot0001/agent'
echo 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  /target/usr/local/sbin/xpxe-agent' | sha256sum -c
chmod 0755 /target/usr/local/sbin/xpxe-agent
wget -O /target/var/lib/xpxe/plan.json 'http://10.77.0.1:8080/p/agent001/plan.json'
echo 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  /target/var/lib/xpxe/plan.json' | sha256sum -c
chmod 0600 /target/var/lib/xpxe/plan.json
cat > /target/etc/systemd/system/xpxe-firstboot.service << 'XPXE_UNIT'
[Unit]
Description=x-pxe first boot configuration
Wants=network-online.target
After=network-online.target
ConditionPathExists=/var/lib/xpxe/plan.json

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/xpxe-agent run --plan /var/lib/xpxe/plan.json
StandardOutput=journal+console
StandardError=journal+console

[Install]
WantedBy=multi-user.target
XPXE_UNIT
in-target systemctl enable xpxe-firstboot.service
in-target cat /etc/network/interfaces || true
wget -q -O /dev/null --post-data 'event=installer.finished event_id=ev-00000001' 'http://10.77.0.1:8080/p/boot0001/events/installer.finished' || true