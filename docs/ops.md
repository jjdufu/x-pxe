# x-pxe 运维手册

本文面向在一台 Linux 宿主机上部署与维护 x-pxe 的运维人员。部署单元是一个镜像（`x-pxe:<标签>`）与 `deploy/compose.yaml` 中的两个服务：

| 服务 | 网络 | 端口 | 作用 |
| :--- | :--- | :--- | :--- |
| `control` | Compose 桥接网络 | 8088/tcp 管理端（WebUI + API）、8080/tcp 引导端（iPXE/preseed/事件）、8081/tcp 内部 API（只发布到 127.0.0.1） | 控制面：认证、配置、资产、状态机、事件、Redfish |
| `bootnet` | host | 67/udp ProxyDHCP、69/udp TFTP、4011/udp PXE | 只在 `XPXE_BOOTNET_ALLOWED_IFACES` 列出的网卡上应答；不发地址，不改现网 DHCP |

镜像 `FROM scratch`：只有三个静态二进制（`xpxe-control`、`xpxe-bootnet`、`xpxe-agent`）与 Web 静态文件，没有 shell。所有排障命令都通过 `xpxe-control` 子命令或宿主机工具完成。

## 1. 安装

前提：Linux x86_64，Docker Engine ≥ 24 与 Compose v2；宿主机有一块网卡接在 PXE 网段；内核 ≥ 5.7（更低版本见第 7 节）。

```bash
# 1) 取得镜像（离线环境用 docker load）
docker build -f deploy/Dockerfile -t x-pxe:"$(git describe --always)" .      # 或 docker load -i x-pxe.tar

# 2) 复制部署目录并填写 .env
sudo mkdir -p /opt/x-pxe && sudo cp -r deploy/. /opt/x-pxe/ && sudo cp scripts/init-secrets.sh /opt/x-pxe/
cd /opt/x-pxe
sudo cp .env.example .env
sudo vi .env        # XPXE_VERSION=镜像标签；XPXE_BOOT_BASE_URL=http://<宿主机 PXE 网段 IP>:8080；XPXE_BOOTNET_ALLOWED_IFACES=<网卡>

# 3) 生成 secret（只在文件不存在时生成；属主 10001:10001，权限 0400）
sudo ./init-secrets.sh /opt/x-pxe/secrets

# 4) 启动
sudo docker compose up -d
sudo docker compose ps            # 两个服务都应为 healthy

# 5) 首次登录
sudo cat /opt/x-pxe/secrets/admin_initial_password; echo
```

浏览器打开 `http://<宿主机>:8088`（配置证书后为 `https://`），用户名 `admin`，密码为上面输出的初始密码。首次登录强制修改密码；改密后可删除 `secrets/admin_initial_password`（之后重启不再需要它）。

登录后的最少配置顺序：

1. **资产**：获取（或离线上传）iPXE 固件与 Debian netboot，激活。
2. **装机 Profile**：从 `nexus-lvm` 预设新建，把“镜像源”改成目标机可达的 Debian 镜像（内核 ABI 必须与已激活 netboot 一致，否则 arm 被拒绝）。
3. **初始化计划**：从 `deb13-init-scripts` 预设新建，按需调整。
4. **引导网络**：选 ProxyDHCP（与现网 DHCP 共存）、复用现有 DHCP（页面给出 Kea/ISC/dnsmasq/Windows 片段）或仅 Redfish。
5. **节点**：登记 MAC、固件、通道（LAN/Redfish），arm，开机。

仅使用 Redfish 时可以只启动 control：`docker compose up -d control`。

## 2. 升级

```bash
cd /opt/x-pxe
# 1) 在线备份（SQLite 在线备份 API，不停服务）
sudo docker compose exec control xpxe-control backup --out /data/backup-$(date -u +%Y%m%dT%H%M%SZ).db
# 2) 导入新镜像，修改 .env 的 XPXE_VERSION
sudo vi .env
# 3) 重建容器；数据库迁移在启动时自动执行，失败时拒绝启动并保留原库
sudo docker compose up -d
sudo docker compose exec control xpxe-control version
```

## 3. 回退

数据库 schema 只向前迁移，**回退必须配合备份**：

```bash
sudo docker compose down
# 用升级前的备份覆盖数据卷中的数据库（数据卷名默认 x-pxe_xpxe-data）
sudo docker run --rm -v x-pxe_xpxe-data:/data -v /path/to/backup.db:/backup.db:ro \
  --entrypoint /usr/local/bin/xpxe-control x-pxe:<旧标签> version   # 仅确认旧镜像存在
# 镜像内没有 shell，复制文件请用宿主机：
sudo cp /path/to/backup.db "$(sudo docker volume inspect -f '{{.Mountpoint}}' x-pxe_xpxe-data)/xpxe.db"
sudo vi .env    # XPXE_VERSION=<旧标签>
sudo docker compose up -d
```

