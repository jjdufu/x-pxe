#!/bin/bash
# x-pxe 初始化模块 shell_profile：Shell 环境
set -euo pipefail

cat > /etc/profile.d/zz-aliases.sh << 'XPXE_EOF'
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
XPXE_EOF
chmod 644 /etc/profile.d/zz-aliases.sh
cat > /etc/profile.d/ssh-agent.sh << 'XPXE_EOF'
if [ -z "$SSH_AGENT_PID" ]; then
    eval "$(ssh-agent -s)" > /dev/null
fi
ssh-add ~/.ssh/id_ed25519 2>/dev/null || true
XPXE_EOF
chmod 644 /etc/profile.d/ssh-agent.sh
