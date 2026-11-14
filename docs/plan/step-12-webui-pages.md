# Step 12：WebUI 业务页面与联调

## 1. 目标

在 Step 11 的基座上实现全部业务页面，接入 Step 10 的 API 与 SSE。每个页面都：只用 Step 11 组件库；覆盖四态；键盘可完整操作；375 px 宽度下页面无横向溢出；后台刷新不丢表单输入和滚动位置。下表“复用”列指向 [`webui-ref/exhtml/`](../webui-ref/exhtml/) 中的出处。

## 2. 导航结构

侧栏分组（参考 `index.html` `#sidebarNav` 与 `.nav-group-label`）：

| 分组 | 页面 | 路由 |
| :--- | :--- | :--- |
| 运行 | 总览、节点 | `/`、`/nodes` |
| 配置 | 装机 Profile、初始化计划、引导网络、资产 | `/profiles`、`/init-plans`、`/boot-network`、`/assets` |
| 追溯 | 配置历史、审计日志 | `/history`、`/audit` |
| 系统 | 设置 | `/settings` |

登录页 `/login` 不使用外壳。命令面板（`⌘K`）可跳转到任一页面、按主机名或 MAC 搜索节点、对选中节点执行 arm、打开 Theme Editor（参考 `app.js` `renderCmdList` 的“条目 + 右侧徽章/kbd”样式）。

## 3. 页面规格

### 3.1 登录 `/login`

| 项 | 规格 |
| :--- | :--- |
| 布局 | 居中 `.ui-card`（宽 380 px），品牌标记同侧栏 `.brand` |
| 组件 | `Input`（用户名、密码，密码框带显示切换 `.input-action-btn`）、主按钮、`Alert` 错误 |
| 行为 | 失败显示统一错误；退避期间按钮禁用并显示剩余秒数（`tabular`）；`must_change_password` 时登录后立即弹出不可关闭的 `Dialog` 修改密码 |
| 四态 | Loading：按钮内旋转图标；Error：`ui-alert-danger` |

### 3.2 总览 `/`

| 区块 | 组件与复用 |
| :--- | :--- |
| 节点状态计数（Idle/Armed/进行中/Ready/Failed） | `.grid-4` + `.ui-card` + `Badge`，数字 `tabular`，点击跳转带筛选的节点页 |
| 引导网络健康 | `.ui-card`：模式、网卡、指令版本、最近心跳；冲突时 `ui-alert-danger` 并链接到引导网络页 |
| 资产就绪 | 每个资产种类一行：激活版本、阶段 `Badge`、Netboot ABI 对齐结论 |
| 近 30 天装机结果 | `Tracker`（参考 `components.js` tremor 区块 `.tracker-bar`，`trackerDays` 数据形状：日期、状态、比率） |
| 最近事件 | `Timeline`（参考 `.timeline-list`，最近 20 条节点事件与配置变更） |

### 3.3 节点 `/nodes`

