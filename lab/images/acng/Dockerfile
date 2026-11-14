# 实验室 APT 缓存（apt-cacher-ng），供虚拟机装机与离线场景使用。
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2
RUN apt-get update \
 && apt-get install -y --no-install-recommends apt-cacher-ng ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && sed -i 's/^# *PassThroughPattern:.*/PassThroughPattern: .*/' /etc/apt-cacher-ng/acng.conf
EXPOSE 3142
CMD ["/usr/sbin/apt-cacher-ng", "-c", "/etc/apt-cacher-ng", "ForeGround=1"]
