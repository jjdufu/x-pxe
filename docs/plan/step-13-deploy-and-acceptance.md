# Step 13：镜像、Compose 部署与验收

## 1. 目标

交付一个镜像、一份 `compose.yaml`，在一台 Linux 宿主机上 `docker compose up -d` 即可运行；用 `scripts/accept.sh` 在开发机（box）上的隔离实验室（Step 2）中跑完全部验收场景，输出一份验收报告。完成本步即 v1 发布；真实硬件验收是发布后的可选项。

## 2. 交付物

| 路径 | 内容 |
| :--- | :--- |
| `deploy/Dockerfile` | 三阶段构建：Web → Rust（musl 静态）→ `FROM scratch` |
| `deploy/compose.yaml` | `control` 与 `bootnet` 两个服务 |
| `deploy/.env.example` | 必填变量示例 |
| `deploy/compose.lab.yaml` | 实验室覆盖文件：`XPXE_BOOT_BASE_URL=http://10.77.0.1:8080`、`XPXE_BOOTNET_ALLOWED_IFACES=br-lab`、管理端只发布到 `127.0.0.1`、control 网络固定子网 `172.30.77.0/24`（离线场景按子网封禁出站） |
| `deploy/secrets/README.md` | 三个 secret 的生成方式与权限要求 |
| `scripts/init-secrets.sh` | 在宿主机生成 secret 文件（只在文件不存在时生成） |
| `scripts/accept.sh` | 验收编排：按场景名运行，输出 `docs/acceptance/<日期>.md` |
| `docs/ops.md` | 安装、升级、备份恢复、证书、主密钥轮换、排障 |

## 3. 镜像

```dockerfile
# deploy/Dockerfile（基础镜像在实现时固定到 digest）
FROM node:22-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build                                   # 产物 web/dist，全部资源同源、无 CDN

FROM rust:1.100-bookworm AS rust
RUN rustup target add x86_64-unknown-linux-musl \
 && apt-get update && apt-get install -y --no-install-recommends musl-tools
WORKDIR /src
COPY . .
RUN cargo build --release --locked --target x86_64-unknown-linux-musl \
      -p xpxe-control -p xpxe-bootnet -p xpxe-agent
RUN mkdir -p /out/data /out/etc \
 && echo 'xpxe:x:10001:10001::/data:/nonexistent' > /out/etc/passwd \
 && echo 'xpxe:x:10001:' > /out/etc/group

FROM scratch
COPY --from=rust /out/etc/ /etc/
COPY --from=rust --chown=10001:10001 /out/data /data
COPY --from=rust /src/target/x86_64-unknown-linux-musl/release/xpxe-control /usr/local/bin/
COPY --from=rust /src/target/x86_64-unknown-linux-musl/release/xpxe-bootnet /usr/local/bin/
COPY --from=rust /src/target/x86_64-unknown-linux-musl/release/xpxe-agent  /usr/local/share/xpxe/
COPY --from=web  /src/web/dist /web
USER 10001:10001
EXPOSE 8088 8080 8081
HEALTHCHECK --interval=15s --timeout=3s CMD ["/usr/local/bin/xpxe-control", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/xpxe-control"]
CMD ["serve"]
```

要点：

- 镜像里只有三个静态二进制、Web 静态文件和最小 `passwd`/`group`；没有 shell、包管理器、CA 证书包（TLS 根证书由 `webpki-roots` 编译进二进制）、时区库（时间一律 UTC 存储，界面按浏览器时区显示）。
- `xpxe-agent` 作为文件放在 `/usr/local/share/xpxe/`，由控制面通过 `/p/{token}/agent` 提供，镜像构建时计算 SHA-256 并由控制面启动时复核。
- Rust 门禁不在镜像构建中执行，由 CI 在构建前运行 `scripts/gate.sh` 与 `scripts/web-gate.sh`；镜像构建只用 `--locked` 复现同一依赖。
- 镜像标签为 `x-pxe:<git describe>`；`xpxe-control version` 输出版本、提交、构建时间与 rustc 版本。

## 4. Compose

