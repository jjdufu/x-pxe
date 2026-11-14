#!/bin/bash
# Debian 13 (trixie) 服务器初始化优化脚本（面向 K8s 节点）
# 相对原版：幂等 drop-in、修 VSOCK 控制台刷屏、sudoers.d、不换源等

set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "请以 root 运行此脚本" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# 可选说明（保留原注释意图）
# useradd -m -s /sbin/nologin -U n9e
# ---------------------------------------------------------------------------

# 解决 useradd 默认 SHELL=/bin/sh 无 home 习惯问题
sed -i 's#SHELL=/bin/sh#SHELL=/bin/bash#g' /etc/default/useradd

# ---------------------------------------------------------------------------
# 修复控制台 systemd-ssh-generator AF_VSOCK 报错
# 裸机无 vsock 时 generator 查询本地 CID 失败会刷屏；用 /dev/null 覆盖即可
# ---------------------------------------------------------------------------
mkdir -p /etc/systemd/system-generators
ln -sfn /dev/null /etc/systemd/system-generators/systemd-ssh-generator
# 若存在相关 socket 单元则屏蔽（保留常规 ssh.service）
systemctl mask sshd-unix-local.socket 2>/dev/null || true
systemctl mask sshd-vsock.socket 2>/dev/null || true

# ---------------------------------------------------------------------------
# SSH drop-in（幂等：整文件覆盖，勿 >>）
# ---------------------------------------------------------------------------
mkdir -p /etc/ssh/sshd_config.d
cat > /etc/ssh/sshd_config.d/jwczjzl.conf << 'EOF'
PermitRootLogin no
X11Forwarding no
X11UseLocalhost yes
AllowTcpForwarding yes
AllowAgentForwarding yes
IgnoreRhosts yes
GSSAPIAuthentication no
UseDNS no
Compression no
TCPKeepAlive yes
PasswordAuthentication no
Banner /etc/issue
EOF

# 主配置里关闭 X11（与 drop-in 一致）
sed -i 's/^#\?X11Forwarding.*/X11Forwarding no/' /etc/ssh/sshd_config 2>/dev/null || true

# ---------------------------------------------------------------------------
# 用户与 SSH 公钥
# ---------------------------------------------------------------------------
SSH_PUBKEY='ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBtCscA5CqkUDclClV2dKSGH4JxR2vLulpTgLR34Pz1l jay@localhost'

mkdir -p /root/.ssh
chmod 700 /root/.ssh
if ! grep -qxF "$SSH_PUBKEY" /root/.ssh/authorized_keys 2>/dev/null; then
  echo "$SSH_PUBKEY" >> /root/.ssh/authorized_keys
fi
chmod 600 /root/.ssh/authorized_keys

if ! id -u jwczjzl &>/dev/null; then
  useradd -m -p "$(openssl passwd -6 "${JWCZJZL_PASSWORD:?redacted}")" jwczjzl
fi

# 实际加入 sudo 组（原脚本仅注释）
usermod -aG sudo jwczjzl

mkdir -p /home/jwczjzl/.ssh
chmod 700 /home/jwczjzl/.ssh
# 从 root 复制密钥目录后去重写入公钥，避免同一 key 重复两行
if [[ ! -f /home/jwczjzl/.ssh/authorized_keys ]]; then
  if [[ -f /root/.ssh/authorized_keys ]]; then
    cp /root/.ssh/authorized_keys /home/jwczjzl/.ssh/authorized_keys
  else
    touch /home/jwczjzl/.ssh/authorized_keys
  fi
fi
if ! grep -qxF "$SSH_PUBKEY" /home/jwczjzl/.ssh/authorized_keys 2>/dev/null; then
  echo "$SSH_PUBKEY" >> /home/jwczjzl/.ssh/authorized_keys
fi
chmod 600 /home/jwczjzl/.ssh/authorized_keys
chown -R jwczjzl:jwczjzl /home/jwczjzl/.ssh

loginctl enable-linger jwczjzl

