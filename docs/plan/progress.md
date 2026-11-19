# x-pxe 实施进度日志

> 运行日志：每步结束时更新（提交、CI 运行号、偏差、实验室记录）。仓库外另有同内容副本。不记录任何密钥、令牌或口令。

- 当前（2026-11-18 12:50 UTC+8）：step04 全部完成。origin/main = 25a33e7，CI 绿（run 44239409097）。工作树干净。下一步 step05（docs/plan/step-05-init-modules-and-agent.md），尚未开始。
- 工作副本：/workspace/x-pxe-repo，branch main，与 origin/main 同步，工作树干净。
- 已完成：
  - docs 10d6d06
  - step01 f1c786f + CI 修复 1aa34d3（CI 绿：https://github.com/jjdufu/x-pxe/actions/runs/43579672194）
  - step02 c23d253（CI 绿：https://github.com/jjdufu/x-pxe/actions/runs/43586815453）Rust/Web/docker-build（不推送）/lab 冒烟均成功。
  - step03 3d2400c（CI 绿：https://github.com/jjdufu/x-pxe/actions/runs/43588703355）四个任务全绿：Rust 门禁、Web 门禁、镜像构建（不推送）、KVM 实验室冒烟。
- 实验室矩阵（2026-11-14 Asia/Shanghai，step02）：B1–B6、U1–U6、W1–W2、X1–X2、N1、N2、R1、R2、G1、H9a/H9b 全部通过。结论见 docs/lab/spike-report.md、docs/lab/compat-matrix.md。
- step03 本地验证：scripts/gate.sh 全部通过；27 个 xpxe-domain 单元测试通过（含 77 个状态×事件组合与 70 份 Step 2 请求夹具决策断言）；cargo tree -p xpxe-domain -e normal 不含 tokio/hyper/rusqlite。
- step03 新偏差（相对步骤文档文件名，类型名未改）：为满足 module_name_repetitions（门禁禁止 allow），arch.rs → option93.rs（ClientArch）、node.rs → inventory.rs（NodeSpec）、state.rs → lifecycle.rs（NodeState）、config.rs → versioning.rs（ConfigDocument）。DhcpAnswer 的 Ignore/Offer 均装箱以满足 variant_size_differences。xpxe-domain 尚未加入 [workspace.dependencies]（避免 unused_workspace_dependencies，待有 crate 依赖后再加）。NodeSpec 约束检查方法名为 check_constraints（避免与 ConfigDocument::validate 触发 same_name_method）。
- 未做：不打 tag、不发版、不推送镜像；步骤 04–13 尚未开始。
- 续做：从 docs/plan/step-04-debian-install-render.md 实现第 4 步 → gate → 中文提交 step04 → push → 等 CI 绿，再 05–13。

- step04 aa93612：Profile（provision.rs）、xpxe-render 全部渲染、选盘脚本、钩子、快照矩阵、注入/长度/一致性测试；xpxe-lab render 子命令（--set 覆盖）；lab/run.sh install-uefi|install-bios|install-static|install-all（acng 镜像、20G 系统盘 + 8G 数据盘 + 1G BMC 虚拟 U 盘、disk-first 启动顺序）。
  偏差：集成测试 main.rs + #[cfg(test)] mod cases（tests_outside_test_module）；tasksel 输出顺序 "standard, ssh-server"；静态场景仍在实验网（有 Kea）上做 iPXE 引导，d-i 用内核参数静态配置并在重启后 ping 10.77.0.16 验证。
- step04 实验室（2026-11-18 Asia/Shanghai）：install-uefi PASS（12:08 批次）、install-static PASS、install-bios PASS（12:26 批次）——installer.started/finished、重启从系统盘起来（SSH 横幅）、串口登录提示、静态地址 10.77.0.16 可达、选盘未碰 BMC 虚拟 U 盘。
  后续修复：cmdline 在 console= 前加 `---`（d-i 把控制台写进目标系统引导器，否则装好的系统串口无输出）；partman 钩子 debconf-get 回读 partman-auto/disk 与 grub-installer/bootdev（一次批次中 grub-install 被指向 /dev/sda=U 盘后失败，未复现，加回读防护）。
  实验室：lab-acng 在本机网络下回源 HTTP 500，run.sh 自动回落 lab/lib/apt-proxy.py（带缓存与重试）；/dev/kvm 属组在 box 重启后会变成 pulse-access，需 `sudo chgrp kvm /dev/kvm`；DI_SYSLOG=1 可把 d-i syslog 回传到 serve 日志排障。