```yaml
# deploy/compose.yaml
name: x-pxe

x-hardening: &hardening
  image: x-pxe:${XPXE_VERSION:?请在 .env 中设置}
  restart: unless-stopped
  read_only: true
  cap_drop: [ALL]
  security_opt: ["no-new-privileges:true"]
  user: "10001:10001"
  logging: { driver: json-file, options: { max-size: "20m", max-file: "5" } }

services:
  control:
    <<: *hardening
    command: ["serve"]
    ports:
      - "8088:8088"
      - "8080:8080"
      - "127.0.0.1:8081:8081"
    environment:
      XPXE_BOOT_BASE_URL: ${XPXE_BOOT_BASE_URL:?例如 http://192.168.2.3:8080}
      XPXE_OFFLINE: ${XPXE_OFFLINE:-false}
      XPXE_LOG: ${XPXE_LOG:-info}
    secrets: [xpxe_master_key, xpxe_admin_initial_password, xpxe_bootnet_token]
    volumes: ["xpxe-data:/data"]
    tmpfs: ["/tmp:size=64m,mode=1777"]

  bootnet:
    <<: *hardening
    entrypoint: ["/usr/local/bin/xpxe-bootnet"]
    command: ["run"]
    network_mode: host
    cap_add: [NET_BIND_SERVICE]          # 内核 < 5.7 时追加 NET_RAW（见 README 3.1）
    environment:
      XPXE_CONTROL_URL: http://127.0.0.1:8081
      XPXE_BOOTNET_ALLOWED_IFACES: ${XPXE_BOOTNET_ALLOWED_IFACES:?例如 ens192}
      XPXE_LOG: ${XPXE_LOG:-info}
    secrets: [xpxe_bootnet_token]
    healthcheck:
      test: ["CMD", "/usr/local/bin/xpxe-bootnet", "healthcheck"]
      interval: 15s
    depends_on:
      control: { condition: service_healthy }

secrets:
  xpxe_master_key:             { file: ./secrets/master_key }
  xpxe_admin_initial_password: { file: ./secrets/admin_initial_password }
  xpxe_bootnet_token:          { file: ./secrets/bootnet_token }

volumes:
  xpxe-data: {}
```

要点：

- `bootnet` 使用 host 网络，宿主机网络命名空间的 `ip_unprivileged_port_start` 为 1024，因此需要 `NET_BIND_SERVICE`；Docker 对非 root 容器把 `cap_add` 放进 ambient 集，验收时用 `/proc/1/status` 的 `CapEff` 确认只有 `0x400`。
- 仅使用 Redfish 时可只启动 `control`（`docker compose up -d control`）；bootnet 在“仅 Redfish”模式下也只做现网 DHCP 探测与空闲心跳。
- Compose 的文件型 secret 以绑定挂载提供，宿主机文件需要容器用户可读：`scripts/init-secrets.sh` 生成后执行 `chown 10001:10001` 与 `chmod 0400`。三个 secret：`master_key`（32 字节随机）、`admin_initial_password`（24 位随机可打印字符，首次登录后即可删除文件）、`bootnet_token`（32 字节随机，十六进制）。
- 数据卷 `/data` 内是 SQLite 数据库与资产仓；主密钥不在数据卷中。
- 管理端默认是 HTTP。需要 HTTPS 时把证书与私钥作为 secret 挂入并设置 `XPXE_TLS_CERT_FILE`/`XPXE_TLS_KEY_FILE`（同一端口切换为 HTTPS）；未配置时管理端口只应发布在受信任的管理网段。

## 5. 运维要点（写入 `docs/ops.md`）