# ---------------------------------------------------------------------------
# 时区 / locale / timesyncd（不换 apt 源；timesyncd 用 drop-in）
# ---------------------------------------------------------------------------
timedatectl set-timezone "Asia/Shanghai"
timedatectl set-local-rtc 0
echo "Asia/Shanghai" > /etc/timezone
ln -sf /usr/share/zoneinfo/Asia/Shanghai /etc/localtime

mkdir -p /etc/systemd/timesyncd.conf.d
cat > /etc/systemd/timesyncd.conf.d/ntp.conf << 'EOF'
[Time]
NTP=ntp.aliyun.com
EOF

cat > /etc/default/locale << 'EOF'
LANG="en_US.UTF-8"
LC_ALL="en_US.UTF-8"
LANGUAGE="en_US.UTF-8"
EOF

# locale.gen：先取消注释，再按需追加，避免重复生成
touch /etc/locale.gen
sed -i 's/^# *zh_CN.UTF-8 UTF-8/zh_CN.UTF-8 UTF-8/' /etc/locale.gen
sed -i 's/^# *en_US.UTF-8 UTF-8/en_US.UTF-8 UTF-8/' /etc/locale.gen
grep -qxF 'zh_CN.UTF-8 UTF-8' /etc/locale.gen || echo 'zh_CN.UTF-8 UTF-8' >> /etc/locale.gen
grep -qxF 'en_US.UTF-8 UTF-8' /etc/locale.gen || echo 'en_US.UTF-8 UTF-8' >> /etc/locale.gen
locale-gen
update-locale LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 LANGUAGE=en_US.UTF-8

# systemd 启停超时：5s 过短，改为 30s（drop-in，勿 >> system.conf）
mkdir -p /etc/systemd/system.conf.d
cat > /etc/systemd/system.conf.d/timeout.conf << 'EOF'
[Manager]
DefaultTimeoutStartSec=30s
DefaultTimeoutStopSec=30s
EOF

# ---------------------------------------------------------------------------
# shell 别名 / PATH / ssh-agent（profile.d，幂等；agent 用引号 heredoc）
# ---------------------------------------------------------------------------
cat > /etc/profile.d/zz-aliases.sh << 'EOF'
unset MAILCHECK

if [ -x /usr/bin/dircolors ]; then
    test -r ~/.dircolors && eval "$(dircolors -b ~/.dircolors)" || eval "$(dircolors -b)"
    alias ls='ls --color=auto'
    alias grep='grep --color=auto'
    alias fgrep='fgrep --color=auto'
    alias egrep='egrep --color=auto'
fi
alias ll='ls -alF'
alias la='ls -A'
alias l='ls -CF'

export PATH="$PATH:/usr/local/sbin"
EOF
chmod 644 /etc/profile.d/zz-aliases.sh

# 引号 EOF：避免脚本写入时展开 $SSH_AGENT_PID / $(ssh-agent)
cat > /etc/profile.d/ssh-agent.sh << 'EOF'
if [ -z "$SSH_AGENT_PID" ]; then
    eval "$(ssh-agent -s)" > /dev/null
fi
ssh-add ~/.ssh/id_ed25519 2>/dev/null || true
EOF
chmod 644 /etc/profile.d/ssh-agent.sh

# ---------------------------------------------------------------------------
# APT：不换网络镜像；仅处理 DVD 安装残留的 cdrom（否则 apt-get update 必失败）
# ---------------------------------------------------------------------------
# 经典 sources.list：注释掉 deb cdrom: 行
if [[ -f /etc/apt/sources.list ]]; then
  sed -i -E 's/^[[:space:]]*(deb(-src)?[[:space:]]+cdrom:)/# \1/' /etc/apt/sources.list
