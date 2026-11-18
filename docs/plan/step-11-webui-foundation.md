# Step 11：WebUI 基座、Token 与 Theme Editor（`web/`）

## 1. 目标

按 [`webui-style-guide.md`](../webui-style-guide.md)（下称“规范”）建立 WebUI 基座：设计 Token、双色模式、Theme Editor、组件库、应用外壳、四态容器与 Web 门禁。视觉与交互以 [`webui-ref/exhtml/`](../webui-ref/exhtml/)（下称“参考实现”）为字典：每个组件都注明取自参考实现的哪个文件、哪个类或函数，实现时逐项对照。

## 2. 技术栈（锁定）

| 层 | 选型 | 说明 |
| :--- | :--- | :--- |
| 框架 | React 19 + TypeScript（`strict`、`noUncheckedIndexedAccess`） | 只写 React；Ark UI 用 React 的 `asChild`，代码中不出现 `as-child` 或其他框架语法（规范 §1、§4.1） |
| 原语 | `@ark-ui/react` | 所有浮层、选择、对话框、菜单、开关、复选框、Tabs、Toast 都基于 Ark 状态机 |
| 样式 | Tailwind CSS v4（`@tailwindcss/vite`） | 唯一样式引擎，不引入 Panda CSS（规范 §4.3） |
| 图标 | `lucide-react` | SVG 随包打入，离线可用 |
| 数据 | `@tanstack/react-query` + 原生 `EventSource` | 后台刷新只更新缓存，不重建表单 |
| 路由 | `react-router` | - |
| 表单 | `react-hook-form` + `ajv`（JSON Schema 来自后端 `schemars` 导出） | 前端即时校验，后端为准 |
| 构建 | Vite | 产物只含同源资源 |
| 测试 | Vitest + Testing Library（jsdom）、Playwright（Chromium）+ `@axe-core/playwright` | 测试运行时全部离线 |

字体：只用参考实现 `styles.css` `:root` 中的系统字体栈 `--font-sans`（`-apple-system, BlinkMacSystemFont, 'SF Pro Text', 'PingFang SC', 'Hiragino Sans GB', 'Segoe UI', system-ui, sans-serif`）与 `--font-mono`，不加载任何网络字体（规范 §3 OFFLINE）。

## 3. Token

### 3.1 契约 Token 与扩展 Token

`web/src/styles/tokens.css` 是唯一定义 Token 的文件：

- **基线与项目契约**：规范 §2 的 9 个变量是基线；本节 3.2 表中的值是本项目的契约值，具有约束力，实现与门禁都以本节为准。项目契约值与规范基线只有一处不同：浅色模式的 `--text-muted` 加深到满足 WCAG AA（3.5 节给出实测值）。规范文档本身保持原文，不随项目取值修改。
- **项目新增 Token** `--border-input`：表单控件的边界色（`Input`、`Textarea`、`Select` 触发器、`Combobox`、`NumberField`、`Checkbox`、`Radio`、`Switch` 关闭态轨道、`Slider` 轨道），满足 WCAG 1.4.11 非文本对比度 ≥ 3:1。`--border` 只用于卡片、表格分隔线等装饰性边框。
- **扩展 Token**：取自参考实现 `styles.css` 第 1 节 `:root` 与 `[data-theme="dark"]`：`--surface-hover`、`--border-strong`、`--accent-soft`、`--danger`、`--danger-fg`、`--success`、`--warning`、`--info`、`--radius-sm`、`--radius-lg`、`--shadow-xs/sm/md`、`--font-sans`、`--font-mono`、`--tracking-tight`、`--ring`（`0 0 0 2px var(--bg), 0 0 0 4px var(--accent)`）。
- 参考实现里 Park 风格用的 `--park-*` 中间变量不引入；全仓禁止出现 `--park-`、`--colors-` 前缀（规范 §4.3，门禁扫描）。

### 3.2 灰阶（`html[data-gray]`）