| 项 | 规格 |
| :--- | :--- |
| 表格 | `DataTable`（参考 `#sec-table`）：选择、主机名、MAC（`mono tabular`）、通道（LAN/Redfish `Badge`）、固件（BIOS/UEFI）、Profile、初始化计划、状态（`Badge` + `.badge-dot`，颜色：Armed=info、进行中=warn、Ready=success、Failed=danger）、截止倒计时（`tabular`，每秒本地递减，不触发重取）、最近事件、操作 `Menu` |
| 工具栏 | 搜索（主机名/MAC/IP/BMC 地址）、状态与通道 `Select` 筛选、列显隐、`批量 arm (N)`/`批量 disarm (N)`（N = 可见且选中行数）、`+ 新建节点`（`Dialog`）、`从观测导入`（列出 bootnet 看到的未登记 MAC） |
| 右键菜单 | `ContextMenu`（参考 `#customContextMenu`）：arm、disarm、重新装机、Redfish 部署、弹出介质、复制 MAC、打开详情 |
| 危险操作 | `AlertDialog`：对 `Ready` 节点重新装机需输入主机名才可确认（与领域层 `Rearm` 确认串一致）；Redfish 部署提示“将强制重启目标服务器”；批量操作列出受影响主机名 |
| 详情抽屉 | `Drawer`（参考 `#demoDrawer`）内 `Tabs`：**概览**（`Steps` 显示 Armed→Installing→Installed→Configuring→Ready，当前步骤与截止时间，参考 `.steps-bar`）；**事件**（`Timeline`）；**日志**（`CodeBlock`：d-i 失败上报的选盘清单、agent 日志尾部，带复制）；**规格**（表单，保存产生 `node_spec` 新版本）；**BMC**（地址、用户名、密码只写不读、`probe` 显示证书指纹/主题/有效期、`trust` 需 `AlertDialog` 核对指纹）；**会话**（固定的 Profile/计划/资产版本号，令牌只显示末 4 位） |
| 实时 | SSE 更新行状态；选择集、搜索词、排序、滚动位置、打开的抽屉都保持 |
| 四态 | Loading 骨架行；Empty：“还没有节点”+“新建节点”/“从观测导入”；筛选无结果：“重置筛选”；Error：重试 |

### 3.4 装机 Profile `/profiles`、`/profiles/:id`

| 项 | 规格 |
| :--- | :--- |
| 列表 | `DataTable`：名称、镜像源、分区方案、被多少节点引用、头版本、更新人、更新时间；`+ 新建`（从预设 `nexus-lvm`/`official-mirror` 或空白） |
| 编辑布局 | ≥ 1024 px 两栏：左表单、右侧 sticky 预览；< 1024 px 上下堆叠，预览折叠为 `Accordion` |
| 表单分组（`Accordion`） | 区域（Locale、键盘、时区：`Combobox` 可搜索）；网络与控制台；镜像源与 APT（协议 `Segmented`、主机端口、目录、代理、security 来源 `RadioCards`、套件与组件 `Checkbox`、额外仓库列表与公钥指纹显示）；账户（root `RadioCards`：禁用/设置口令；口令输入后只显示“已设置”；主用户）；选盘规则（匹配条件列表、容量 `RangeSlider`、排除型号 `Combobox` 标签、偏好 `RadioCards`）；分区（`RadioCards` + 自定义分区行编辑表格）；高级（firmware cpio、d-i 调试级别） |
| 预览 | `Tabs`：`boot.ipxe`、内核参数、`preseed.cfg`、选盘脚本；“预览上下文”`Select` 选一个样例节点（DHCP/静态）；输入停止 300 ms 后调用 `POST …/preview`；更新时保留各 Tab 的滚动位置；`CopyButton` |
| 校验 | 后端 `fields[].path` 映射到字段下方 `.ui-error-msg`，并在顶部 `Alert` 汇总可点击跳转 |
| 保存栏 | 底部 sticky：头版本号、“未保存”`Badge`、`差异`（`Dialog` 内 `DiffView`：字段差异 + 渲染差异）、`保存`（必填变更说明）；版本冲突 409 时展示对方修改的差异并提供“基于最新版本重做” |

### 3.5 初始化计划 `/init-plans`、`/init-plans/:id`

