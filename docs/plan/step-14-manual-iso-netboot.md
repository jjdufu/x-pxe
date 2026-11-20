# Step 14：手动 ISO 网络引导（WebUI 上传 ISO + 可编辑 iPXE 引导菜单）

## 1. 目标

在不改变“节点状态机是唯一装机事实来源”的前提下，补一条**手动装机**通道：管理员在 WebUI 上传任意操作系统安装 ISO，编辑 iPXE 引导菜单条目，目标机经现有 PXE/HTTP Boot 链路进入菜单后手动选择条目，从网络启动该 ISO。典型用途是 Proxmox VE、其他发行版或厂商工具盘的临时安装与救援。

验收以 **Proxmox VE 9.2 安装 ISO** 在实验室虚拟机上经网络引导进入图形/文本安装界面为准。**v1 发版条件同时包含本步验证通过**（Step 13 的验收全绿之后，还需本步实验室验收通过才能打 tag 发布）。

## 2. 范围

### 2.1 范围内

| 能力 | 说明 |
| :--- | :--- |
| ISO 上传 | WebUI 分块上传（断点续传），服务端流式计算 SHA-256，按内容寻址存入 `/data/assets/iso/<sha256>`；可选填写期望 SHA-256，不一致即拒收 |
| ISO 识别 | 纯 Rust 读取 ISO9660/Joliet/Rock Ridge 目录与 El Torito 引导目录（复用 Step 9 `xpxe_image::readback` 并扩展），识别卷标、是否 hybrid（含 MBR/GPT）、大小、已知发行版特征（`/boot/linux26` + `/boot/initrd.img` 为 PVE 系，`/casper`、`/live`、`/install.amd` 等） |
| 引导菜单 | 一个全局“手动引导菜单”配置对象（版本链、可回滚、审计），条目有序，每条为类型化定义；WebUI 拖拽排序、启用/停用、默认项与倒计时 |
| 条目类型 | `sanboot`（整盘 ISO 经 HTTP 映射）、`kernel_initrd`（从 ISO 抽取内核与 initrd，按需把 ISO 打包进 initrd）、`chain`（链到另一 iPXE 脚本 URL，仅允许同源或白名单）、`exit`（本地启动） |
| 抽取 | Rust 从 ISO 中抽取内核与 initrd，追加一个 newc cpio 段把整份 ISO 放进 initrd（PVE 风格安装器在 initrd 中寻找 ISO）；产物内容寻址缓存，ISO 删除时级联清理 |
| 菜单入口 | 未武装（不在状态机 `Armed`）的机器默认仍返回 `exit`；只有管理员显式开启“手动引导菜单”且 MAC 命中白名单（或开启“对所有未知机器显示”）时，`/boot/mac/{mac}` 返回菜单脚本 |
| 审计 | ISO 上传/删除、菜单版本变更、菜单被某 MAC 命中、条目被选择（iPXE 回调）全部审计 |

### 2.2 范围外

- 手动装机**不进入节点状态机**：没有 `Armed → Installing → Ready` 转换、没有超时、没有 agent 初始化；菜单命中与条目选择只记观测与审计。
- 不做 ISO 内容修改（不注入 preseed/kickstart/answer file）；自动化应答留给 v2。
- 不做 Redfish 虚拟介质挂载整份 ISO（BMC 直挂大 ISO 的兼容性差异大，v2 再评估）。
- 不支持 BIOS 下的 `kernel_initrd` 大于 iPXE 内存上限的组合（见第 6 节风险，WebUI 给出提示）。

## 3. 设计

### 3.1 数据模型

| 表 / 配置 | 字段 | 说明 |
| :--- | :--- | :--- |
| `iso_images` | id, sha256, size, label, volume_id, hybrid, detected_family, uploaded_by, uploaded_at, deleted_at | ISO 元数据；文件内容寻址 |
| `iso_extracts` | iso_sha256, kind（`kernel`/`initrd`/`initrd_with_iso`）, sha256, size, source_path | 抽取产物缓存 |
| 配置 `boot_menu`（`ConfigDocument`） | enabled, show_unknown, allow_macs[], default_entry, timeout_s, entries[] | 版本链、乐观锁、回滚，同 Step 6 |
| `BootMenuEntry` | id, title, enabled, kind（`Sanboot{iso}` / `KernelInitrd{iso, kernel_path, initrd_path, cmdline, embed_iso}` / `Chain{url}` / `Exit`）, arch（`any`/`bios`/`uefi`） | 类型化；`cmdline` 只允许白名单字符并限长，服务端渲染时转义 |

