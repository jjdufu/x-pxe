# syntax=docker/dockerfile:1
# x-pxe 镜像：多阶段构建，最终阶段 FROM scratch，只含静态 musl 二进制与 Web 静态文件。

FROM node:24-trixie-slim@sha256:af02d57b57719b9657a45199a79264a9437940cd55b3c7b3fc99615654f124a0 AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
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
RUN cargo build --release --locked --target x86_64-unknown-linux-musl \
      -p xpxe-control -p xpxe-bootnet \
 && mkdir -p /out/bin /out/data \
 && cp target/x86_64-unknown-linux-musl/release/xpxe-control target/x86_64-unknown-linux-musl/release/xpxe-bootnet /out/bin/

FROM scratch
COPY --from=rust /out/bin/ /usr/local/bin/
COPY --from=rust --chown=10001:10001 /out/data /data
COPY --from=web /src/web/dist/ /web/
USER 10001:10001
ENV XPXE_DATA_DIR=/data XPXE_WEB_DIR=/web
EXPOSE 8088 8080 8081
ENTRYPOINT ["/usr/local/bin/xpxe-control"]
