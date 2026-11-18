#!/usr/bin/env bash
# Step 5 等价性：两台全新 Debian 13 云镜像虚拟机，A 跑 legacy/init-d13.sh，B 跑 deb13-init-scripts 计划（xpxe-agent），
# 比较约定路径。被 lab/run.sh source；入口 ieq_main [--plan <plan.json>] [--legacy <脚本>]。
# 两台机器的 APT 源统一改为实验室 APT 缓存（虚拟机没有外网），口令与公钥换成测试值。
IEQ="${WORK}/initeq"
IEQ_WWW="${WORK}/www/initeq"
IEQ_IMAGE_URL=https://cloud.debian.org/images/cloud/trixie/latest
IEQ_IMAGE="${WORK}/cloud/debian-13-genericcloud-amd64.qcow2"
IEQ_PASS=xpxe-lab-init
IEQ_AGENT_TOKEN=labagente2
# 版本 B 相对 legacy 的计划内差异：motd 头部两行回退（docs/plan/step-05 §4）
IEQ_MOTD_EXTRA=('PROCESSOR_NAME=${PROCESSOR_NAME:-$(uname -m)}' '    [[ "${DISK_USAGE}" =~ ^[0-9]+$ ]] || DISK_USAGE=0')
# 比较的路径（相对 /）
IEQ_PATHS=(etc/ssh/sshd_config.d etc/sudoers.d etc/sysctl.d etc/security/limits.d etc/profile.d
           etc/update-motd.d/00-custom-header etc/default/locale etc/locale.conf etc/locale.gen root/.vimrc home/jwczjzl/.vimrc)

ieq_image() {
  [ -f "${IEQ_IMAGE}" ] && return 0
  mkdir -p "$(dirname "${IEQ_IMAGE}")"
  log "下载 Debian 13 云镜像"
  curl -sSfL -o "${IEQ_IMAGE}.part" "${IEQ_IMAGE_URL}/$(basename "${IEQ_IMAGE}")"
  curl -sSfL -o "${IEQ_IMAGE%/*}/SHA512SUMS" "${IEQ_IMAGE_URL}/SHA512SUMS"
  (cd "${IEQ_IMAGE%/*}" && mv "$(basename "${IEQ_IMAGE}").part" "$(basename "${IEQ_IMAGE}")" \
    && grep " $(basename "${IEQ_IMAGE}")\$" SHA512SUMS | sha512sum -c --quiet -) || die "云镜像校验失败"
}

ieq_ssh() { # <ip> <命令>
  ssh -n -i "${IEQ}/id_ed25519" -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -o ConnectTimeout=5 -o LogLevel=ERROR "jwczjzl@$1" "$2"
}
ieq_sudo() { ieq_ssh "$1" "echo '${IEQ_PASS}' | sudo -S -p '' bash -c $(printf '%q' "$2")"; }

ieq_seed() { # <名字> <mac> <ip> <runcmd 脚本>
  local d="${IEQ}/seed-$1"
  rm -rf "${d}"; mkdir -p "${d}"
  printf 'instance-id: initeq-%s-%s\nlocal-hostname: initeq\n' "$1" "$(date +%s)" > "${d}/meta-data"
  cat > "${d}/network-config" << NET
version: 2
ethernets:
  lab0:
    match:
      macaddress: "$2"
    addresses: [$3/24]
NET
  cat > "${d}/user-data" << UD
#cloud-config
users: []
ssh_pwauth: false
runcmd:
  - [bash, -c, "curl -fsS ${BASE}/initeq/prep.sh | bash && curl -fsS ${BASE}/initeq/$4 | bash"]
UD
  echo "${d}"
}