| 灰阶 | 模式 | `--bg` | `--surface` | `--surface-2` | `--border` | `--border-input` | `--text` | `--text-muted` |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| Zinc（默认） | Light | `#fcfcfc` | `#f4f4f5` | `#e4e4e7` | `#e4e4e7` | `#8c8c8f` | `#18181b` | `#63636b` |
| Zinc | Dark | `#09090b` | `#121215` | `#18181b` | `#27272a` | `#626264` | `#fafafa` | `#a1a1aa` |
| Neutral | Light | `#fcfcfc` | `#f5f5f5` | `#e5e5e5` | `#e5e5e5` | `#8c8c8c` | `#171717` | `#656565` |
| Neutral | Dark | `#0a0a0a` | `#141414` | `#1f1f1f` | `#262626` | `#636363` | `#fafafa` | `#a3a3a3` |
| Slate | Light | `#fcfcfd` | `#f1f5f9` | `#e2e8f0` | `#e2e8f0` | `#868d99` | `#0f172a` | `#59677e` |
| Slate | Dark | `#020617` | `#0f172a` | `#1e293b` | `#1e293b` | `#5d6673` | `#f8fafc` | `#94a3b8` |
| Sand | Light | `#fdfdfc` | `#f5f5f4` | `#e7e5e4` | `#e7e5e4` | `#8f8c8b` | `#1c1917` | `#6b6460` |
| Sand | Dark | `#0c0a09` | `#1c1917` | `#292524` | `#292524` | `#686665` | `#fafaf9` | `#a8a29e` |

浅色 `--text-muted` 与两种模式的 `--border-input` 由各灰阶原色向 `--text` 在 sRGB 中线性插值得到，取满足 3.5 节门槛的最浅（浅色模式）或最暗（深色模式）值，色相与灰阶保持一致。

### 3.3 圆角阶梯

- `--radius-md` 取值 0–99 px：Theme Editor 提供阶梯按钮 `0 / 4 / 6 / 8 / 12 / 99`（None/Sm/Md/Base/Lg/Full，对应参考实现 `components.js` Park 区块的 `data-park-radius` 按钮组），以及 0–99 的滑杆（参考实现 `.ui-range`）。
- 派生：`--radius-sm = md × 0.6`、`--radius-lg = md × 1.4`（同参考实现 `[data-style="park"]` 的计算方式）；卡片、对话框、抽屉等大面积容器使用 `min(var(--radius-lg), 24px)`，避免 99 px 时容器变形，按钮、徽章、输入框保持药丸形。

### 3.4 Accent 与前景色自动反转

| 预设 | Light `--accent` / `--accent-fg` | Dark `--accent` / `--accent-fg` |
| :--- | :--- | :--- |
| Neutral（默认，跟随灰阶） | 灰阶 `--text` / `#ffffff` | 灰阶 `--text`（如 `#fafafa`）/ 灰阶 `--bg`（如 `#09090b`） |
| Slate | `#0f172a` / `#ffffff` | `#f8fafc` / `#020617` |
| Blue | `#2563eb` / `#ffffff` | 同左 |
| Emerald | `#047857` / `#ffffff` | 同左 |
| Violet | `#7c3aed` / `#ffffff` | 同左 |
| Amber | `#b45309` / `#ffffff` | 同左 |
| Rose | `#e11d48` / `#ffffff` | 同左 |

`resolveAccent(choice, mode, gray) -> { accent, accentFg, adjusted }`（纯函数，单元测试全覆盖）：

1. 无彩色（Neutral、Slate 或自定义色饱和度 < 0.15）：深色模式反转为浅色 Accent + 深色前景（规范 §4.2 正例）。
2. 彩色：前景固定 `#ffffff`；若 `contrast(#ffffff, accent) < 4.5`，在 OKLCH 中逐步降低亮度直到 ≥ 4.5，`adjusted = true`，Editor 显示“已自动加深以满足对比度”。
3. 同时要求 `contrast(accent, bg) ≥ 3`（焦点环与选中边框的非文本对比度），不满足时同样调整。

Emerald 取 `#047857` 而非参考实现中的 `#059669`：后者与白色前景对比度为 3.77，达不到 4.5。

### 3.5 对比度门槛与实测值

门槛（WCAG 2.x 相对亮度，全部为硬性要求）：

