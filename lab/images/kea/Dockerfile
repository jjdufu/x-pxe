# 实验室“现网 DHCP”与伪造 DHCP 替身，不属于产品。
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
RUN apt-get update \
 && apt-get install -y --no-install-recommends kea-dhcp4-server iproute2 \
 && rm -rf /var/lib/apt/lists/* \
 && mkdir -p /run/kea /var/lib/kea
# 容器启动时 eth0 尚未接入（由 lab/dhcp.sh 移入 veth），先等网卡出现再启动 Kea。
CMD ["sh", "-c", "for i in $(seq 1 100); do ip -4 addr show eth0 2>/dev/null | grep -q inet && break; sleep 0.1; done; exec kea-dhcp4 -c /etc/kea/kea-dhcp4.conf"]