| 项 | 规格 |
| :--- | :--- |
| 列表 | 同 Profile；新建可选预设 `k8s-node`、`minimal` |
| 编辑布局 | ≥ 1024 px 三栏：左模块列表、中模块表单、右预览；< 1024 px 模块列表变为顶部 `Select`，预览折叠 |
| 模块列表 | 20 个模块按执行顺序排列，样式同侧栏 `.nav-item`（序号用 `.nav-num` 风格）+ `Switch` 启用开关 + 校验状态点；表单由 `GET /init-modules` 的字段 schema 驱动 |
| 字段控件 | 布尔→`Switch`；有范围的整数→`NumberField`（显示范围与默认值，`Tooltip` 解释用途）；枚举→`RadioCards`（≤ 4 项）或 `Select`；列表→可增删行；包名/用户组/单元名→`Combobox` 标签；SSH 公钥→`Textarea` 每行一把，逐行校验并显示指纹 |
| 软件包模块 | 8 个分组 `Checkbox`，每组显示包数 `Badge`，可展开查看清单；额外与排除列表；底部显示最终安装包数 |
| sysctl 模块 | 预设 `RadioCards`（K8sNode/WebServer/Minimal）+ 按组 `Accordion`；与预设不同的字段旁显示“已修改”标记与“恢复预设值” |
| 自定义片段 | 顶部 `ui-alert-danger`“以 root 在每台目标机上执行”；非 admin 只读；开启需 `AlertDialog`；`Textarea` 等宽字体；语法错误逐条列出行号 |
| 预览 | `Tabs`：本模块脚本、整份计划、与头版本的差异（`DiffView`）；跨模块校验错误（如“关闭密码登录但没有带公钥的 sudo 用户”）在顶部 `Alert` 中列出并链接到字段 |
| 保存栏 | 同 Profile |

### 3.6 引导网络 `/boot-network`

| 区块 | 规格 |
| :--- | :--- |
| 模式 | `RadioCards`（参考 `.radio-card-grid`）：ProxyDHCP / 复用现有 DHCP / 仅 Redfish，每张卡写明会监听的端口；网卡 `Select` 只列出白名单内网卡；保存产生 `boot_network` 新版本 |
| 应答规则 | UEFI 固件 `ipxe.efi`/`snponly.efi`、HTTP Boot 开关、是否同时写 Opt66/67 |
| bootnet 状态 | `.ui-card`：健康、模式、指令版本、socket 状态、计数器（收包、应答、忽略原因分布，`tabular`） |
| 现网 DHCP 探测 | 结果表（服务器地址、分类、引导相关选项）+“立即探测”；`Conflict` 时 `ui-alert-danger` + “确认继续”`AlertDialog`（写审计） |
| 现网 DHCP 配置片段 | 复用现有 DHCP 模式下显示 `Tabs`：ISC Kea、ISC dhcpd、Windows DHCP Server、MikroTik RouterOS，`CodeBlock` + `CopyButton`；附探测结论“已正确指向 / 缺少 next-server / 文件名不符” |
| 观测 | `DataTable`：时间、MAC、架构、user-class、结果或忽略原因；未登记 MAC 行操作“登记为节点”；分页 |

### 3.7 资产 `/assets`

| 区块 | 规格 |
| :--- | :--- |
| 资产卡片 | `.grid-2`，每个种类一张卡：当前激活版本（标签、iPXE 版本串或 Debian ABI、SHA-256 前 12 位 + `HoverCard` 全文 + `CopyButton`、来源、日期、阶段 `Badge`）；Debian 卡额外显示签名密钥指纹与各镜像源对齐结论（重新检查按钮） |
| 获取 | `Menu`：从官方源获取 / 从 URL 获取 / 上传（`FileDrop`，Debian 需同时放入 InRelease 与 SHA256SUMS）；进度 `Progress`（SSE 推送字节数） |
| 激活向导 | 候选版本旁的 `Steps`：获取 → 校验 → 金丝雀 → 激活；金丝雀步骤选择节点（`Select`）并跳转节点详情；“跳过金丝雀”需在 `AlertDialog` 中填写理由 |
| 版本表 | `DataTable`：标签、SHA-256、大小、来源、阶段、金丝雀节点、时间、操作（校验/金丝雀/激活/删除） |
| 回滚 | 卡片上“回滚到上一版本”，`AlertDialog` 显示两版差异 |
| 离线模式 | `XPXE_OFFLINE=true` 时顶部 `ui-alert-info` 说明只能上传，官方获取入口禁用 |
| 四态 | Empty：首次部署未获取任何资产时，每张卡显示引导文案与“从官方源获取”主按钮 |

### 3.8 配置历史 `/history`