| 组合 | 门槛 |
| :--- | :--- |
| `--text`、`--text-muted` 分别对 `--bg`、`--surface`、`--surface-2` | ≥ 4.5 |
| `--accent-fg` / `--accent`（所有预设、两种模式） | ≥ 4.5（见 3.4） |
| `--accent` / `--bg`（焦点环、选中边框） | ≥ 3 |
| `--border-input` 分别对 `--bg`、`--surface`（表单控件只放在这两种背景上） | ≥ 3 |

实测值：

| 灰阶 | 模式 | `--text-muted` 对 bg / surface / surface-2 | `--border-input` 对 bg / surface |
| :--- | :--- | :--- | :--- |
| Zinc | Light | 5.80 / 5.42 / 4.69 | 3.27 / 3.05 |
| Zinc | Dark | 7.76 / 7.30 / 6.91 | 3.27 / 3.07 |
| Neutral | Light | 5.68 / 5.35 / 4.63 | 3.28 / 3.08 |
| Neutral | Dark | 7.85 / 7.30 / 6.53 | 3.30 / 3.07 |
| Slate | Light | 5.59 / 5.23 / 4.65 | 3.26 / 3.05 |
| Slate | Dark | 7.87 / 6.96 / 5.71 | 3.47 / 3.07 |
| Sand | Light | 5.70 / 5.32 / 4.62 | 3.28 / 3.06 |
| Sand | Dark | 7.83 / 6.93 / 6.01 | 3.46 / 3.06 |

作为参照，灰阶色板中紧邻 `--border` 的下一档（Zinc `#d4d4d8`）对 `--bg` 只有 1.47:1，因此 `--border-input` 取上表数值而非色板档位。`--border` 对背景约 1.2:1，只承担装饰作用，不用来标识控件边界。

### 3.6 与 Tailwind 的绑定

```css
/* web/src/styles/app.css */
@import "tailwindcss";
@import "./tokens.css";
@theme inline {
  --color-*: initial;
  --radius-*: initial;
  --color-bg: var(--bg);           --color-surface: var(--surface);   --color-surface-2: var(--surface-2);
  --color-border: var(--border);   --color-border-input: var(--border-input); --color-text: var(--text);         --color-muted: var(--text-muted);
  --color-accent: var(--accent);   --color-accent-fg: var(--accent-fg);
  --color-danger: var(--danger);   --color-success: var(--success);   --color-warning: var(--warning);  --color-info: var(--info);
  --radius-chip: var(--radius-sm); --radius-control: var(--radius-md); --radius-card: min(var(--radius-lg), 24px);
}
[hidden] { display: none !important; }   /* 规范 §3 HIDDEN，同参考实现 styles.css 第 2 节 */
.tabular { font-variant-numeric: tabular-nums; }
```

Tailwind 的圆角工具类使用 `rounded-control` 等独立命名，避免与契约变量 `--radius-md` 同名自引用。

## 4. Theme Editor

| 项 | 设计 |
| :--- | :--- |
| 入口 | 顶栏“主题”按钮（位置同参考实现 `index.html` 的 `#toggleTokensBtn`）打开右侧 Drawer（参考 `#demoDrawer`）；顶栏另有明暗切换按钮（参考 `#toggleThemeBtn`，sun/moon 图标与文字标签） |
| 控件 | 颜色模式 Light/Dark/System（Segmented，参考 `.tabs-bar/.tab-btn`）；灰阶 4 选 1（RadioCard，参考 `.radio-card-grid`）；圆角阶梯 + 滑杆；Accent 预设色块（参考 Park 区块 `data-park-accent` 按钮与 `.token-swatch`）+ 自定义色（Ark ColorPicker） |
| 实时预览 | Drawer 内一组样例：主按钮、描边按钮、输入框、Select、Switch、Badge、Alert、小表格；下方 Token 条显示当前值（参考 `.token-bar/.token-chip`）与每组对比度数值 |
| 应用方式 | `html[data-theme]`、`html[data-gray]` 属性 + `document.documentElement.style.setProperty` 写入 `--radius-md`、`--accent`、`--accent-fg`（同参考实现 `app.js` `applyThemeAndTokens`）；CSSOM 写入符合 CSP |
| 防闪烁 | `index.html` 同步加载同源的 `/theme-boot.js`，在 React 挂载前读取存储并设置属性（CSP 不允许内联脚本） |
| 持久化 | `src/lib/storage.ts` 封装 `localStorage`，所有读写包 `try/catch`，不可用时退化为内存（同参考实现 `loadSaved`/`savePref`）；键 `xpxe.theme.v1`，带 schema 版本 |
| 导出 | “导出主题配置”生成 `{ engine: "tailwind-v4", mode, gray, radius, accent }` JSON 与对应的 CSS 变量片段，可复制或下载；只导出 Tailwind 预设（规范 §4.3） |
| 重置 | 恢复 Zinc + Neutral + 6 px |

