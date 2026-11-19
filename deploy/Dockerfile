# syntax=docker/dockerfile:1
# x-pxe 镜像（Step 13 定稿）：Web → Rust（musl 静态）→ FROM scratch。
# 最终镜像只含三个静态二进制、Web 静态文件与最小 passwd/group；没有 shell、包管理器、CA 证书包与时区库。
# 构建：docker build -f deploy/Dockerfile \
#         --build-arg XPXE_GIT_DESCRIBE="$(git describe --always --dirty)" \
#         --build-arg XPXE_GIT_COMMIT="$(git rev-parse HEAD)" \
#         --build-arg XPXE_BUILD_TIME="$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
#         -t x-pxe:"$(git describe --always)" .

FROM node:24-trixie-slim@sha256:af02d57b57719b9657a45199a79264a9437940cd55b3c7b3fc99615654f124a0 AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
# 产物 web/dist：全部资源同源、无 CDN（scan-dist 在 web-gate 中检查）
RUN npm run build

FROM rust:1.100.0-slim-trixie@sha256:e9d95b74296f7126a83ae3d041aeaba1ff4c478f13abc5cc034fcf7c131c3d4a AS rust
RUN apt-get update \
 && apt-get install -y --no-install-recommends musl-tools \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY rust-toolchain.toml ./
RUN rustup show active-toolchain
COPY Cargo.toml Cargo.lock clippy.toml rustfmt.toml README.md ./
COPY crates/ crates/
ARG XPXE_GIT_DESCRIBE=unknown
ARG XPXE_GIT_COMMIT=unknown
ARG XPXE_BUILD_TIME=unknown
# 门禁（gate.sh / web-gate.sh）由 CI 在构建前运行；这里只用 --locked 复现同一依赖
RUN export XPXE_GIT_DESCRIBE XPXE_GIT_COMMIT XPXE_BUILD_TIME XPXE_RUSTC="$(rustc --version)" \
 && cargo build --release --locked --target x86_64-unknown-linux-musl \
      -p xpxe-control -p xpxe-bootnet -p xpxe-agent \
 && R=target/x86_64-unknown-linux-musl/release \
 && mkdir -p /out/bin /out/share /out/data /out/etc \
 && cp "$R/xpxe-control" "$R/xpxe-bootnet" /out/bin/ \
 && cp "$R/xpxe-agent" /out/share/ \
 && (cd /out/share && sha256sum xpxe-agent > xpxe-agent.sha256) \
 && echo 'xpxe:x:10001:10001::/data:/nonexistent' > /out/etc/passwd \
 && echo 'xpxe:x:10001:' > /out/etc/group

FROM scratch
ARG XPXE_GIT_DESCRIBE=unknown
ARG XPXE_GIT_COMMIT=unknown
LABEL org.opencontainers.image.title="x-pxe" \
      org.opencontainers.image.source="https://github.com/jjdufu/x-pxe" \
      org.opencontainers.image.version="${XPXE_GIT_DESCRIBE}" \
      org.opencontainers.image.revision="${XPXE_GIT_COMMIT}"
COPY --from=rust /out/etc/ /etc/
COPY --from=rust --chown=10001:10001 /out/data /data
COPY --from=rust /out/bin/ /usr/local/bin/
# agent 由控制面经 /p/{token}/agent 下发；启动时按 xpxe-agent.sha256 复核
COPY --from=rust /out/share/ /usr/local/share/xpxe/
COPY --from=web /src/web/dist/ /web/
USER 10001:10001
ENV XPXE_DATA_DIR=/data XPXE_WEB_DIR=/web
EXPOSE 8088 8080 8081
HEALTHCHECK --interval=15s --timeout=3s --start-period=20s CMD ["/usr/local/bin/xpxe-control", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/xpxe-control"]
CMD ["serve"]