- step14（手动 ISO 网络引导）：用户新增，按指示不开始，仅记录。
- step04 后续提交：e0ff927（实验室验证修复）、25a33e7（移除误提交的 __pycache__，.gitignore 忽略）。e0ff927 的 CI 被并发取消，25a33e7 包含其全部内容且 CI 全绿。
- step04 复跑（2026-11-18 12:48–12:55 UTC+8，代码 25a33e7，含 bootdev 回读防护与 `---` 控制台修复）：install-bios PASS（386s）、install-static PASS（356s）；installer.started/finished、重启从系统盘起来、串口登录提示、10.77.0.16 可达、BMC 虚拟 U 盘未写入。日志 /workspace/labruns/step04-rerun.log。
- step05 进行中（12:50 起）：xpxe-domain::init（20 模块类型、预设 k8s-node/minimal、package-groups.json、跨模块校验）已写，单元测试 9 个通过。
- step05（1/2）c0573ce（CI 绿：https://github.com/jjdufu/x-pxe/actions/runs/44243218940）：xpxe-domain::init、xpxe-render::init（20 模板、黄金快照、legacy 逐字节一致）、xpxe-agent（std-only，musl static-pie 约 1 MiB，9 个集成测试）。
- step05（2/3）bb4986e：用户决定把计划预设 k8s-node 命名为 deb13-init-scripts（不装 Kubernetes），sysctl 子预设 K8sNode → Deb13InitScripts（其取值即 legacy 内核参数）；lab/vm.sh --image/--seed（vvfat CIDATA）；lab/run.sh init-equivalence（lab/lib/init-equivalence.sh）。
  step05 偏差：yash-syntax 引入重复依赖，改为自研 shell 结构检查；中性默认（vimrc 默认仅 root、全权 sudo 默认空）；motd 头部两行回退为计划内差异（比较时剔除）；file 显示 static-pie linked（验收里 grep 'statically linked' 不匹配）；最终事件未送达时不重启、不禁用服务、保留计划（下次启动续跑重报）；网络中断后进度事件只试一次；event_id = <session>.<kind>[.<module>]；实验室 APT 源两台机器统一指向 10.77.0.1:3142（deb.debian.org/debian 与 /debian-security）。
- step05 完成（2026-11-18 13:50 UTC+8）：c0573ce（CI 44243218940 绿）、bb4986e（CI 44244589741 绿）、be71d00（CI 44246824113 绿，四个任务全绿）。
  实验室 init-equivalence PASS（13:36–13:46，541s）：A legacy vs B deb13-init-scripts 计划零差异（含 744 个包、masked 单元）；B 的 agent.finished 已送达、服务 disabled、plan.json 与 agent 已删、日志 19 起止 + 1 跳过（custom_snippet）。日志 /workspace/labruns/step05-initeq.log、结果 step05-initeq-result.txt。
  云镜像缓存在 lab/work/cloud/debian-13-genericcloud-amd64.qcow2（约 330 MB，已 gitignore）。
- step06 d242b9c（14:20 UTC+8 推送，CI 44250625191 四个任务全绿）：xpxe-store 全部模块 + xpxe-lab store-smoke + CI 冒烟步骤。gate 全绿，7 单元 + 24 集成测试；store-smoke 验收通过（数据目录无主密钥）。
  step06 偏差：句柄式 API（multiple_inherent_impl）；方法显式接收 now；query_opt/query_vec 替代 rusqlite 闭包（result_large_err）；dev profile 对 argon2/blake2 开 opt-level=3；退避 2^(n−4) 秒；MasterKey::load 返回告警列表；rotate/audit export 的 CLI 留到 step10。详见 docs/plan/step-06 第 7 节。