## 4. 备份与恢复

| 内容 | 位置 | 说明 |
| :--- | :--- | :--- |
| 数据卷 | `x-pxe_xpxe-data`（容器内 `/data`） | SQLite 数据库、资产仓、会话产物与 ISO 缓存 |
| 主密钥 | `secrets/master_key` | **与数据卷分开保存**。只有数据卷而没有主密钥时，BMC 密码不可恢复，其余数据完整 |
| `.env` 与 `bootnet_token` | 部署目录 | 可重新生成（bootnet 令牌重新生成后重启两个服务即可） |

日常备份：`docker compose exec control xpxe-control backup --out /data/backup-<时间>.db`，再把该文件与主密钥分别复制到异地。恢复：停服务 → 把备份文件放回数据卷的数据库路径 → 用原主密钥启动。

## 5. 证书（管理端 HTTPS）

管理端默认 HTTP，只应发布在受信任的管理网段（`.env` 中 `XPXE_ADMIN_PUBLISH=<管理网段 IP>:8088`）。启用 HTTPS：

```bash
sudo install -o 10001 -g 10001 -m 0400 server.crt /opt/x-pxe/secrets/tls_cert
sudo install -o 10001 -g 10001 -m 0400 server.key /opt/x-pxe/secrets/tls_key
```

在 `compose.yaml` 的 `secrets:` 中增加 `xpxe_tls_cert: { file: ./secrets/tls_cert }`、`xpxe_tls_key: { file: ./secrets/tls_key }`，并加入 `control.secrets`；`.env` 中设置：

```
XPXE_TLS_CERT_FILE=/run/secrets/xpxe_tls_cert
XPXE_TLS_KEY_FILE=/run/secrets/xpxe_tls_key
```

`docker compose up -d` 后同一端口 8088 切换为 HTTPS，会话 Cookie 带 `Secure`。引导端 8080 始终是 HTTP（iPXE 与 d-i 的取文件路径由一次性令牌与 MD5 校验保护）。

## 6. 主密钥轮换

```bash
cd /opt/x-pxe
sudo sh -c 'umask 077; head -c 32 /dev/urandom > secrets/master_key_new'
sudo chown 10001:10001 secrets/master_key_new && sudo chmod 0400 secrets/master_key_new
# 在 compose.yaml 的 secrets: 中临时增加  xpxe_master_key_new: { file: ./secrets/master_key_new }
# 并在 control.secrets 中加入 xpxe_master_key_new，然后：
sudo docker compose stop control bootnet
sudo docker compose run --rm control rotate-master-key \
  --old-file /run/secrets/xpxe_master_key --new-file /run/secrets/xpxe_master_key_new
sudo mv secrets/master_key secrets/master_key.old && sudo mv secrets/master_key_new secrets/master_key
# 去掉 compose.yaml 中临时增加的 secret
sudo docker compose up -d
```

确认 BMC 列表可正常探测后，安全销毁 `master_key.old`。

## 7. 排障

| 现象 | 检查 |
| :--- | :--- |
| 容器不健康 | `docker compose ps`；`docker compose logs control`；`curl http://<宿主机>:8080/healthz` |
| 版本与构建信息 | `docker compose exec control xpxe-control version` |
| bootnet 不应答 | WebUI“引导网络”页的状态与观测表（忽略原因：`NotArmed`、`NotAllowed`、冲突等）；`docker compose logs bootnet` |
| 现网有冲突的 DHCP/ProxyDHCP | 引导网络页“探测”；宿主机上 `xpxe-lab dhcp-scan --iface <网卡>`（开发工具，不在镜像中） |
| 抓包 | 宿主机 `tcpdump -i <网卡> -w pxe.pcap udp port 67 or udp port 68 or udp port 69 or udp port 4011`；开发机可用 `xpxe-lab capture` 解析 |
| 装机卡住 | 节点详情抽屉：事件时间轴、会话日志、截止时间；超时会进入对应失败状态并带 `DeadlineExpired` 事件 |
| arm 被拒绝 | 错误中的字段路径与代码：`ALIGNMENT_UNAVAILABLE`/ABI 不一致（镜像源与 netboot 不对齐）、`BOOTNET_UNHEALTHY`、`AGENT_MISSING` 等 |
| 控制面启动失败 `AGENT_DIGEST_MISMATCH` | 镜像内 `xpxe-agent` 与构建时记录的摘要不一致，镜像已损坏，重新导入 |
| 审计链 | `docker compose exec control xpxe-control audit verify`；导出 `audit export --since <RFC3339>` |
| 内核 < 5.7 | bootnet 绑定网卡需要 `NET_RAW`：在 `bootnet.cap_add` 中追加 `NET_RAW` |
| `docker compose exec` 报找不到 shell | 正常：镜像无 shell，只能直接执行二进制（如上） |