ieq_www() { # <legacy> <plan>
  rm -rf "${IEQ_WWW}"; mkdir -p "${IEQ_WWW}"
  local pub; pub="$(cat "${IEQ}/id_ed25519.pub")"
  # legacy：只替换公钥（口令走环境变量 JWCZJZL_PASSWORD）；末尾 reboot 改由包装脚本在回报后执行
  sed -e "s|^SSH_PUBKEY=.*|SSH_PUBKEY='${pub}'|" -e 's|^reboot$|: reboot（由实验室包装脚本执行）|' "$1" > "${IEQ_WWW}/init-d13.sh"
  cp "$2" "${IEQ_WWW}/plan.json"
  cp "${REPO_DIR}/target/x86_64-unknown-linux-musl/release/xpxe-agent" "${IEQ_WWW}/xpxe-agent"
  cp "${REPO_DIR}/crates/xpxe-render/src/templates/firstboot.service.j2" "${IEQ_WWW}/xpxe-firstboot.service"
  cat > "${IEQ_WWW}/prep.sh" << PREP
set -eu
# 两台机器相同的准备：APT 源指向实验室缓存
rm -f /etc/apt/sources.list.d/* /etc/apt/mirrors/*.list
: > /etc/apt/sources.list
cat > /etc/apt/sources.list.d/debian.sources << 'SRC'
Types: deb
URIs: http://${LAB_IP}:3142/deb.debian.org/debian
Suites: trixie trixie-updates
Components: main
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg

Types: deb
URIs: http://${LAB_IP}:3142/deb.debian.org/debian-security
Suites: trixie-security
Components: main
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
SRC
curl -fsS -X POST --data "\$(hostname -I)" ${BASE}/initeq/prep-done || true
PREP
  cat > "${IEQ_WWW}/run-legacy.sh" << RUN
curl -fsSo /root/init-d13.sh ${BASE}/initeq/init-d13.sh
rc=0
JWCZJZL_PASSWORD='${IEQ_PASS}' bash /root/init-d13.sh > /var/tmp/legacy-run.log 2>&1 || rc=\$?
curl -fsS -X POST --data "rc=\${rc}" ${BASE}/initeq/legacy-done || true
[ "\${rc}" = 0 ] && systemctl reboot
RUN
  cat > "${IEQ_WWW}/run-agent.sh" << RUN
set -eu
install -d -m 0700 /var/lib/xpxe
curl -fsSo /usr/local/sbin/xpxe-agent ${BASE}/initeq/xpxe-agent
chmod 0755 /usr/local/sbin/xpxe-agent
curl -fsSo /var/lib/xpxe/plan.json ${BASE}/initeq/plan.json
chmod 0600 /var/lib/xpxe/plan.json
curl -fsSo /etc/systemd/system/xpxe-firstboot.service ${BASE}/initeq/xpxe-firstboot.service
/usr/local/sbin/xpxe-agent verify --plan /var/lib/xpxe/plan.json
systemctl enable xpxe-firstboot.service
curl -fsS -X POST --data staged ${BASE}/initeq/agent-staged || true
systemctl reboot
RUN
}

ieq_render() { # → 计划路径；公钥、口令哈希、事件地址换成实验室值
  local pub hash fp out="${IEQ}/plan.json"
  pub="$(cat "${IEQ}/id_ed25519.pub")"
  hash="$(openssl passwd -6 -salt xpxelab "${IEQ_PASS}")"
  fp="$(printf '%s' "${hash}" | sha256sum | cut -c1-8)"
  "${REPO_DIR}/target/release/xpxe-lab" render-plan --preset deb13-init-scripts --out "${out}" \
    --session lab-initeq --events-url "${BASE}/p/${IEQ_AGENT_TOKEN}/events" \
    --set "/modules/users_keys/settings/root_authorized_keys=[\"${pub}\"]" \
    --set "/modules/users_keys/settings/users/0/authorized_keys=[\"${pub}\"]" \
    --set "/modules/users_keys/settings/users/0/password={\"type\":\"hash\",\"hash\":{\"secret_ref\":\"crypt_hash\",\"fingerprint\":\"${fp}\",\"hash\":\"${hash}\"}}" >&2 \
    || die "渲染计划失败"
  echo "${out}"
}

ieq_wait_ssh() { # <ip> <秒>
  local deadline=$((SECONDS + $2))
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    ieq_ssh "$1" true 2>/dev/null && return 0
    sleep 10
  done
  return 1
}

ieq_collect() { # <ip> <目录>
  local list; list="$(printf '%s ' "${IEQ_PATHS[@]}")"
  mkdir -p "$2/fs"
  ieq_sudo "$1" "cd / && tar -cf - --ignore-failed-read ${list} 2>/dev/null" | tar -C "$2/fs" -xf -
  ieq_sudo "$1" "ls -d /etc/systemd/*.conf.d" | while read -r dir; do
    ieq_sudo "$1" "tar -C / -cf - ${dir#/}" | tar -C "$2/fs" -xf -
  done
  ieq_ssh "$1" "dpkg-query -W" | sort > "$2/packages.txt"
  ieq_ssh "$1" "systemctl list-unit-files --state=masked --no-legend --no-pager" | awk '{print $1}' | sort > "$2/masked.txt"
}

ieq_compare() { # → 0 表示除允许差异外一致
  local a="${IEQ}/A" b="${IEQ}/B" line
  local motd="${b}/fs/etc/update-motd.d/00-custom-header"
  [ -f "${motd}" ] || { log "B 缺少 motd 头部"; return 1; }
  cp "${motd}" "${IEQ}/motd-B.orig"
  for line in "${IEQ_MOTD_EXTRA[@]}"; do
    grep -qxF -- "${line}" "${motd}" || { log "B 的 motd 缺少计划内回退行：${line}"; return 1; }
    grep -vxF -- "${line}" "${motd}" > "${motd}.tmp" || true
    cat "${motd}.tmp" > "${motd}"; rm -f "${motd}.tmp"
  done
  # Debian 13 的 /etc/default/locale 是指向 ../locale.conf 的符号链接：两者都收集，比较时不跟随链接
  diff -r --no-dereference "${a}" "${b}" > "${IEQ}/diff.txt" 2>&1
}

rc_of() { "$@" > /dev/null 2>&1 && echo 0 || echo 1; }

ieq_check_b() { # <ip>
  local enabled log starts ends skipped
  enabled="$(ieq_ssh "$1" "systemctl is-enabled xpxe-firstboot.service" 2>/dev/null || true)"
  check "B 服务已禁用" "$([ "${enabled}" = disabled ] && echo 0 || echo 1)" "is-enabled=${enabled}"
  check "B 计划与 agent 已删除" "$(rc_of ieq_sudo "$1" "test ! -e /var/lib/xpxe/plan.json && test ! -e /usr/local/sbin/xpxe-agent")" \
    "plan.json / xpxe-agent"
  log="$(ieq_sudo "$1" "cat /var/log/xpxe-firstboot.log" || true)"
  printf '%s\n' "${log}" > "${IEQ}/B-firstboot.log"
  starts="$(grep -c '^\[xpxe-agent\] ==> ' <<< "${log}" || true)"
  ends="$(grep -c '^\[xpxe-agent\] <== ' <<< "${log}" || true)"
  skipped="$(grep -c '^\[xpxe-agent\] --- .* 跳过' <<< "${log}" || true)"
  check "B 日志含 20 个模块记录" "$([ $((ends + skipped)) -eq 20 ] && [ "${starts}" -eq "${ends}" ] && echo 0 || echo 1)" \
    "开始 ${starts} 结束 ${ends} 跳过 ${skipped}"
}

ieq_wait_final() { # <秒>：等 B 的 agent.finished / agent.failed
  local deadline=$((SECONDS + $1)) path="/p/${IEQ_AGENT_TOKEN}/events"
  while [ "${SECONDS}" -lt "${deadline}" ]; do
    post_seen "${path}" lab-initeq.agent.finished && return 0
    post_seen "${path}" lab-initeq.agent.failed && return 1
    sleep 10
  done
  return 1
}

ieq_main() {
  local legacy="${REPO_DIR}/legacy/init-d13.sh" plan="" ipa=10.77.0.41 ipb=10.77.0.42 a b
  while [ $# -gt 0 ]; do
    case "$1" in --plan) plan="$2" ;; --legacy) legacy="$2" ;; *) die "未知参数 $1" ;; esac
    shift 2
  done
  begin init-equivalence
  rm -rf "${IEQ}"; mkdir -p "${IEQ}"
  ssh-keygen -q -t ed25519 -N '' -C xpxe-lab-initeq -f "${IEQ}/id_ed25519"
  ieq_image
  # --plan 给出的计划需自带实验室公钥；未给出时按 deb13-init-scripts 渲染并替换为测试公钥与口令
  [ -n "${plan}" ] && log "使用外部计划 ${plan}（须含实验室公钥与事件地址）" || plan="$(ieq_render)" || die "渲染计划失败"
  ieq_www "${legacy}" "${plan}"
  a="$(ieq_seed A "$(mac_of e1)" "${ipa}" run-legacy.sh)"
  b="$(ieq_seed B "$(mac_of e2)" "${ipb}" run-agent.sh)"
  "${LAB_DIR}/vm.sh" --name initeq-A --fw seabios --nic virtio --mac "$(mac_of e1)" --tap tap0 \
    --boot disk-first --disks 20G,1G --image "${IEQ_IMAGE}" --seed "${a}" > /dev/null
  "${LAB_DIR}/vm.sh" --name initeq-B --fw seabios --nic virtio --mac "$(mac_of e2)" --tap tap1 \
    --boot disk-first --disks 20G,1G --image "${IEQ_IMAGE}" --seed "${b}" > /dev/null
  wait_post /initeq/legacy-done rc= 3600 || true
  check "A legacy 脚本完成" "$(rc_of post_seen /initeq/legacy-done rc=0)" "/initeq/legacy-done"
  check "B agent.finished 已送达" "$(rc_of ieq_wait_final 3600)" "/p/${IEQ_AGENT_TOKEN}/events"
  sleep 30
  check "A 重启后可 SSH" "$(rc_of ieq_wait_ssh "${ipa}" 600)" "${ipa}"
  check "B 重启后可 SSH" "$(rc_of ieq_wait_ssh "${ipb}" 600)" "${ipb}"
  ieq_collect "${ipa}" "${IEQ}/A" || true
  ieq_collect "${ipb}" "${IEQ}/B" || true
  ieq_check_b "${ipb}"
  check "A/B 除允许差异外一致" "$(rc_of ieq_compare)" "差异见 ${IEQ}/diff.txt"
  cp -a "${IEQ}/diff.txt" "${IEQ}/B-firstboot.log" "${D}/" 2>/dev/null || true
  [ "${KEEP}" = 1 ] || { vm_stop "${WORK}/vm/initeq-A"; vm_stop "${WORK}/vm/initeq-B"; }
  finish init-equivalence
}