fi
shopt -s nullglob
for f in /etc/apt/sources.list.d/*.list; do
  sed -i -E 's/^[[:space:]]*(deb(-src)?[[:space:]]+cdrom:)/# \1/' "$f"
done
# deb822 *.sources：整段 URI 指向 cdrom 的文件先禁用（不改其它源内容）
for f in /etc/apt/sources.list.d/*.sources; do
  if grep -qiE '^URIs:[[:space:]]*cdrom:' "$f"; then
    mv -f "$f" "${f}.disabled-cdrom"
  fi
done
# DVD 安装常见：网络源被注释、只剩 cdrom。仅取消注释已有 http(s) 行，不写入新镜像地址
if [[ -f /etc/apt/sources.list ]]; then
  sed -i -E 's/^#[[:space:]]*(deb(-src)?[[:space:]]+https?:)/\1/' /etc/apt/sources.list
fi
for f in /etc/apt/sources.list.d/*.list; do
  sed -i -E 's/^#[[:space:]]*(deb(-src)?[[:space:]]+https?:)/\1/' "$f"
done
shopt -u nullglob

# DVD 装机常见：禁用 cdrom 后没有任何网络源。此时写入 Debian 官方源（deb.debian.org），
# 不写入第三方镜像；若已有 http(s)/deb822 网络源则不动。
has_net_src=0
if grep -RqhE '^[[:space:]]*deb(-src)?[[:space:]]+https?:' /etc/apt/sources.list /etc/apt/sources.list.d 2>/dev/null; then
  has_net_src=1
fi
shopt -s nullglob
for f in /etc/apt/sources.list.d/*.sources; do
  if grep -qiE '^URIs:[[:space:]]*https?:' "$f"; then
    has_net_src=1
    break
  fi
done
shopt -u nullglob

if [[ "$has_net_src" -eq 0 ]]; then
  echo "未检测到网络 APT 源（DVD 装机仅 cdrom 时常见），写入 Debian 官方 deb822 源…"
  cat > /etc/apt/sources.list.d/debian.sources << 'EOF'
Types: deb
URIs: https://deb.debian.org/debian
Suites: trixie trixie-updates trixie-backports
Components: main contrib non-free non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg

Types: deb
URIs: https://deb.debian.org/debian-security
Suites: trixie-security
Components: main contrib non-free non-free-firmware
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
EOF
  # 避免空的 sources.list 干扰；保留文件但确保无有效 cdrom
  if [[ -f /etc/apt/sources.list ]] && ! grep -qE '^[[:space:]]*deb' /etc/apt/sources.list; then
    : # 已无有效 deb 行，可留空
  fi
fi

apt-get autoclean -y || true
apt-get update

# libaio1 → libaio1t64；libncurses5 过时 → libncurses6；
# libpam-cracklib → libpam-pwquality；lrkhunter → rkhunter
# 浏览器/Electron 相关库保留（无头 Chromium/Playwright）
PKGS=(
  python3-defusedxml
  gh
  libreoffice-writer-nogui
  poppler-utils
  fonts-noto-cjk
  fonts-wqy-microhei
  xdg-utils
  libnspr4
  libnss3
  libatk1.0-0t64
  libatk-bridge2.0-0t64
  libatspi2.0-0t64
  libcups2t64
  libgbm1
  libasound2t64
  libxkbcommon0
  libcairo2
  libpango-1.0-0
  libxcomposite1
  libxdamage1
  libxfixes3
  libxrandr2
  fonts-liberation
  fonts-noto-color-emoji
  libvulkan1
  pkg-config
  xfsprogs
  ifenslave
  gpg
  strace
  libssl-dev
  ncdu
  gcc
  ripgrep
  powertop
  dnsutils
  zip
  g++
  rkhunter
  clamav
  make
  rsyslog
  curl
  sudo
  parted
  xz-utils
  liblzma-dev
  rsync
  vim
  git
  wget
  dos2unix
  net-tools
  tree
  nload
  sysstat
  psmisc
  iotop
  bash-completion
  fail2ban
  jq
  nmon
  nfs-common
  rpcbind
  libpam-pwquality
  dialog
  man-db
  cron
  iputils-ping
  expect
  socat
  tcptraceroute
  fping
  hping3
  p7zip-full
  btop
  iftop
  libaio1t64
  libncurses6
  tmux
)

apt-get install -y "${PKGS[@]}"

# 可选：# apt-get install zram-tools -y

# ---------------------------------------------------------------------------
# 服务：关 unattended 定时器；开 SSH；mask 不需要的单元
# ---------------------------------------------------------------------------
systemctl disable --now apt-daily.timer apt-daily-upgrade.timer

systemctl unmask ssh 2>/dev/null || true
systemctl enable --now ssh

systemctl mask cryptdisks-early.service
systemctl mask cryptdisks.service
systemctl mask x11-common.service
# 故意 mask nftables；fail2ban 仅安装不配置
systemctl mask nftables.service

systemctl daemon-reload

update-alternatives --set editor /usr/bin/vim.basic

# 可选 chrony（保留注释）
# grep -E -q "^pool" /etc/chrony/chrony.conf | sed -i 's/^pool/# pool/g' /etc/chrony/chrony.conf
# cat << EOF > /etc/chrony/conf.d/jwczjzl.conf
# pool 192.168.2.2 iburst maxsources 1
# EOF
# systemctl enable chrony.service && systemctl restart chrony.service

# ---------------------------------------------------------------------------
# MOTD / Banner
# ---------------------------------------------------------------------------
: > /etc/issue
: > /etc/issue.net
: > /etc/motd
chmod -x /etc/update-motd.d/* 2>/dev/null || true

# Banner 已在 sshd drop-in；若主配置仍无则补一行（幂等）
if ! grep -qiE '^\s*Banner\s+' /etc/ssh/sshd_config; then
  echo 'Banner /etc/issue' >> /etc/ssh/sshd_config
else
  sed -ri 's/^\s*(banner|Banner)\s+.*/Banner \/etc\/issue/' /etc/ssh/sshd_config