### 3.2 引导链路

```text
目标机 PXE/HTTP Boot → bootnet（未武装：照常不应答或按现网 DHCP）
  → 现网 DHCP 指向的 iPXE / 或已在 iPXE 中 → GET /boot/mac/{mac}
     ├─ Armed：chain /p/{token}/boot.ipxe（状态机通道，不变）
     ├─ 菜单开启且 MAC 命中：返回 /m/menu.ipxe（手动通道）
     └─ 其他：exit
/m/menu.ipxe → 用户选择条目
     ├─ sanboot：sanboot --no-describe http://<boot_base>/m/iso/<sha256>.iso
     ├─ kernel_initrd：kernel /m/x/<kernel_sha> <cmdline> ； initrd /m/x/<initrd_sha> ； boot
     ├─ chain：chain <url>
     └─ exit
```

- `/m/*` 挂在引导端口，只读、无令牌（菜单与 ISO 本身不含机密），但受“菜单开启”总开关与每 IP 限速约束；菜单关闭时 `/m/*` 全部返回 404。
- ISO 与抽取产物支持 HTTP Range（iPXE `sanboot` 依赖 Range 读取），复用 Step 10 `served.rs` 的流式单段 Range 实现。
- 菜单脚本由 `xpxe-render` 新模板 `templates/manual-menu.ipxe.j2` 渲染：`menu`/`item`/`choose --timeout`，条目标题经 iPXE 字符串转义；选择后先 `imgfetch` 一个回调 URL 记录观测再执行。

### 3.3 sanboot 与 kernel/initrd 的取舍

| ISO 类型 | 推荐条目 | 理由 |
| :--- | :--- | :--- |
| 小型或 hybrid ISO（救援盘、厂商工具、≤ 1 GiB） | `sanboot` | 固件把 HTTP 映射成只读块设备，ISO 原样引导；无需理解 ISO 内部结构 |
| PVE 系（`/boot/linux26` + `/boot/initrd.img`，安装器在 initrd 阶段查找 ISO） | `kernel_initrd` + `embed_iso = true` | sanboot 后安装器在 Linux 中看不到 iSCSI/HTTP 块设备，找不到安装介质；把 ISO 打进 initrd 后安装器在内存中找到它 |
| 发行版 live/netinst（`/casper`、`/live`、`/install.amd`） | `kernel_initrd`（`fetch=`/`url=` 等参数由管理员在 cmdline 中填写） | 由发行版自身从 HTTP 拉取 squashfs |

WebUI 根据 ISO 识别结果预填推荐类型与路径，管理员可改。

### 3.4 抽取与 initrd 打包（纯 Rust）

1. 读取 ISO 目录定位 `kernel_path`、`initrd_path`（Rock Ridge 长名优先，回落 Joliet/ISO9660）。
2. 内核原样输出；initrd 原样输出为 `initrd` 产物。
3. `embed_iso = true` 时生成 `initrd_with_iso`：原 initrd 字节 + 4 字节对齐填充 + 一个 newc cpio 段，内含 `/proxmox.iso`（文件名按发行版特征选择，PVE 为根目录下任意 `*.iso`，默认 `/proxmox.iso`）与 `TRAILER!!!`。压缩的原 initrd 与未压缩 cpio 段串联是内核支持的格式。
4. 产物流式写入临时文件后计算 SHA-256 并原子改名，避免整份 ISO 读入内存；同一 `(iso, 路径, embed)` 只生成一次。
5. 新增 `xpxe_image::cpio`（newc 写入器）与 `xpxe_image::iso_read`（目录遍历、按路径读取文件、流式拷贝），保持零外部依赖。

### 3.5 WebUI

- 资产页新增“ISO 镜像”标签：`FileDrop` 分块上传 + `Progress`、表格（卷标、大小、SHA-256、识别结果、被哪些条目引用）、删除前 `AlertDialog` 列出引用条目。
- 新增“手动引导菜单”页：总开关、MAC 白名单（`Combobox` 多选标签）、默认项、倒计时 `NumberField`；条目列表可拖拽排序（键盘 ↑/↓ 等价操作）、`Switch` 启停、编辑 `Drawer`（类型 `RadioCards`、ISO `Select`、路径 `Combobox`（候选来自 ISO 目录）、cmdline `Textarea` 带字符白名单校验）；右侧 `CodeBlock` 实时预览渲染出的 `menu.ipxe`。
- 风格、四态、键盘与离线要求同 Step 11/12；保存走版本链，显示差异并可回滚。