## 5. 组件库（`web/src/components/ui/`）

| 组件 | Ark 原语 | 参考实现出处（类名 / 函数） | 项目内用途 |
| :--- | :--- | :--- | :--- |
| `Button`、`IconButton` | - | `styles.css` `.ui-btn`、`.ui-btn-{primary,secondary,outline,ghost,danger}`、`-sm/-lg/-icon`、`.icon-btn`；`components.js`“按钮变体与状态” | 全局 |
| `Input`、`Textarea`、`InputGroup`、`FieldError` | Field | `.ui-input`、`.ui-textarea`、`.input-group`、`.input-addon`、`.input-action-btn`、`.ui-input.has-error`、`.ui-error-msg`；“文本与验证输入”；边框使用 `--border-input`（参考实现中的 `--border` 在此处替换） | 表单 |
| `Select` | Select | `.ui-select`、`.ui-select-trigger`、`.ui-select-menu`、`.ui-select-option`、`.ui-select-check`；`app.js` `bindCustomSelectsAndDropdowns`（↑↓、Enter、ESC、外部点击、`aria-expanded`/`aria-selected`） | 所有单选下拉，替代原生 `<select>` |
| `Combobox`（多选标签） | Combobox | `.combobox-wrap`、`.combobox-tags`、`.combobox-dropdown`；`app.js` `renderCombobox` | 时区、Locale、软件包、用户组 |
| `Menu`、`ContextMenu` | Menu | `.ui-dropdown`、`.ui-dropdown-menu`、`.ui-dropdown-item(.danger)`、`.ui-dropdown-divider`；`index.html` `#customContextMenu`（带 `kbd` 快捷键提示） | 行操作、表格右键 |
| `Dialog` | Dialog | `index.html` `#demoModal`、`.modal-backdrop`、`.ui-dialog-header/body/footer`；`app.js` `openModal`（打开后聚焦第一个输入） | 新建/编辑 |
| `AlertDialog` | Dialog（`role="alertdialog"`，点击遮罩不关闭） | `#alertDialogModal`、`.ui-badge-danger` 标签、`.ui-btn-danger` | 替代 `confirm()`：arm 已就绪节点、Redfish 部署、删除、回滚、激活资产、开启自定义片段 |
| `Drawer` | Dialog（侧边） | `#demoDrawer`、`.ui-drawer`、`.ui-drawer-body` | 节点详情、Theme Editor |
| `CommandPalette` | Dialog + Combobox | `#cmdModal`、`.cmd-dialog`、`.cmd-input-wrap`、`.cmd-list`、`.cmd-item.highlighted`；`app.js` `openCmdPalette`、`updateCmdHighlight`、`renderCmdList`；侧栏 `.cmd-trigger` + `⌘K` | 跳转页面、搜索节点（主机名/MAC）、arm、切换主题 |
| `Toaster` | Toast（`createToaster`） | `.toast-container`、`.toast`、`.toast-close`；`app.js` `showToast`（3.2 s 自动消失、可关闭、`success`/`danger` 变体、`aria-live="polite"`） | 替代 `alert()`，所有异步结果 |
| `Tabs`、`Segmented` | Tabs、SegmentGroup | `.ui-tabs/.ui-tab`、`.tabs-bar/.tab-btn` | 预览切换、筛选 |
| `Accordion` | Accordion | `.accordion-item/.accordion-trigger/.accordion-body` | Profile 分组表单 |
| `Switch` | Switch | `.ui-switch[role=switch][aria-checked]` | 模块启用、布尔字段 |
| `Checkbox` | Checkbox（含 `indeterminate`） | `.ui-checkbox`、`.checkbox-row`；`app.js` `syncIndeterminate` | 表格多选、软件包分组 |
| `RadioCards` | RadioGroup | `.radio-card-grid`、`.radio-card(.selected)`、`.radio-card-title/-sub` | 引导网络模式、灰阶、分区方案 |
| `ToggleGroup` | ToggleGroup | `.toggle-group`、`.toggle-btn` | 视图切换 |
| `NumberField` | NumberInput | `components.js`“数值、区间…”中的 stepper | 有范围的数值字段（超时、sysctl） |
| `Slider`、`RangeSlider` | Slider | `.ui-range`、`.dual-range-wrap/-track/-fill` | 圆角、选盘容量区间 |
| `FileDrop` | FileUpload | `.dropzone` | 资产上传 |
| `Progress` | Progress | `.progress-track`、`.progress-fill` | 下载、上传、装机进度 |
| `Badge` | - | `.ui-badge(-success/-warn/-danger/-outline/-solid)`、`.badge-dot` | 状态、资产阶段 |
| `Alert` | - | `.ui-alert(-info/-success/-warn/-danger)`、`.ui-alert-title/-desc` | 提示、错误态 |
| `Tooltip`、`HoverCard`、`Popover` | Tooltip、HoverCard、Popover | `.tooltip-wrap/.tooltip-bubble`、`.hover-card-wrap/-panel`、`.popover-panel` | 字段说明、SHA-256 全文、MAC 详情 |
| `Skeleton` | - | `.skeleton`；`app.js` `renderTableRows` 加载态行 | Loading 态 |
| `Steps` | Steps | `.steps-bar`、`.step-item(.done/.active)`、`.step-num` | 资产激活向导、节点装机进度 |
| `Timeline` | - | `.timeline-list/.timeline-item/.timeline-dot.<status>` | 节点事件、配置历史 |
| `Tracker` | - | `.tracker-bar/.tracker-cell` | 总览页近 30 天装机结果 |
| `Pagination` | Pagination | `.pagination/.page-btn` | 审计、观测 |
| `CodeBlock` | - | `.code-block`；`app.js` `enhanceCodeBlocks`（附复制按钮） | 渲染预览 |
| `DiffView` | - | `.diff-line.diff-bad/.diff-good` | 配置与渲染差异 |
| `CopyButton` | Clipboard | `app.js` `copyText`：`navigator.clipboard.writeText` 失败或不存在时，用隐藏 `textarea` + `document.execCommand('copy')` 降级，再失败提示手动复制 | 令牌外的所有可复制内容（URL、SHA-256、配置片段）；内网 HTTP 下 `navigator.clipboard` 常不可用 |
| `Card`、布局原语 | - | `.ui-card`、`.ui-card-surface`、`.card-label`、`.grid-2/3/4`、`.row-between`、`.row-gap`、`.form-field`、`.ui-label`、`.muted-text`、`.tabular`、`.mono`、`kbd` | 全局 |
| `DataTable` | Checkbox、Menu | 见第 6 节 | 节点、资产、审计、观测、版本 |