| 主题 | 内容 |
| :--- | :--- |
| 安装 | 复制 `deploy/` → 填 `.env` → `scripts/init-secrets.sh` → `docker compose up -d` → 浏览器访问 `http://<host>:8088` 登录并改密（配置证书时为 `https://`） |
| 升级 | `xpxe-control backup --out /data/backup-<时间>.db`（SQLite 在线备份 API）→ 改 `XPXE_VERSION` → `docker compose up -d`；迁移在启动时自动执行，失败则拒绝启动并保留原库 |
| 回退 | 镜像回退到上一标签并恢复备份；数据库 schema 只向前迁移，回退必须配合备份 |
| 备份 | 数据卷 + `secrets/master_key` 分开保存；只有数据卷没有主密钥时，BMC 密码不可恢复，其余数据完整 |
| 主密钥轮换 | 在 compose 中临时增加 secret `xpxe_master_key_new` 后执行 `docker compose run --rm control rotate-master-key --old-file /run/secrets/xpxe_master_key --new-file /run/secrets/xpxe_master_key_new`，再把新文件替换为 `master_key` |
| 排障 | `xpxe-lab dhcp-scan`、`capture` 可在宿主机直接运行；节点详情的事件与日志页；`/healthz` |

## 6. 验收场景

### 6.1 实验室验收（v1 发布条件，全部在 box 上执行）

`scripts/accept.sh <场景…>|lab` 的流程：`lab/setup.sh` 与 `lab/net.sh up`（Step 2）→ `docker compose -f deploy/compose.yaml -f deploy/compose.lab.yaml up -d` 部署被测镜像 → 经管理 API 完成初始化（改密、上传或获取资产、导入实验室 Profile 与 `deb13-init-scripts` 计划）→ 依次运行场景。每个场景输出 PASS/FAIL、耗时与日志目录（串口日志、`br-lab` 抓包、控制面事件导出），汇总为 `docs/acceptance/<日期>.md`。装机类场景每台虚拟机 2 vCPU / 2 GiB，默认 `-j 2`。

