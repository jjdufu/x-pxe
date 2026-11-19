# deploy/secrets

Compose 以绑定挂载方式把本目录下三个文件提供给容器（`/run/secrets/<名字>`）。文件不入库（`.gitignore` 已忽略，除本说明外）。

| 文件 | 容器内 | 内容 | 用途 |
| :--- | :--- | :--- | :--- |
| `master_key` | `/run/secrets/xpxe_master_key` | 32 字节随机数（原始字节） | 加密 BMC 密码、会话密钥等敏感字段；**丢失后 BMC 密码不可恢复** |
| `admin_initial_password` | `/run/secrets/xpxe_admin_initial_password` | 24 位随机可打印字符 | 首次启动创建 `admin` 的初始密码；首次登录强制改密，之后可删除该文件 |
| `bootnet_token` | `/run/secrets/xpxe_bootnet_token` | 32 字节随机数的十六进制（64 字符） | bootnet 调用控制面内部 API 的 Bearer 令牌；control 与 bootnet 读同一文件 |

## 生成

```bash
scripts/init-secrets.sh            # 默认目录 deploy/secrets；只在文件不存在时生成
scripts/init-secrets.sh /opt/x-pxe/secrets
```

脚本等价于：

```bash
umask 077
head -c 32 /dev/urandom > master_key
LC_ALL=C tr -dc 'A-Za-z0-9!#%+,.:=@^_~-' < /dev/urandom | head -c 24 > admin_initial_password
head -c 32 /dev/urandom | od -An -vtx1 | tr -d ' \n' > bootnet_token
sudo chown 10001:10001 master_key admin_initial_password bootnet_token
sudo chmod 0400 master_key admin_initial_password
sudo chmod 0440 bootnet_token
```

## 权限要求

- 属主 `10001:10001`（镜像内 `xpxe` 用户），`master_key` 与 `admin_initial_password` 权限 `0400`；控制面发现主密钥文件对组或其他用户可读时会在日志中告警。
- `bootnet_token` 权限 `0440`：bootnet 以 UID 0、组 10001 运行（Docker 对非 root 用户不设置 ambient 能力，`cap_add: NET_BIND_SERVICE` 只在 UID 0 下生效；容器仍只有这一项能力、没有 `DAC_OVERRIDE`），靠组读权限读取令牌。
- 查看初始密码：`sudo cat deploy/secrets/admin_initial_password`（只在首次登录前需要）。
- 备份时把 `master_key` 与数据卷**分开**保存；主密钥轮换见 `docs/ops.md`。