- step07：4b1c41a（进行中提交，CI 44253537181 绿）+ e11413a（完成，CI 44254392443 运行中）。xpxe-assets 全部模块（OpenPGP 子集、Debian 信任根、release/kernel/ipxe/align/fetch/kind/catalog/ingest），23 单元 + 14 集成测试（含测试内 HTTPS 服务）；xpxe-lab assets-smoke/assets-align；.github/workflows/assets-smoke.yml（每周 + 手动）。
  联网验收（14:47–14:50 UTC+8）：5 种资产全部 verified（netboot ABI 6.12.107+deb13-amd64）；deb.debian.org 已对齐；旧夹具 6.12.38 未对齐并列出 6.12.107/6.12.94。192.168.2.5 Nexus 从 box 不可达（超时）。日志 /workspace/labruns/step07-assets-{smoke,align}.log。
  step07 偏差：boot.ipxe.org EFI 移到 x86_64-efi/；官方 iPXE 为压缩映像无明文版本串（版本可空，undionly 用 ljmp 07c0 前缀检查）；.efi 节表截断检查；固件上限 1 GiB；netboot = 清单资产 + 两个成员文件；子钥指纹直接入白名单；对齐缓存/arm 拦截、金丝雀 arm 放到 step10；失败记录写审计；离线模式三类来源均拒官方主机。详见 docs/plan/step-07 第 7 节。
- 续做：step08（docs/plan/step-08-bootnet-proxydhcp-tftp.md）。step14 仍不开始。