### 3.6 安全

- 菜单默认关闭；开启、修改白名单、选择“对所有未知机器显示”均为 `AlertDialog` 确认并审计。
- `chain` 条目 URL 只允许 `boot_base` 同源或管理员配置的白名单前缀，渲染时校验。
- 上传只允许 admin；单文件上限默认 16 GiB（可配置），磁盘剩余低于阈值时拒收并提示。
- `/m/*` 不返回任何会话令牌或配置内容；状态机通道（`/p/{token}`）与手动通道完全隔离。

## 4. 交付物

| 交付物 | 位置 |
| :--- | :--- |
| ISO 读取与 cpio 写入 | `crates/xpxe-image/src/{iso_read,cpio}.rs` + 测试（小型夹具 ISO 由测试内生成） |
| 菜单领域类型与校验 | `crates/xpxe-domain/src/manual.rs`（`BootMenu`、`BootMenuEntry`、cmdline 白名单） |
| 菜单模板与渲染 | `crates/xpxe-render/templates/manual-menu.ipxe.j2`、`xpxe_render::manual` + 黄金快照 |
| 存储 | `crates/xpxe-store/migrations/000N_iso.sql`、`iso_images`/`iso_extracts` 句柄 |
| 控制面 | 上传（分块、断点续传）、ISO 列表/删除、抽取任务、菜单配置 API、引导端 `/m/*`、`/boot/mac` 分流 |
| WebUI | ISO 标签页、手动引导菜单页、e2e（Playwright）用例 |
| e2e | `crates/xpxe-e2e/tests/manual_menu/`：菜单关闭时 `/m/*` 404；开启后命中 MAC 得到菜单；sanboot/kernel_initrd 条目渲染；Range 读取；Armed 节点仍走状态机通道 |
| 实验室 | `lab/run.sh manual-sanboot`、`lab/run.sh manual-pve`（见第 5 节） |

## 5. 验收

```bash
cargo test -p xpxe-image -p xpxe-render -p xpxe-control -p xpxe-e2e --all-targets --locked
scripts/gate.sh
scripts/web-gate.sh
lab/run.sh manual-sanboot     # 小型 hybrid ISO（如 iPXE 自身的 ipxe.iso 或 memtest）经 sanboot 启动，串口出现预期横幅
lab/run.sh manual-pve         # PVE 9.2 ISO：kernel_initrd + embed_iso，UEFI 与 BIOS 各一台虚拟机进入安装界面
```

验收标准：

- 上述命令全部通过。
- `manual-pve`：Proxmox VE 9.2 官方 ISO（实现时记录其 SHA-256）上传后识别为 PVE 系并预填 `/boot/linux26`、`/boot/initrd.img`、`embed_iso = true`；OVMF 与 SeaBIOS 虚拟机各一台经网络引导进入 PVE 安装界面（串口/截图判定），安装器能识别安装介质（日志中无 “no cdrom found”）。
- `manual-sanboot`：hybrid ISO 经 `sanboot` 在 OVMF 与 SeaBIOS 下均可启动。
- 菜单关闭时，同一台机器 `/boot/mac` 返回 `exit`；菜单开启但 MAC 未命中时返回 `exit`；Armed 节点始终走状态机通道。
- 审计链包含上传、菜单版本变更、命中与选择记录，`audit verify` 通过。
- **发布条件**：Step 13 验收与本步验收都通过后才允许打 tag、推送镜像与发布。

## 6. 风险