| 场景 | 步骤 | 通过条件 |
| :--- | :--- | :--- |
| A `lan-bios` | ProxyDHCP 模式，`lab-dhcp`（`coexist`）分配地址；SeaBIOS + virtio-net 与 SeaBIOS + e1000 各一台，登记并 arm | 自动装完 Debian 13 → firstboot 执行 `deb13-init-scripts` 计划 → `Ready`；目标机上 `dpkg-query` 包清单与计划一致；全程无人工输入 |
| B `lan-uefi` | 同 A，OVMF + e1000e 与 OVMF + vmxnet3 | 同 A；抓包可见 4011 REQUEST/ACK 与 TFTP `ipxe.efi` |
| C `nic-matrix` | Step 2 矩阵 B1–B6、U1–U6 在被测产品上重跑（只到 d-i 取走 preseed，随后关闭虚拟机并以 `Reset` 事件让节点回到 `Idle`） | 全部与 Step 2 结论一致 |
| D `http-boot` | `ovmf-http`（只剩 HTTP Boot 启动项）× virtio-net、e1000e | 经 `HTTPClient` 应答下载 `ipxe.efi` 并装完 |
| E `existing-dhcp` | 引导网络切到“复用现有 DHCP”；`lab-dhcp` 使用从产品 Web/API 取得的 Kea 片段生成的配置；SeaBIOS virtio-net 与 OVMF e1000e 各一台 | bootnet 不发任何 DHCP 报文（抓包确认）；两台均装完 |
| F `redfish` | 仅 Redfish 模式；sushy-tools + libvirt OVMF 域；探测 → 核对并确认指纹 → 部署 | UEFI 节点装完，`installer.started` 后介质被弹出；登记“BIOS + Redfish”节点被拒绝（`redfish_requires_uefi`）；更换 sushy 证书后部署被拒并告警 |
| G `static-ip` | 停止 `lab-dhcp`，Profile 设静态地址，Redfish 部署 | iPXE 与 d-i 都不发 DHCP（抓包确认）；装完后 `ip addr` 与配置一致 |
| H `no-reinstall` | A、B 完成后把虚拟机引导顺序设为网络优先（`--boot net-first`）并重启 3 次 | 每次都从本地盘启动；bootnet 观测记录忽略原因 `NotArmed`；装机后写入的标记文件仍在 |
| I `disk-select` | 标准布局（20 GiB 系统盘、60 GiB 数据盘、1 GiB USB 可移动“BMC 虚拟 U 盘”），规则“最小且 ≥ 16 GiB、排除可移动盘”；另一轮把两块 virtio 盘都设为 20 GiB 且规则设为 `Unique` | 第一轮装到 `SYS0001` 盘，USB 盘与数据盘未被改写（分区表哈希不变）；第二轮进入 `Failed`，节点详情展示三块盘的清单与排除原因 |
| J `timeouts` | 超时调到最短：arm 后不开机；安装中途断开 tap（`ip link set tapN down`） | 分别因 `arm_ttl`、`install_ttl` 进入对应状态，事件中有 `DeadlineExpired` |
| K `restart-resume` | 安装进行中重启 `control`；另测停止 `control` 30 s 以上 | 重启后状态与截止时间不变并继续完成；控制面不可达超过 30 s 时 bootnet 停止应答（fail closed），恢复后继续 |
| L `rogue-dhcp` | ProxyDHCP 模式下启动 `lab-rogue`（下发 `next-server`/`filename`） | 引导网络页出现冲突告警；未确认前拒绝切换到 ProxyDHCP；确认动作写审计 |
| M `offline` | 预热 APT 缓存后切到 `Offlinemode`；`XPXE_OFFLINE=true`；在 `DOCKER-USER` 链对 `172.30.77.0/24` 到 `10.77.0.0/24` 以外的新连接 `REJECT` 并计数 | 官方获取入口禁用，上传资产可激活；B 场景在该条件下通过；REJECT 计数为 0（产品没有尝试出站） |
| N `abi-mismatch` | 上传一个与 APT 缓存中 `kernel-image-*-di` ABI 不一致的 netboot 夹具 | arm 被拒绝，错误指明 ABI 与镜像源 |
| O `assets-rollback` | 激活一个新 iPXE 版本并在金丝雀节点上失败（上传截断文件之外的“可通过格式检查但无法引导”的夹具） → 一键回滚 | 回滚后新 arm 的节点使用回滚目标版本，`fw/<sha12>/` 路径随之变化，进行中的会话不受影响 |
| P `security` | 端口扫描 box；容器检查；未认证请求 | 对外只有管理端 8088（实验室为回环）、8080、67/69/4011 udp，8081 只在回环；`docker inspect` 显示两个容器只读根文件系统、`cap_drop ALL`，bootnet `/proc/1/status` 的 `CapEff` 为 `0x400`；`docker run --entrypoint /bin/sh` 失败（镜像无 shell）；数据卷中检索不到主密钥与 BMC 明文密码；未登录访问 `/api/v1/*` 返回 401；HTTP 管理端的 `Set-Cookie` 含 `HttpOnly`、`SameSite=Strict`；挂入测试证书后同一端口为 HTTPS 且 Cookie 含 `Secure`；重复取 preseed 返回 410；`xpxe-control audit verify` 通过 |
| Q `webui` | `scripts/web-gate.sh` 与 Step 12 联调脚本对被测镜像运行 | 全部通过 |
| R `gates` | 在发布提交上运行全部门禁 | `scripts/gate.sh`、`scripts/web-gate.sh` 全绿；`cargo deny check` 无违规；镜像大小记录在报告中 |

### 6.2 真实硬件验收（可选，不阻塞 v1）

实验室无法复现厂商 PXE ROM 与 UNDI 驱动的差异（Step 2 第 2.1 节）。有现场设备时运行 `scripts/accept.sh hardware --inventory lab/hardware.toml`（记录每台机器的 MAC、BMC 地址与固件模式），对每个机型执行 A、B、F、H 的同等流程，结果写入 `docs/lab/compat-matrix.md`，失败项附抓包。该结果用于后续版本修正兼容性，不作为 v1 发布条件。

## 7. 验收

```bash
docker build -f deploy/Dockerfile -t x-pxe:$(git describe --always) .
scripts/accept.sh lab                                     # A–R，全部在 box 上执行
lab/net.sh down                                           # 恢复 box 网络
```

验收标准：

- A–R 全部 PASS。
- `docs/acceptance/<日期>.md` 提交到仓库，包含镜像 digest、各场景日志路径与门禁输出。
- `docs/ops.md` 中的安装步骤由未参与开发的人员照做一次并成功登录。
