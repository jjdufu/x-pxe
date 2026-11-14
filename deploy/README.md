# deploy

镜像与编排：`Dockerfile`（多阶段构建，最终阶段 `FROM scratch`，只含静态 musl 二进制 `xpxe-control`、`xpxe-bootnet` 与 Web 静态文件）、`compose.yaml`、`secrets/*.example`。Step 13 定稿。