参考实现中的日历、OTP、评分、看板、甘特图、AI 对话不在本项目使用。

## 6. DataTable（规范 §3 TABLE & CHECKBOX）

以参考实现 `components.js` `#sec-table` 区块与 `app.js` `getFilteredTableRows`、`syncTableSelectAll`、`renderTableRows` 为蓝本：

- 工具栏：搜索框（宽 220 px）、“已选 N 行”、筛选 Select、列显隐 Menu（菜单内为复选框，点击不关闭菜单）、排序按钮、主操作按钮；窄屏换行（`.row-gap` + `flex-wrap`）。
- 全选框只作用于**当前过滤后的可见行**：`checked = 可见行全部选中`，`indeterminate = 可见行部分选中`；过滤条件变化后立即重新计算；被过滤掉的已选行保持选中，批量操作只作用于可见且选中的行，按钮上显示实际数量。
- 行复选框用 tbody 上的事件委托（同参考 `tbody.onchange`），不给每行单独绑定。
- 所有数值、时间、MAC、IP、SHA-256、大小列使用 `tabular` + `mono`。
- Loading 显示 4 行骨架（与真实列一致，隐藏列同样隐藏）；Empty 与 Error 见第 7 节；表格在卡片内横向滚动（`.table-wrap`），页面本身不出现横向滚动。
- 列显隐与排序存入 `storage.ts`（按表 ID）。