| 风险 | 影响 | 对策 |
| :--- | :--- | :--- |
| `initrd_with_iso` 体积大（PVE ISO 约 1.5 GiB） | BIOS 下 iPXE 可用内存不足或下载慢 | UEFI 优先；BIOS 下 WebUI 提示内存需求；HTTP 下载走 Range 与长连接；实验室虚拟机分配 ≥ 4 GiB 内存 |
| 固件 sanboot 兼容性差异 | 部分 UEFI 实现不支持 HTTP 块设备 | 条目可切换为 `kernel_initrd`；兼容性结果写入 `docs/lab/compat-matrix.md` |
| PVE 安装器查找 ISO 的约定随版本变化 | 9.x 后续版本可能改路径或文件名 | 文件名与路径可在条目中配置；实验室固定 9.2 ISO 并记录 SHA-256；识别规则以特征文件为准而非版本号 |
| 大文件上传中断 | 上传失败、磁盘残留 | 分块 + 断点续传，未完成分块按时限清理；上传前检查磁盘余量 |
| 菜单被未授权机器使用 | 误装或信息暴露 | 默认关闭、MAC 白名单、`/m/*` 无机密、全部审计；与状态机通道隔离 |
| ISO 解析器面对恶意或损坏 ISO | 越界读取、无限循环 | 纯安全 Rust、目录深度与条目数上限、循环检测、模糊测试夹具；解析失败只标记“未识别”，不影响其他功能 |
| 磁盘占用 | `/data` 写满影响装机主链路 | ISO 与抽取产物单独配额，超额拒收；删除 ISO 级联清理抽取产物 |

## 7. 实现偏差（落地记录）

| 项目 | 落地情况 | 原因 / 影响 |
| :--- | :--- | :--- |
| 模板位置 | `crates/xpxe-render/src/templates/manual-menu.ipxe.j2`（与其他模板同目录），黄金快照 `tests/render/golden/manual-menu.ipxe` | 沿用 xpxe-render 现有模板目录约定，方案写的 `templates/` 不存在 |
| 磁盘余量检查 | 以 ISO 配额代替“磁盘剩余低于阈值”：`XPXE_ISO_MAX_BYTES`（单文件，默认 16 GiB）与 `XPXE_ISO_QUOTA_BYTES`（ISO + 抽取产物总量，默认 64 GiB），开始上传时按“已用 + 本次”判定，单文件超限 413 `TOO_LARGE`、总量超限 507 `QUOTA_EXCEEDED` | 工作区禁止 `unsafe`/libc，标准库没有 statvfs；配额同样保证 `/data` 不被 ISO 写满（风险表“磁盘占用”的对策） |
| `/m/*` 限速 | 带 `Range` 头的 `/m/` 请求不计入每 IP 限速，其余（菜单脚本、整份下载、回调）照常计数 | sanboot 按块读取 ISO，一次启动就有成千上万个 Range 请求，计数会把目标机自己限住 |
| 抽取时机 | 菜单保存后与控制面启动时在后台抽取（`spawn_prepare`），条目在产物就绪前渲染为“不可用：extract pending/running”，`GET /boot-menu/status` 可查询 | PVE 的 initrd+ISO 约 1.7 GiB，同步抽取会阻塞保存请求 |
| 删除仍被引用的 ISO | 允许（admin、AlertDialog 列出引用条目），条目随即渲染为“不可用：iso deleted”，抽取产物级联删除 | 方案只要求删除前列出引用；不阻止删除，避免菜单与 ISO 互相锁死 |
| 路径 `Combobox` | 编辑抽屉中内核/initrd 路径用原生 `<input list>` + `<datalist>`（候选来自 `/isos/{sha}/tree` 的文件），可自由输入 | 组件库只有多选 `MultiCombobox`；datalist 原生可键盘操作且离线 |
| 保存差异与回滚 | 保存走 `SingletonSave`（变更说明 + 版本链）；差异与回滚在“配置历史”页（新增 `boot_menu` 类型标签） | 与其他单例配置一致，不在菜单页重复实现 |
| 菜单入口链路 | 按 3.2 节：bootnet 对未武装 MAC 照常不应答，手动通道依赖现网 DHCP 指向 `/boot.ipxe`（`external_dhcp` + 产品 Kea 片段）；ProxyDHCP 模式下未武装机器不会进入菜单 | 与方案一致，在此明确：实验室场景用 `external_dhcp` |
| 实验室判定（验收中发现并修复） | `manual-pve` 只在内核 `Run /init` 之后的串口输出里找安装器标志；`lab/run.sh manual-*` 结束时把现网 DHCP 替身恢复为 `coexist` | iPXE 菜单标题 “Proxmox VE 9.2” 会让原判定在内核启动前就误判通过；manual 运行留下的 product 配置会让随后的 `scripts/accept.sh` 进入 proxy_dhcp 时报冲突 |