fi

tee /etc/update-motd.d/00-custom-header << 'EOF'
#!/bin/bash

# Get load averages
LOAD1=$(awk '{print $1}' /proc/loadavg)
LOAD5=$(awk '{print $2}' /proc/loadavg)
LOAD15=$(awk '{print $3}' /proc/loadavg)
# Get free memory
MEMORY_USED=$(free -t -m | awk '/Mem/{print $3}')
MEMORY_ALL=$(free -t -m | awk '/Mem/{print $2}')
MEMORY_PERCENTAGE=$(free | awk '/Mem/{printf("%.2f%%\n", $3/$2*100)}')
# Get system uptime
UPTIME=$(awk '{print int($1)}' /proc/uptime)
UPTIME_DAYS=$((UPTIME/60/60/24))
UPTIME_HOURS=$((UPTIME/60/60%24))
UPTIME_MINS=$((UPTIME/60%60))
UPTIME_SECS=$((UPTIME%60))
# Get processes
PROCESS=$(ps -eo user=|sort|uniq -c | awk '{print $2 " " $1 }')
PROCESS_ALL=$(echo "${PROCESS}" | awk '{print $2}' | awk '{SUM += $1} END {print SUM}')
PROCESS_ROOT=$(echo "${PROCESS}" | awk '$1=="root"{print $2}')
PROCESS_USER=$(echo "${PROCESS}" | awk '$1!="root"{SUM += $2} END {print SUM+0}')
# Get processors
PROCESSOR_NAME=$(grep "model name" /proc/cpuinfo | cut -d ' ' -f3- | awk '{print $0}' | head -1)
PROCESSOR_COUNT=$(grep -c '^processor' /proc/cpuinfo)
# Colors
G="\033[01;32m"
R="\033[01;31m"
D="\033[39m\033[2m"
N="\033[0m"
echo -e "${G}Login success.${N} Please execute the commands and operation data carefully."
echo -e "You are logged in to ${G}$(uname -n)${N}, Login time is $(/bin/date "+%Y-%m-%d %H:%M:%S").\n"
echo -e "[System Info]\n"
echo -e "  SYSTEM    : $(head -1 /etc/os-release | awk -F'[="]+' '{print $2}')"
echo -e "  KERNEL    : $(uname -sr)"
echo -e "  UPTIME    : ${UPTIME_DAYS} days ${UPTIME_HOURS} hours ${UPTIME_MINS} minutes ${UPTIME_SECS} seconds"
echo -e "  CPU       : ${PROCESSOR_NAME} (${G}${PROCESSOR_COUNT}${N} vCPU)\n"
echo -e "  MEMORY    : ${MEMORY_USED} MB / ${MEMORY_ALL} MB (${G}${MEMORY_PERCENTAGE}${N} Used)"
echo -e "  LOAD AVG  : ${G}${LOAD1}${N} (1m), ${G}${LOAD5}${N} (5m), ${G}${LOAD15}${N} (15m)"
echo -e "  PROCESSES : ${G}${PROCESS_ROOT}${N} (root), ${G}${PROCESS_USER}${N} (user), ${G}${PROCESS_ALL}${N} (total)"
echo -e "  USERS     : ${G}$(users | wc -w)${N} users logged in"
echo -e "  BASH      : ${G}${BASH_VERSION}${N}\n"
echo -e "[Disk Usage]\n"
mapfile -t DFH < <(df -h -x zfs -x squashfs -x tmpfs -x devtmpfs -x overlay --output=target,pcent,size,used | tail -n+2)
for LINE in "${DFH[@]}"; do
    DISK_USAGE=$(echo "${LINE}" | awk '{print $2}' | sed 's/%//')
    USAGE_WIDTH=$((DISK_USAGE*60/100))
    if [ "${DISK_USAGE}" -gt 90 ]; then
        COLOR="${R}"
    else
        COLOR="${G}"
    fi
    BAR="[${COLOR}"
    for ((i=0; i<USAGE_WIDTH; i++)); do
        BAR+="="
    done
    BAR+=${D}
    for ((i=USAGE_WIDTH; i<60; i++)); do
        BAR+="="
    done
    BAR+="${N}]"
    echo "${LINE}" | awk '{ printf("Mounted: %-32s %s / %s (%s Used)\n", $1, $4, $3, $2); }' | sed -e 's/^/  /'
    echo -e "${BAR}" | sed -e 's/^/  /'