## 7. 四态容器（规范 §5）

`<AsyncView query loading empty error>` 统一处理：

| 状态 | 表现（参考出处） |
| :--- | :--- |
| Populated | 正常内容 |
| Empty | 居中卡片：44 px 图标框、标题、说明、两个按钮（“重置筛选”与主操作）（`components.js` untitled 区块 `mode === 'empty'`） |
| Loading | 与布局一致的骨架（`.skeleton`；同区块 `mode === 'loading'`） |
| Error | `.ui-alert-danger`：标题为错误码、说明为 `error.message`、`ui-btn-danger ui-btn-sm`“重新加载”调用 `refetch`（同区块 `mode === 'error'`） |

后台刷新（SSE 推送或定时重取）只更新查询缓存：已打开的表单保留未提交输入（表单状态只在首次加载或用户确认“载入新版本”时重置），列表与代码预览保持滚动位置，组件不因刷新重新挂载。

## 8. 应用外壳

取自参考实现 `index.html` 与 `styles.css` 第 2 节：

| 区域 | 规格 |
| :--- | :--- |
| `.app-shell` | flex；侧栏 + 主区 |
| 侧栏 `.sidebar` | 宽 276 px、sticky 满高、`--surface` 背景；头部 `.brand`（30 px `.brand-mark` 用 `--accent`/`--accent-fg`）；`.cmd-trigger`（搜索 + `⌘K`）；`.nav-group-label` 分组；`.nav-item(.active)`（激活项 `--bg` 背景 + 边框 + `--shadow-xs`）；底部 `.sidebar-footer` 显示当前用户与 bootnet 健康徽章 |
| 顶栏 `.topbar` | 高 54 px、sticky、`color-mix(in srgb, var(--bg) 88%, transparent)` + `backdrop-filter: blur(10px)`；左侧 `.breadcrumb`；右侧页面级 `.jump-pill` 快速跳转、`.divider-v`、主题按钮、明暗切换 |
| 内容 `.canvas` | `max-width: 1280px`、内边距 `28px 28px 64px`；分区用 `.showcase-section/.section-head/.section-title/.section-desc` 的层级 |
| ≤ 1024 px | `.grid-4` → 2 列，`.grid-3` → 1 列，隐藏 `.jump-pill` |
| ≤ 768 px | 侧栏变为离屏抽屉 + `.sidebar-backdrop`（`rgba(0,0,0,.4)`），显示 `.mobile-only` 菜单按钮，隐藏 `.hide-sm` 文字，`.grid-2` → 1 列，内容内边距 `16px 16px 48px` |

## 9. 全局事件（规范 §3 STATE & LEAKS）

- 唯一允许调用 `document.addEventListener`/`window.addEventListener` 的文件是 `src/app/global-events.ts`，在应用根组件挂载时注册一次、卸载时移除：`⌘K`/`Ctrl+K` 打开命令面板、滚动时关闭右键菜单（`passive: true`），对应参考实现 `app.js` 末尾的单次全局绑定。
- 浮层的外部点击与 ESC 由 Ark 状态机处理，组件内不额外绑定 document 监听。
- `useEffect` 中注册的订阅（SSE、定时器）必须返回清理函数，ESLint 强制。

## 10. Web 门禁（`scripts/web-gate.sh`）

