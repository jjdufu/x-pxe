# Step 10：控制面服务（`xpxe-control`）与进程内端到端测试（`xpxe-e2e`）

## 1. 目标

把前面各 crate 装配成控制面：三个相互隔离的监听、认证与授权、装机事件入口、状态机定时器、bootnet 指令发布、后台任务、SSE 推送、WebUI 静态托管。同时建立 `xpxe-e2e`，在单进程内把控制面、bootnet、模拟目标机、模拟 BMC 串起来跑完整装机流程。

## 2. 进程结构

`xpxe-control` 与 `xpxe-bootnet` 都拆成 `lib.rs`（全部逻辑）+ `main.rs`（只解析参数并调用库），以便 e2e 在进程内启动。

```text
xpxe-control serve                      # 默认子命令
xpxe-control healthcheck                # 容器健康检查（镜像内没有 curl）
xpxe-control audit verify | export --since <ts>
xpxe-control rotate-master-key --old-file <f> --new-file <f>
xpxe-control backup --out <file>        # VACUUM INTO，在线备份
xpxe-control version                    # 版本、提交、构建时间、rustc 版本
```

| 监听 | Router | 中间件 |
| :--- | :--- | :--- |
| 管理端 `XPXE_ADMIN_LISTEN`（HTTP；配置证书时 HTTPS） | `/api/v1/*`、WebUI 静态文件（SPA 回退到 `index.html`） | 请求 ID、访问日志、会话/API Token 认证、CSRF、角色、请求体上限（JSON 1 MiB；上传走流式，按资产种类限额）、安全响应头（`Content-Security-Policy: default-src 'self'; frame-ancestors 'none'`、`X-Content-Type-Options`、`Referrer-Policy: no-referrer`；仅 HTTPS 时发送 HSTS） |
| 引导端 `XPXE_BOOT_LISTEN`（HTTP） | `/boot.ipxe`、`/boot/mac/{mac}`、`/fw/*`、`/p/{token}/*`、`/healthz` | 令牌解析、按令牌种类授权、请求体 64 KiB 上限、每 IP 限速 |
| 内部 `XPXE_INTERNAL_LISTEN` | `/internal/v1/bootnet/*` | Bearer 令牌（常量时间比较） |

三个 Router 互不挂载对方路由，引导端口上不存在任何管理 API。

## 3. 关键流程

### 3.1 arm 前置检查

`POST /nodes/{id}/arm` 依次检查，任一失败返回字段级错误：

1. 节点规格、Profile、初始化计划都通过校验。
2. `asset_channels` 中所需资产已激活：LAN 需要 `ipxe_undionly`、`ipxe_efi_x64`（或规则选择的 `snponly`）；Redfish 需要 `ipxe_efi_x64`（Redfish 节点固件必为 UEFI，见 Step 3 `NodeSpec` 约束）；所有通道都需要 `debian_netboot_amd64`。
3. Netboot 与 Profile 镜像源的 ABI 对齐结论有效（Step 7）。
4. 通道与引导网络模式匹配：LAN 节点要求模式为 `ProxyDhcp` 或 `ExternalDhcp`；Redfish 节点要求 BMC 已固定证书指纹。
5. `ProxyDhcp` 模式下 bootnet 健康且不处于 `Conflict`。

通过后在一个事务内：创建会话（固定所有版本）、生成令牌、渲染并缓存 iPXE 脚本、preseed（及其 MD5）、初始化计划（及其 SHA-256）、状态转为 `Armed`、写审计、递增指令版本号。

### 3.2 事件入口 `POST /p/{token}/events`

```json
{ "event_id": "0190f…", "kind": "installer.started", "payload": { "disks": [ … ] } }
```

- `kind` 白名单：`installer.started|finished|failed`、`agent.started|finished|failed`、`module.started|finished`（后两者只记进度，不转换状态）。安装器事件要求 `Boot` 令牌，agent 事件要求 `Agent` 令牌。
- 调用 `xpxe_domain::state::apply`；状态变更、事件记录、`EventId` 去重、审计、副作用写入发件箱（outbox 表）在同一事务提交。
- 发件箱由后台 worker 执行：发布指令（bootnet 立即收到长轮询返回）、Redfish 弹出介质、关闭会话时作废令牌与删除 ISO 缓存。执行失败按指数退避重试，结果记节点事件。

### 3.3 定时器

单个调度任务按最近的 `deadline_at` 休眠（上限 5 s），到期为对应节点投递 `DeadlineExpired { state, since }`，与人工或回调事件走同一事务路径。时钟通过 `Clock` trait 注入，测试使用可拨动的时钟。

### 3.4 `/boot/mac/{mac}`

只读：`Armed` 返回 `chain /p/{token}/boot.ipxe`，否则返回 `exit` 脚本。每次命中写一条观测；同一会话被第二个源 IP 命中时写审计告警，并在节点详情中突出显示。