done
echo
EOF
chmod +x /etc/update-motd.d/00-custom-header

# ---------------------------------------------------------------------------
# 内核参数（勿清空 /etc/sysctl.conf；仅写 drop-in）
# ---------------------------------------------------------------------------
cat > /etc/sysctl.d/99-fleet-netvm.conf << 'EOF'

# ------ 转发 / IPv6 ------
# K8s 节点需要转发；纯 nginx 开着也无妨
net.ipv4.ip_forward = 1
# 确认集群无依赖 IPv6 后再禁用；若有双栈 Service 不要用这三项
net.ipv6.conf.all.disable_ipv6 = 1
net.ipv6.conf.default.disable_ipv6 = 1
net.ipv6.conf.lo.disable_ipv6 = 1

# ------ TCP ------
net.ipv4.tcp_syncookies = 1
net.ipv4.tcp_tw_reuse = 1
net.ipv4.tcp_fin_timeout = 30
net.ipv4.tcp_syn_retries = 5
net.ipv4.tcp_synack_retries = 5
net.ipv4.tcp_fastopen = 3
net.ipv4.tcp_slow_start_after_idle = 0

# 空闲 10 分钟后探活；配合 intvl/probes，死连接收得比 7200 快（适合 K8s/反代）
net.ipv4.tcp_keepalive_time = 600
net.ipv4.tcp_keepalive_intvl = 30
net.ipv4.tcp_keepalive_probes = 5

# 缓冲：default 保持克制，max 允许大流量抬升（勿再把 default 调到数 MB）
net.ipv4.tcp_rmem = 4096 87380 12582912
net.ipv4.tcp_wmem = 4096 16384 12582912
net.core.rmem_default = 212992
net.core.wmem_default = 16384
net.core.rmem_max = 12582912
net.core.wmem_max = 12582912

net.ipv4.tcp_max_orphans = 262144
net.ipv4.tcp_max_syn_backlog = 32768
net.ipv4.tcp_max_tw_buckets = 1048576
net.ipv4.ip_local_port_range = 10000 65535

# 拥塞：维持发行版常见组合即可；公网入口若要试 BBR，再单独开下面两行并回归测试
# net.ipv4.tcp_congestion_control = bbr
# net.core.default_qdisc = fq
# 当前建议保持（若系统已是 cubic + fq_codel，不必强改）
# net.ipv4.tcp_congestion_control = cubic
# net.core.default_qdisc = fq_codel

