#!/bin/bash
# x-pxe 初始化模块 users_keys：用户与公钥
set -euo pipefail

if grep -qE '^#? ?SHELL=' /etc/default/useradd; then
  sed -i -E 's|^#? ?SHELL=.*|SHELL=/bin/bash|' /etc/default/useradd
else
  echo 'SHELL=/bin/bash' >> /etc/default/useradd
fi
install -d -m 700 /root/.ssh
touch /root/.ssh/authorized_keys
grep -qxF 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBtCscA5CqkUDclClV2dKSGH4JxR2vLulpTgLR34Pz1l jay@localhost' /root/.ssh/authorized_keys || echo 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBtCscA5CqkUDclClV2dKSGH4JxR2vLulpTgLR34Pz1l jay@localhost' >> /root/.ssh/authorized_keys
chmod 600 /root/.ssh/authorized_keys

# ---- jwczjzl ----
if ! id -u 'jwczjzl' &>/dev/null; then
  useradd -m -s '/bin/bash' 'jwczjzl'
fi
usermod -s '/bin/bash' 'jwczjzl'
passwd -l 'jwczjzl' >/dev/null
usermod -aG 'sudo' 'jwczjzl'
home="$(getent passwd 'jwczjzl' | cut -d: -f6)"
install -d -m 700 "${home}/.ssh"
if [[ ! -f "${home}/.ssh/authorized_keys" ]]; then
  if [[ -f /root/.ssh/authorized_keys ]]; then
    cp /root/.ssh/authorized_keys "${home}/.ssh/authorized_keys"
  else
    touch "${home}/.ssh/authorized_keys"
  fi
fi
grep -qxF 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBtCscA5CqkUDclClV2dKSGH4JxR2vLulpTgLR34Pz1l jay@localhost' "${home}/.ssh/authorized_keys" || echo 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBtCscA5CqkUDclClV2dKSGH4JxR2vLulpTgLR34Pz1l jay@localhost' >> "${home}/.ssh/authorized_keys"
chmod 600 "${home}/.ssh/authorized_keys"
chown -R 'jwczjzl': "${home}/.ssh"
loginctl enable-linger 'jwczjzl'