### 3.5 实时推送

`GET /api/v1/events/stream`（SSE，需要会话）推送：节点状态变化、节点事件、后台任务进度（资产下载、ISO 生成、Redfish 部署）、bootnet 健康与冲突、配置新版本。每 15 s 发送注释行保活；客户端断线重连时带 `Last-Event-ID` 补发。

### 3.6 错误格式

```json
{ "error": { "code": "ARM_PRECHECK_FAILED", "message": "netboot 内核 ABI 与镜像源不一致",
             "fields": [ { "path": "/assets/debian_netboot_amd64", "code": "ABI_MISMATCH", "message": "镜像源只有 6.12.48+deb13-amd64" } ] } }
```

`fields[].path` 为 JSON Pointer，WebUI 据此把错误标到对应表单字段。

### 3.7 优雅退出

收到 SIGTERM：停止接受新连接、等待进行中的请求（≤ 10 s）、取消后台任务并清理临时上传文件、关闭数据库连接后退出。

## 4. `xpxe-e2e` 场景

全部在一个测试进程内完成：临时数据目录、随机端口、测试时钟、内存 DHCP 传输、回环 TFTP、模拟 BMC、模拟安装器与 agent（直接发 HTTP 请求）。目录形式集成测试：`tests/<场景>/main.rs + cases.rs`。

| 场景 | 断言 |
| :--- | :--- |
| `lan_uefi` | 初始管理员登录并改密 → 用 `nexus-lvm` 与 `k8s-node` 预设建配置 → 离线上传夹具资产并激活 → 登记节点 → arm → 回放 OVMF DISCOVER 得到 `fw/<sha12>/ipxe.efi` → TFTP 下载字节一致 → 回放 iPXE DISCOVER 得到会话 URL → 取脚本、preseed（第二次 410）→ `installer.started/finished` → agent 令牌取计划（Boot 令牌被拒）→ `agent.started/finished` → `Ready` → 再次回放 DISCOVER 无应答、`/boot/mac` 返回 `exit` |
| `lan_bios` | 同上，BIOS 夹具，得到 `undionly.kpxe` |
| `http_boot` | HTTP Boot 夹具得到含端口的 URL 与 Opt60 `HTTPClient` |
| `timeouts` | 每个带超时的状态拨动时钟后进入期望状态；迟到的回调被拒为 `StaleSession` |
| `redfish` | 模拟 BMC：probe 不带凭据 → trust → deploy 七步 → `installer.started` 后收到 EjectMedia；错误指纹时 deploy 失败；固件为 BIOS 的 Redfish 节点在 arm 时被拒绝 |
| `security` | 无会话 401；viewer arm 403；缺 CSRF 403；令牌跨种类、过期、会话关闭后均拒绝；登录退避；引导端口访问 `/api/v1/nodes` 返回 404 |
| `admin_transport` | 未配置证书：管理端为 HTTP，`Set-Cookie` 含 `HttpOnly`、`SameSite=Strict`，不含 `Secure`，无 HSTS 头；配置测试证书（rcgen）：同一端口为 HTTPS，Cookie 含 `Secure`，响应带 HSTS；只配置证书或只配置私钥时启动失败 |
| `versioning` | 配置并发修改 409；回滚；进行中的会话不受新版本影响 |
| `bootnet_failsafe` | 停止控制面的内部 API，拨动时钟 30 s 后 bootnet 指令表为空 |
| `audit` | 以上场景结束后 `audit verify` 通过 |

## 5. 验收

```bash
cargo test -p xpxe-control -p xpxe-e2e --all-targets --locked
# 本地冒烟（宿主机直接运行，非容器）
head -c 32 /dev/urandom > /tmp/mk; echo 'Init-Admin-Passw0rd!' > /tmp/ap; head -c 32 /dev/urandom | base64 > /tmp/bt
XPXE_BOOT_BASE_URL=http://127.0.0.1:8080 XPXE_DATA_DIR=/tmp/xd XPXE_MASTER_KEY_FILE=/tmp/mk \
XPXE_ADMIN_INITIAL_PASSWORD_FILE=/tmp/ap XPXE_BOOTNET_TOKEN_FILE=/tmp/bt XPXE_WEB_DIR=web/dist \
  cargo run -p xpxe-control -- serve &
curl -s  http://127.0.0.1:8088/api/v1/nodes  -o /dev/null -w '%{http_code}\n'      # 401
curl -s  http://127.0.0.1:8080/api/v1/nodes  -o /dev/null -w '%{http_code}\n'      # 404
curl -s  http://127.0.0.1:8080/boot/mac/52-54-00-00-00-01                          # exit 脚本
scripts/gate.sh
```

验收标准：

- 第 4 节全部场景通过。
- 冒烟三条请求结果分别为 401、404 与 `exit` 脚本。
- `scripts/gate.sh` 全绿。