# ------ 队列 ------
net.core.netdev_max_backlog = 32768
net.core.somaxconn = 32768

# ------ 内存 / VM（ES/Java/K8s 友好）------
vm.max_map_count = 262144
vm.dirty_background_ratio = 5
vm.dirty_ratio = 10
vm.overcommit_memory = 1
vm.swappiness = 10
vm.min_free_kbytes = 65536
vm.vfs_cache_pressure = 50

# ------ 文件监控（kubelet / 同步类）------
fs.fanotify.max_queued_events = 65536
fs.inotify.max_user_watches = 524288

# ------ 安全（与 Calico/K8s 兼容）------
net.ipv4.conf.all.accept_redirects = 0
net.ipv4.conf.default.accept_redirects = 0
net.ipv4.conf.all.secure_redirects = 0
net.ipv4.conf.default.secure_redirects = 0
# 严格 rp_filter=1 易在覆盖网络/不对称路径上丢包；全员用 0（或改成 2 loose）
net.ipv4.conf.all.rp_filter = 0
net.ipv4.conf.default.rp_filter = 0

EOF

sysctl --system || true

# limits：整文件覆盖 drop-in，可重入
cat > /etc/security/limits.d/90-nofile.conf << 'EOF'
* soft nofile 655350
* hard nofile 655350
* soft nproc  unlimited
* hard nproc  unlimited
* soft core   unlimited
* hard core   unlimited
# End of file
EOF

# ---------------------------------------------------------------------------
# vimrc
# ---------------------------------------------------------------------------
cat > /root/.vimrc << 'EOF'
let mapleader = ','
let g:mapleader = ','

syntax on

set history=2000

filetype on
filetype plugin on
filetype indent on

set nocompatible
set autoread
set shortmess=atI

set magic
set title
set nobackup

set novisualbell
set noerrorbells
set visualbell t_vb=
set t_vb=
set tm=500

set cursorcolumn
set cursorline

set scrolloff=7

set ruler
set number
set nowrap
set showcmd
set showmode
set showmatch
set matchtime=2

set hlsearch
set incsearch
set ignorecase
set smartcase

set expandtab
set smarttab
set shiftround

set autoindent smartindent shiftround
set shiftwidth=4
set tabstop=4
set softtabstop=4

set foldenable
set foldmethod=indent
set foldlevel=99
let g:FoldMethod = 0
map <leader>zz :call ToggleFold()<cr>
fun! ToggleFold()
    if g:FoldMethod == 0
        exe "normal! zM"
        let g:FoldMethod = 1
    else
        exe "normal! zR"
        let g:FoldMethod = 0
    endif
endfun

set encoding=utf-8
set fileencodings=ucs-bom,utf-8,cp936,gb18030,big5,euc-jp,euc-kr,latin1
set termencoding=utf-8
set ffs=unix,dos,mac
set formatoptions+=m
set formatoptions+=B

set selection=inclusive
set selectmode=mouse,key

set completeopt=longest,menu
set wildmenu
set wildmode=longest,list,full
set wildignore=*.o,*~,*.pyc,*.class

set backspace=indent,eol,start
set whichwrap+=<,>,h,l

if has("autocmd")
  au BufReadPost * if line("'\"") > 1 && line("'\"") <= line("$") | exe "normal! g'\"" | endif
endif

set background=dark
colorscheme desert

hi! link SignColumn   LineNr
hi! link ShowMarksHLl DiffAdd
hi! link ShowMarksHLu DiffChange