`DataTable`：类型、对象、版本、作者、时间、说明；勾选任意两版（同一对象）后“对比”打开 `DiffView`（字段差异 + 渲染差异）；“回滚到此版本”`AlertDialog`。

### 3.9 审计日志 `/audit`

`DataTable` + `Pagination`：时间、操作者、来源 IP、动作、对象、结果；筛选（操作者 `Combobox`、动作 `Select`、结果 `Segmented`、时间范围）；“校验哈希链”显示 `ui-alert-success` 或断链位置；“导出 JSON Lines”。

### 3.10 设置 `/settings`

| 区块 | 规格 |
| :--- | :--- |
| 我的账户 | 修改密码；活动会话列表（IP、UA、最近活动，可注销）；API Token（创建时仅显示一次，`CopyButton`；列表显示前缀、作用域、过期时间，可吊销） |
| 用户管理（admin） | `DataTable` + `Dialog` |
| 装机超时 | 四个 `NumberField`（分钟，带范围），保存产生 `timeouts` 新版本 |
| 启动配置（只读） | 表格展示 README 第 9 节变量的当前值（密钥类只显示“已配置”），注明“需修改启动配置后重启” |
| 外观 | 打开 Theme Editor |

## 4. 数据层约定

- 每个页面的服务端数据都经 react-query；SSE 事件按类型 `setQueryData` 局部更新或标记失效；SSE 断开时显示顶栏小徽章并退化为 30 s 重取。
- 表单草稿只在首次加载、切换对象、或用户点击“载入新版本”时从服务端初始化；后台出现新头版本时显示 `ui-alert-info` 横幅，不覆盖草稿。
- 所有写操作完成后用 Toast 反馈（成功/失败，失败带错误码）。

## 5. 联调

`web/e2e/integration.spec.ts` 对真实 `xpxe-control`（Step 10 冒烟方式启动，加载 Step 7 的离线夹具资产）执行：

1. 初始管理员登录 → 强制改密。
2. 上传 iPXE 与 Debian 夹具资产 → 激活（跳过金丝雀并填写理由）→ 审计页出现该记录。
3. 由 `nexus-lvm` 预设新建 Profile → 修改 NTP → 预览中 `clock-setup/ntp-server` 实时变化 → 保存 → 差异中可见该字段。
4. 由 `k8s-node` 预设新建初始化计划 → 关闭 `docs_fonts` 分组 → `packages` 模块预览中对应包消失、包数减 6 → 保存。
5. 引导网络选“复用现有 DHCP”→ 片段 Tabs 出现。
6. 新建节点 → arm → 状态徽章变为 Armed、倒计时开始。
7. 运行 `cargo run -p xpxe-lab -- simulate-install --boot-base http://127.0.0.1:8080 --mac <mac>`（本步为 `xpxe-lab` 增加的子命令，受同一门禁约束）：它像真实客户端一样请求 `/boot/mac/{mac}` 得到会话 URL，取 preseed 并从 `late_command` 中解析出 agent 令牌，再取计划并按 d-i 与 agent 的顺序 POST 事件，不使用任何测试后门；页面无刷新地依次显示 Installing → Installed → Configuring → Ready，抽屉事件时间轴同步增长，期间节点表的选择与搜索词保持不变。
8. 对该节点“重新装机”：不输入主机名时确认按钮不可用。

## 6. 验收

```bash
scripts/web-gate.sh                       # 第 10 项检查覆盖全部页面
cd web && npx playwright test e2e/integration.spec.ts
```

验收标准：

- `scripts/web-gate.sh` 全绿：每个页面四态、键盘流程（每页至少一条：节点页“搜索 → 方向键选行 → 菜单 → arm → AlertDialog 确认”；Profile 页“Tab 到 Select → ↑↓ → Enter → 预览更新”）、375/767 px 无横向溢出。
- 联调脚本 8 步全部通过。
- 规范 §5 自检清单逐项勾选并附截图。
