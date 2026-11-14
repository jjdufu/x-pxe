#!/usr/bin/env bash
# 幂等：安装软件包、KVM 权限、启动 libvirtd/virtlogd、sushy-tools、实验室镜像、d-i 与 iPXE 文件。
source "$(dirname "$0")/lib/common.sh"

log "安装软件包"
"${LAB_DIR}/lib/apt-install.sh" qemu-system-x86 qemu-utils ovmf ipxe-qemu ipxe seabios \
  libvirt-daemon-system libvirt-clients python3-libvirt python3-venv \
  iproute2 iptables iputils-ping tcpdump socat openssl apache2-utils curl xz-utils sqv debian-archive-keyring

log "KVM 权限（/dev/kvm 属组 $(kvm_group)）"
sudo usermod -aG "$(kvm_group)" "$(id -un)"
id libvirt-qemu >/dev/null 2>&1 && sudo usermod -aG "$(kvm_group)" libvirt-qemu
as_kvm 'test -r /dev/kvm && test -w /dev/kvm' && log "kvm-ok"

log "libvirt（无 systemd，手动拉起）"
pgrep -x virtlogd >/dev/null || sudo virtlogd -d
if ! pgrep -x libvirtd >/dev/null; then
  sudo sed -i 's/^#\?cgroup_controllers = .*/cgroup_controllers = [ ]/' /etc/libvirt/qemu.conf
  sudo libvirtd -d
  sleep 2
fi
sudo virsh -c qemu:///system version >/dev/null && log "libvirt-ok"
sudo virsh -c qemu:///system net-destroy default >/dev/null 2>&1 || true
sudo virsh -c qemu:///system net-autostart default --disable >/dev/null 2>&1 || true
# sushy-tools 下载虚拟介质时写入 libvirt 的 default 存储池（与 default 网络无关）
if ! sudo virsh -c qemu:///system pool-info default >/dev/null 2>&1; then
  sudo virsh -c qemu:///system pool-define-as default dir --target /var/lib/libvirt/images >/dev/null
fi
sudo virsh -c qemu:///system pool-start default >/dev/null 2>&1 || true
sudo virsh -c qemu:///system pool-autostart default >/dev/null 2>&1 || true

log "sushy-tools"
if [ ! -x /opt/xpxe-lab/sushy/bin/sushy-emulator ]; then
  sudo mkdir -p /opt/xpxe-lab && sudo chown "$(id -un)" /opt/xpxe-lab
  python3 -m venv --system-site-packages /opt/xpxe-lab/sushy
  /opt/xpxe-lab/sushy/bin/pip install -q -r "${LAB_DIR}/sushy/requirements.txt"
fi
S="${WORK}/sushy"; mkdir -p "${S}"
if [ ! -f "${S}/cert.pem" ]; then
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 30 \
    -subj /CN=lab-bmc -addext "subjectAltName=IP:${LAB_IP}" \
    -keyout "${S}/key.pem" -out "${S}/cert.pem" 2>/dev/null
fi
if [ ! -f "${S}/password" ]; then
  ( umask 077; openssl rand -hex 12 > "${S}/password" )
  htpasswd -nbB admin "$(cat "${S}/password")" > "${S}/htpasswd"
fi

log "实验室镜像"
docker build -q -t xpxe-lab/kea "${LAB_DIR}/images/kea" >/dev/null
docker build -q -t xpxe-lab/acng "${LAB_DIR}/images/acng" >/dev/null

log "探针二进制"
(cd "${REPO_DIR}" && cargo build -q --release -p xpxe-lab)
sudo install -d -m 0755 "${LAB_BIN_DIR}"
sudo install -m 0755 "${REPO_DIR}/target/release/xpxe-lab" "${LAB_BIN}"

log "Debian 安装器（InRelease 签名 → SHA256SUMS → linux/initrd.gz）"
DI="${WORK}/di"; mkdir -p "${DI}"
M="${XPXE_LAB_DEBIAN_MIRROR:-https://deb.debian.org/debian}/dists/trixie"
curl -fsSL -o "${DI}/InRelease" "${M}/InRelease"
sqv --keyring /usr/share/keyrings/debian-archive-keyring.pgp --cleartext --output "${DI}/Release" "${DI}/InRelease" >/dev/null
curl -fsSL -o "${DI}/SHA256SUMS" "${M}/main/installer-amd64/current/images/SHA256SUMS"
want="$(awk '/^SHA256:/{s=1;next} /^[A-Za-z]/{s=0} s && $3=="main/installer-amd64/current/images/SHA256SUMS"{print $1}' "${DI}/Release")"
[ "$(sha256sum "${DI}/SHA256SUMS" | cut -d' ' -f1)" = "${want}" ] || die "SHA256SUMS 与 InRelease 不符"
for f in linux initrd.gz; do
  sum="$(awk -v p="./netboot/debian-installer/amd64/${f}" '$2==p{print $1}' "${DI}/SHA256SUMS")"
  if [ ! -f "${DI}/${f}" ] || [ "$(sha256sum "${DI}/${f}" | cut -d' ' -f1)" != "${sum}" ]; then
    curl -fsSL -o "${DI}/${f}" "${M}/main/installer-amd64/current/images/netboot/debian-installer/amd64/${f}"
  fi
  [ "$(sha256sum "${DI}/${f}" | cut -d' ' -f1)" = "${sum}" ] || die "${f} 校验失败"
done

log "HTTP/TFTP 根目录"
W="${WORK}/www"; mkdir -p "${W}/d-i" "${W}/fw/x64" "${W}/node"
cp "${LAB_DIR}/www/"*.cfg "${LAB_DIR}/www/"*.ipxe "${W}/"
ln -sf "${DI}/linux" "${W}/d-i/linux"; ln -sf "${DI}/initrd.gz" "${W}/d-i/initrd.gz"
cp /usr/lib/ipxe/ipxe.efi "${W}/fw/x64/ipxe.efi"
T="${WORK}/tftp"; mkdir -p "${T}"
cp /usr/lib/ipxe/ipxe.efi /usr/lib/ipxe/snponly.efi /usr/lib/ipxe/undionly.kpxe "${T}/"
chmod -R a+rX "${WORK}"
log "setup 完成"