set statusline=%<%f\ %h%m%r%=%k[%{(&fenc==\"\")?&enc:&fenc}%{(&bomb?\",BOM\":\"\")}]\ %-14.(%l,%c%V%)\ %P
set laststatus=2

autocmd FileType python set tabstop=4 shiftwidth=4 expandtab ai
autocmd FileType ruby set tabstop=2 shiftwidth=2 softtabstop=2 expandtab ai
autocmd BufRead,BufNew *.md,*.mkd,*.markdown  set filetype=markdown.mkd

autocmd BufNewFile *.sh,*.py exec ":call AutoSetFileHead()"
function! AutoSetFileHead()
    if &filetype == 'sh'
        call setline(1, "\#!/bin/bash")
    endif
    if &filetype == 'python'
        call setline(1, "\#!/usr/bin/env python")
        call append(1, "\# encoding: utf-8")
    endif
    normal G
    normal o
    normal o
endfunc

autocmd FileType c,cpp,java,go,php,javascript,puppet,python,rust,twig,xml,yml,perl autocmd BufWritePre <buffer> :call <SID>StripTrailingWhitespaces()
fun! <SID>StripTrailingWhitespaces()
    let l = line(".")
    let c = col(".")
    %s/\s\+$//e
    call cursor(l, c)
endfun

nnoremap k gk
nnoremap gk k
nnoremap j gj
nnoremap gj j

map <C-j> <C-W>j
map <C-k> <C-W>k
map <C-h> <C-W>h
map <C-l> <C-W>l

nnoremap <F2> :set nu! nu?<CR>
nnoremap <F3> :set list! list?<CR>
nnoremap <F4> :set wrap! wrap?<CR>
set pastetoggle=<F5>
au InsertLeave * set nopaste
nnoremap <F6> :exec exists('syntax_on') ? 'syn off' : 'syn on'<CR>

inoremap kj <Esc>

nnoremap <leader>q :q<CR>
nnoremap <leader>w :w<CR>

map <Leader>sa ggVG

nnoremap U <C-r>

nnoremap ' `
nnoremap ` '

nnoremap <silent> n nzz
nnoremap <silent> N Nzz
nnoremap <silent> * *zz
nnoremap <silent> # #zz
nnoremap <silent> g* g*zz

noremap <silent><leader>/ :nohls<CR>

vnoremap < <gv
vnoremap > >gv

map Y y$

nnoremap ; :

nnoremap H ^
nnoremap L $

cmap w!! w !sudo tee >/dev/null %

cnoremap <C-j> <t_kd>
cnoremap <C-k> <t_ku>
cnoremap <C-a> <Home>
cnoremap <C-e> <End>
EOF

cp /root/.vimrc /home/jwczjzl/.vimrc
chown jwczjzl:jwczjzl /home/jwczjzl/.vimrc

# ---------------------------------------------------------------------------
# sudoers：绝不整文件覆盖 /etc/sudoers；只用 sudoers.d + visudo 校验
# 去掉 requiretty（破坏 Ansible/非交互）；保留 Debian 默认 %sudo ALL
# ---------------------------------------------------------------------------
cp -a /etc/sudoers /etc/sudoers.ori 2>/dev/null || true

cat > /etc/sudoers.d/jwczjzl << 'EOF'
# 环境与审计（模块化；不替换整份 /etc/sudoers）
Defaults    env_reset
Defaults    env_keep += "HOME"
Defaults    env_delete = "HTTP_PROXY HTTPS_PROXY"
Defaults    passwd_tries=3
Defaults    timestamp_timeout=10
Defaults    secure_path="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
Defaults    use_pty
Defaults    logfile="/var/log/sudo.log"

# 常用运维命令免密；完整管理仍走 Debian 默认 %sudo ALL=(ALL:ALL) ALL（需密码）
Cmnd_Alias SYSADMIN = /usr/sbin/service, /usr/bin/systemctl, /usr/bin/apt-get, /usr/bin/apt, /usr/sbin/reboot, /usr/sbin/shutdown
%sudo   ALL=(ALL:ALL) NOPASSWD: SYSADMIN
jwczjzl ALL=(ALL:ALL) ALL
EOF
chmod 0440 /etc/sudoers.d/jwczjzl
visudo -cf /etc/sudoers.d/jwczjzl
visudo -cf /etc/sudoers

cat > /etc/logrotate.d/sudo << 'EOF'
/var/log/sudo.log {
    weekly
    rotate 4
    compress
    missingok
    create 640 root root
}
EOF

# debug 日志轮转
#logrotate -d /etc/logrotate.d/sudo
# 强制日志轮转，即便是没到时间
#logrotate -f /etc/logrotate.d/sudo

#  --- ⬇fstrim.service 作用⬇ ---
# 检测 RAID 阵列是否支持 TRIM，非零则支持
# lsblk -d -o NAME,DISC-GRAN,DISC-MAX
# 优化 SSD 性能：TRIM 命令标记已删除数据块，减少 SSD 写操作的擦除开销，提升写速度。
# 延长 SSD 寿命：TRIM 优化空闲块管理，减少不必要的擦除和写入，延长 SSD 闪存寿命。
# 回收空间：fstrim 标记未使用的数据块，允许支持 TRIM 的设备重新利用这些空间。
# 支持某些 RAID 配置：在支持 TRIM 的现代 RAID 控制器（如 SAS RAID 5）上，fstrim 优化阵列性能。
#  --- ⬆fstrim.service 作用⬆ ---

#  --- ⬇如果需要可以安装xanmod内核支持 BBRv3⬇ ---
# 先检测
# eval "awk -f <(curl -s https://dl.xanmod.org/check_x86-64_psabi.sh)"
# 再安装
# wget -qO - https://dl.xanmod.org/archive.key | gpg --dearmor -vo /etc/apt/keyrings/xanmod-archive-keyring.gpg
# echo 'deb [signed-by=/etc/apt/keyrings/xanmod-archive-keyring.gpg] http://deb.xanmod.org releases main' | tee /etc/apt/sources.list.d/xanmod-release.list
# apt update && apt install --no-install-recommends clang lld llvm libelf-dev linux-xanmod-x64v3
#  --- ⬆如果需要可以安装xanmod内核支持 BBRv3⬆ ---

# ---------------------------------------------------------------------------
# 命令历史审计（只写一次；大 HISTFILESIZE；目录先建再 +a，不对不存在文件 chattr -R）
# ---------------------------------------------------------------------------
mkdir -vp /var/log/.history
chmod 1777 /var/log/.history
# 不对目录 chattr +a（否则新建 HISTFILE 别扭）；仅对每个历史文件 +a

cat > /etc/profile.d/history-record.sh << 'EOF'
# 审计：每次登录独立 HISTFILE；HISTSIZE/HISTFILESIZE 统一为大容量
export HISTSIZE=100000
export HISTFILESIZE=100000

LOGTIME=$(date +%Y%m%d-%H-%M-%S)
export HISTFILE="/var/log/.history/${USER}.${LOGTIME}.history"
if [ ! -f "${HISTFILE}" ]; then
  touch "${HISTFILE}"
fi
chmod 600 "${HISTFILE}"
chattr +a "${HISTFILE}" 2>/dev/null || true

HISTTIMEFORMAT="%F_%T $(whoami)#$(who -u am i 2>/dev/null| awk '{print $NF}'|sed -e 's/[()]//g'):"
export HISTTIMEFORMAT
EOF
chmod 644 /etc/profile.d/history-record.sh

# ---------------------------------------------------------------------------
# SSH 配置校验并重载
# ---------------------------------------------------------------------------
sshd -t
systemctl reload ssh 2>/dev/null || systemctl restart ssh

# ---------------------------------------------------------------------------
# Clean
# ---------------------------------------------------------------------------
find / -maxdepth 3 -name hosts.equiv -print0 2>/dev/null | xargs -0r rm -rf
find / -maxdepth 3 -name .netrc -print0 2>/dev/null | xargs -0r rm -rf
find / -maxdepth 3 -name .rhosts -print0 2>/dev/null | xargs -0r rm -rf
apt-get clean
apt-get autoclean -y
apt-get autoremove -y
find /var/log/apt/ -type f -delete 2>/dev/null || true
find /var/log -name "vmware-*.*.log" -o -name "*.log-*" -o -name "*.gz" -o -name "*log.*" 2>/dev/null | while read -r f; do rm -f "$f"; done || true
find /var/log -type f -name "*log" -exec truncate -s 0 {} \; 2>/dev/null || true

echo "初始化完成，即将 reboot ..."
sync
find /tmp -mindepth 1 -delete 2>/dev/null || true
reboot