| # | 检查 | 实现 |
| :--- | :--- | :--- |
| 1 | 类型 | `tsc --noEmit` |
| 2 | 代码规则 | ESLint `--max-warnings 0`：`no-alert`；`react/forbid-elements` 禁 `select`；`no-restricted-syntax` 禁止在 `global-events.ts` 外调用 `document/window.addEventListener`、在 `storage.ts` 外访问 `localStorage`、在 `clipboard.ts` 外访问 `navigator.clipboard`；禁止 `as-child`；`react-hooks/exhaustive-deps` |
| 3 | 单元测试 | Vitest：`resolveAccent` 全矩阵、`storage` 在 `localStorage` 抛异常时退化、`CopyButton` 在 `navigator.clipboard` 不存在时走 `execCommand`、Select/Menu/Combobox 键盘、DataTable 全选与半选 |
| 4 | 构建 | `vite build` |
| 5 | 产物扫描 | `scripts/scan-dist.mjs`：除 SVG 命名空间外不得出现 `http://`、`https://`；无 `<select`、`alert(`、`confirm(`、`prompt(`；无 `@import url(`、`@font-face` 外链；无 `--park-`、`--colors-`；CSS 中存在 `[hidden]{display:none!important}` |
| 6 | 离线 | Playwright 拦截所有非同源请求并断言请求数为 0 |
| 7 | 对比度 | 组件样例页（独立入口 `gallery.html`，不进入生产产物）在 4 灰阶 × 2 模式 × 7 Accent × 圆角 {0, 6, 99} 下运行 axe 颜色对比度与非文本对比度检查，任一违规即失败；`web/scripts/contrast.mjs` 直接解析 `tokens.css`，对 3.5 节全部组合做数值断言，任一低于门槛即失败；`resolveAccent` 断言主按钮 ≥ 4.5、焦点环 ≥ 3 |
| 8 | 键盘 | 仅用 Tab/↑/↓/Enter/ESC 完成：打开并选择 Select、Combobox 多选、Menu、命令面板、Dialog、AlertDialog、Drawer；每个可聚焦元素有可见 `:focus-visible` 环（`--ring`） |
| 9 | 四态 | 每个页面用路由拦截注入 Populated/Empty/Loading/Error，截图对比并断言错误态“重新加载”可恢复 |
| 10 | 移动端 | 视口 375 px 与 767 px 下每个页面 `document.documentElement.scrollWidth <= innerWidth` |

## 11. 交付物

`web/src/styles/{tokens,app}.css`、`web/src/theme/`（`resolveAccent`、`ThemeProvider`、`ThemeEditor`）、`web/public/theme-boot.js`、`web/src/components/ui/*`、`web/src/app/`（`AppShell`、`global-events.ts`、路由、`AsyncView`）、`web/src/lib/{storage,clipboard,api,sse}.ts`、`web/gallery.html`、`web/e2e/*.spec.ts`、完整的 `scripts/web-gate.sh` 与 `web/scripts/scan-dist.mjs`。

## 12. 验收

```bash
scripts/web-gate.sh
cd web && npx vite preview --outDir dist-gallery   # 人工按规范 §5 清单逐项核对
```

验收标准：`scripts/web-gate.sh` 第 1–10 项全部通过；规范 §5 的五项自检逐项勾选并附截图（Light/Dark 各一组、375 px 一组）。

## 13. 实现偏差（落地记录）