## 更新 2026-11-18（step07 CI 修复）
- e11413a 的 CI 44254392443 失败：.gitignore 的 *.efi/*.kpxe 忽略了 xpxe-assets 的 iPXE 夹具，formats 集成测试在 CI 上找不到文件。
- 0bc35d1 修复（加例外规则并补交夹具），CI 44254646950 全绿（Rust 门禁/Web 门禁/KVM 实验室/镜像构建）。
- Step 07 完成。续做：docs/plan/step-08-bootnet-proxydhcp-tftp.md

## 更新 2026-11-18（step08 进行中）
- 00fcddc step08 第一部分（15:20 UTC+8 推送，CI 44258185831 四个任务全绿）：xpxe-domain::bootnet、xpxe-dhcp answer/probe/link、xpxe-tftp 可观测服务+并发限制、xpxe-render dhcp_snippets、xpxe-bootnet 全部模块 + 单元测试 + tests/replay（36 个 DISCOVER 夹具 expect.json）。
- 未提交（进行中）：xpxe-lab control-stub（内部 API 替身，--mode-file 切换模式，观测/心跳逐行 JSON）、lab/run.sh --bootnet（bootnet 以 UID 10001 + NET_BIND_SERVICE 运行；新场景 rogue-proxy、redfish-only-ports）、计划文档第 10 节偏差表。
- 实验室注意：proxy 场景需 lab/dhcp.sh start coexist（existing 下发 file，bootnet 正确判为冲突并暂停应答）。15:34–15:36 box 整体停顿约 2 分钟（systemd-logind 被 watchdog 杀），导致 bootnet 失联 30 s 清空指令——符合设计但会让场景失败，遇到时重跑。

## 更新 2026-11-18（step08 完成，按用户要求暂停）
- 962bdd4 step08 第二部分（17:23 UTC+8 推送，CI 44275288527 四个任务全绿）：xpxe-lab control-stub、lab/run.sh --bootnet、rogue-proxy / redfish-only-ports 场景、计划文档第 10 节偏差表 + 实验室验收记录。
- 实验室（--bootnet，bootnet 以 UID 10001 + 仅 NET_BIND_SERVICE 运行，内核 6.12）：coexist 下 B1–B6、U1–U6、W1、W2、N1 PASS；existing 下 X1、X2 PASS；rogue-proxy PASS（conflict，报告 10.77.0.3，应答 0 次）；redfish-only-ports PASS。gate 全绿，cargo tree -d 为空。日志 /workspace/labruns/step08-gate.log、lab/out/。
- 本次修正：B6 断言接受内容寻址路径 fw/<摘要>/undionly.kpxe；pxe-client 以 root 写的夹具改回 box 属主。注意 run.sh 选项（--bootnet、-j）必须写在场景名之前。
- 用户决定：第 8 步完成后暂停以节省本周额度（周一 11-23 07:41 UTC+8 重置）。第 9 步未开始。
- 续做：docs/plan/step-09-*.md → 10–13 → step14 → 全部验收后再发版。

## 更新 2026-11-18（step09 完成，按用户要求暂停）
- bac1a39 第一部分（CI 44278938829 绿）：xpxe-render::redfish_boot（autoexec.ipxe，DHCP/静态快照）、xpxe-image::session（会话 ISO：ESP.IMG 内 EFI/BOOT/BOOTX64.EFI + EFI/BOOT/autoexec.ipxe，唯一 0xEF 引导项，卷标 XPXE-<8 位>，确定性）、xpxe-image::readback（独立读回解析器）、xpxe-lab iso --session-fixture、DMTF public-rackmount1 Mockup 子集。
- 3340e37 第二部分（CI 44280427555 绿）：xpxe-redfish（指纹固定 ServerCertVerifier、probe、令牌会话/Basic 回落、资源发现、6 个厂商画像、部署第 2–6 步、eject/power），6 单元 + 13 集成测试（模拟 BMC + Mockup）。
- 4b1bcbf 第三部分（CI 44281402000 四个任务全绿）：xpxe-lab redfish probe|deploy|eject、node-check；lab R1/R2 走正式链路；redfish-bios-reject、redfish-pin-change 场景；计划文档第 6 节偏差表。
- 实验室（17:5x–18:0x UTC+8）：redfish-bios-reject PASS、redfish-pin-change PASS、R1 PASS（--iso /tmp/boot.iso）、R2 PASS（静态 + H6）。gate 全绿，cargo tree -d 空。日志 /workspace/labruns/step09-gate.log、lab/out/。
- 注意：box 重启后 /dev/kvm 属组又变成 pulse-access，已 sudo chgrp kvm；sushy-tools 2.2 无 SessionService，实验室走 Basic 回落。
- 用户决定：第 9 步完成后暂停（额度紧张）。第 10 步未开始。续做：docs/plan/step-10-*.md（控制面：ISO 缓存、部署前置检查、事件/审计、Effect::EjectMedia、内部 API 服务端）。

## 更新 2026-11-18（step10 完成，按用户要求暂停）
- 提交（均已普通推送 main）：5daf81f（一：存储发件箱/原子提交、运行态恢复、安装器事件地址带会话令牌，CI 44283626868 绿）、
  372cb4c（二：xpxe-control 控制面主体，CI 44286782273 绿）、f176fe8（三：e2e 编排库 + LAN 场景；CI 因 fmt 失败后被后续推送取消）、
  e3c2615（三·补：fmt 修复）、b4e5edd（四：timeouts/security/admin_transport/versioning/bootnet_failsafe/audit）、
  2ef72c8（五：redfish 场景；installer.started → EjectMedia）、5735139（六：计划文档偏差表与验收记录）。
  CI 有并发取消：中间提交的 run 被后续推送取消，最终以 5735139 的 CI 44289721982 为准。
- 内容：三监听（管理 HTTP/HTTPS、引导、内部）、认证/CSRF/角色、arm 前置检查 + 产物渲染、事件入口、发件箱 worker、定时器、
  SSE、WebUI 静态托管、真实 bootnet 内部 API、Redfish deploy/eject/power + ISO 缓存 /data/cache/iso、
  CLI serve/healthcheck/audit verify|export/rotate-master-key/backup/version；节点登记 BIOS+Redfish 422。
- 验收：xpxe-e2e 场景 lan_uefi、lan_bios、http_boot、timeouts、redfish、security、admin_transport×3、versioning、bootnet_failsafe、audit 全过；
  冒烟 401/404/exit；scripts/gate.sh 全绿；cargo tree -d 空。实验室场景本步未跑（实验室仍用 control-stub，切换在 Step 12）。
- 偏差见 docs/plan/step-10-control-plane.md 第 6 节（推迟到 Step 11/12 的 API 端点清单也在其中）。
- 用户决定：第 10 步完成后暂停。第 11 步未开始。续做：docs/plan/step-11-*.md。

## 更新 2026-11-18 21:30（step11 开始）
- step10 最终 CI 44289721982（5735139）绿。
- 用户要求 [pt26u]：把分步方案放进仓库（新增 docs/plan/step-14-manual-iso-netboot.md、docs/plan/progress.md、README 索引加状态列与 step14），与 step11 首个提交一起提交；继续 step11，做完停（不开始 step12）。
- 从本次起本日志在仓库内同步维护为 docs/plan/progress.md（仓库外副本 /workspace/x-pxe-progress.md 保持一致）。

## 更新 2026-11-18 22:10（step11 完成，按用户要求暂停）
- 提交（均已普通推送 main）：a5aa6b5（一：WebUI 基座 + 分步方案入库 + xpxe_csrf 双提交 cookie）、
  随后一个提交（二：画廊排版微调、step-11 偏差表与验收记录、§5 自检截图、README 状态、本日志）。最终 CI 以 HEAD 的 run 为准（见最终报告）。
- 内容：React 19 + Ark UI 5 + Tailwind v4 基座；Token 契约（4 灰阶 × 2 模式、resolveAccent）；Theme Editor（导出 JSON/CSS）；
  组件库（Select/Combobox/Menu/Dialog/AlertDialog/Drawer/Toast/DataTable/CommandPalette 等）；应用壳（侧栏、⌘K、SSE 状态、移动抽屉）；
  四态容器；lib（api/sse/storage/clipboard）；ESLint 约束；组件画廊；完整 scripts/web-gate.sh。
- 验收：web-gate 全过（Vitest 92、Playwright 60、scan-dist、contrast.mjs）；scripts/gate.sh 全绿；cargo tree -d 空；§5 自检附截图（docs/plan/assets/step-11/）。
- 偏差见 docs/plan/step-11-webui-foundation.md 第 13 节（Step 10 推迟的管理 API 全部移到 Step 12；8 个业务页为占位页）。
- 用户决定：第 11 步完成后暂停。第 12 步未开始。续做：docs/plan/step-12-*.md。

## 更新 2026-11-18 22:20 America/Denver（step12 完成，按用户要求暂停）
- 提交（普通推送 main）：e901cc7（一：WebUI 所需管理端点）、c2d32a1（二：xpxe-lab simulate-install 与 webui-stack 联调栈）、
  其后三个提交（三：全部业务页面；四：页面/联调 e2e 与门禁调整；五：偏差表、验收记录、§5 截图、README 状态、本日志）。最终 CI 以 HEAD 的 run 为准（见最终报告）。
- 内容：登录/强制改密、总览、节点（搜索/方向键选行/行菜单/抽屉/从观测导入）、Profile 与初始化计划编辑器（草稿、实时预览、差异、保存说明、409 重做）、
  引导网络（模式/网卡/探测/冲突确认/DHCP 片段）、资产（获取/上传/激活/回滚/删除）、配置历史（对比/回滚）、审计（过滤/分页/校验链/导出）、设置（口令/会话/令牌/用户/超时/启动配置/主题）。
- 验收：scripts/gate.sh 全绿；web-gate：Vitest 92、Playwright 65（本机深色对比度 4 组为既有环境问题，CI 绿）；integration.spec.ts 8 步通过；§5 截图在 docs/plan/assets/step-12/。
- 偏差见 docs/plan/step-12-webui-pages.md 第 7 节。
- 用户决定：第 12 步完成后暂停。第 13 步未开始。续做：docs/plan/step-13-*.md。