| 计划 | 实现 | 理由 |
| :--- | :--- | :--- |
| SPA 只依赖登录响应里的 `csrf_token` | 后端登录/登出额外下发非 HttpOnly 的 `xpxe_csrf` cookie（双提交），`lib/api.ts` 从 cookie 读取并在非安全方法上发送 `X-CSRF-Token`；e2e `security` 断言该 cookie 存在且不带 HttpOnly | 服务端只存令牌哈希，页面刷新后前端无法恢复令牌 |
| Step 10 推迟到 Step 11/12 的管理 API | 本步不实现，全部移到 Step 12；壳与各页只调用已有端点（`/me`、`/bootnet/status`、`/nodes`、SSE） | Step 11 只交付基座，业务页面在 Step 12 落地 |
| 各业务页面 | 除节点列表（DataTable）外，其余 8 页为占位页（标题 + 四态容器，按端点拉取） | 同上；占位页保证四态/移动端/离线门禁覆盖全部路由 |
| 四态“截图对比” | Playwright 对每页每态断言 DOM（骨架 `role=status`、空态、错误码 + “重新加载”可恢复），截图保存为测试产物，不做像素比对 | 本机与 CI 字体渲染不同，像素基线不稳定 |
| DataTable 复选框用 Ark Checkbox | 原生 `<input type=checkbox>`（`indeterminate` 由 ref 设置，tbody 事件委托） | 大表格下性能与可访问性更好；样式由 `ark.css` 统一 |
| CommandPalette 用 Ark Combobox | Ark Dialog + 自写 combobox/listbox（`aria-activedescendant`，分组标题） | Ark Combobox 不支持分组标题与跳转型条目混排 |
| Steps 用 Ark Steps | `<ol>` 有序列表 | Ark Steps 输出 tablist 语义，axe 报 ARIA 错误 |
| CopyButton 用 Ark Clipboard | `lib/clipboard.ts`（`navigator.clipboard`，失败回退 `execCommand`） | 统一剪贴板入口，便于 ESLint 约束 |
| Combobox 默认行为 | `inputBehavior="autohighlight"`、`selectionBehavior="clear"`、点击即展开、输入框非受控 | 多选时键入即高亮首项、Enter 选中后清空输入，键盘路径最短 |
| 语义色 | 浅色 success `#166534`、warning `#92400e`（原值在 `surface-2` 上不足 4.5）；深色 `#4ade80`/`#fbbf24`；徽章改用语义色变量 | `contrast.mjs` 实测 |
| `scan-dist` 零外链 | 白名单：`w3.org`（SVG 命名空间）、`react.dev/errors`、`reactrouter.com`、精确 `http://localhost`（React Router 内部基址）；Tailwind 许可横幅中的 URL 由 Vite 插件剥离 | 这些是库内字符串常量，不发起请求；离线门禁另行断言零外部请求 |
| `eslint-plugin-react` | 未引入；原生 `<select>`、`asChild` 等用 `no-restricted-syntax` 禁止 | 该插件与 ESLint 10 存在 peer 冲突 |
| ESLint API 约束 | 测试与 e2e 文件不受“裸 `localStorage`/`addEventListener`/clipboard”规则约束（仍禁原生 select 等） | 测试需要直接打桩这些 API |
| 跳转胶囊（jump-pill） | 未实现 | 无长页面需求，留给 Step 12 按页面需要补 |
| 画廊入口 | `vite build --mode gallery` 输出 `dist-gallery/`（`index.html` + `gallery.html`），生产 `dist/` 只含 `index.html` | 画廊不进入生产产物 |
| 主题初始化防闪烁 | 外置 `/theme-boot.js`（读 `xpxe.theme.v1`，失败回退默认） | 后端 CSP `default-src 'self'` 禁内联脚本 |
| 包体积 | 生产主包约 560 kB（gzip 170 kB），Vite 提示 >500 kB，未做代码分割 | 内网单页应用；Step 12 业务页面落地时按路由懒加载 |

### 验收记录（2026-11-18 Asia/Shanghai）

- `scripts/web-gate.sh` 第 1–10 项全部通过：typecheck、lint（0 警告）、Vitest 92 项、构建、`scan-dist`、`contrast.mjs`（3.5 节全部组合与实测表一致）、画廊构建、Playwright 60 项（离线零外联与 CSP、9 页 × 4 态 + 重新加载恢复、375/767 px 无横向溢出与移动抽屉、键盘全流程与 80 个 Tab 停靠点焦点环、4 灰阶 × 2 模式 × 7 Accent × 3 圆角 axe 对比度、画廊与 /nodes 深浅色 WCAG 2 A/AA）。
- `scripts/gate.sh`（Rust）全绿，`cargo tree -d` 为空。
- 规范 §5 自检：
  - [x] Light / Dark 主按钮、边框、输入框、弹窗文字对比度达标（axe + `contrast.mjs`）。截图：[nodes-light](./assets/step-11/nodes-light.png)、[nodes-dark](./assets/step-11/nodes-dark.png)、[gallery-light](./assets/step-11/gallery-light.png)、[gallery-dark](./assets/step-11/gallery-dark.png)。
  - [x] 零原生 `<select>`（ESLint + `scan-dist`），零外部网络依赖（离线 e2e）。
  - [x] Tab/↑/↓/Enter/ESC 操作下拉、命令面板、弹窗、抽屉，保留 `:focus-visible` 环（keyboard e2e）。
  - [x] Populated/Empty/Loading/Error 四态全覆盖（states e2e）。
  - [x] 语法/类型/构建零报错；<768 px 无横向溢出。截图：[nodes-375](./assets/step-11/nodes-375.png)、[nodes-375-drawer](./assets/step-11/nodes-375-drawer.png)。
