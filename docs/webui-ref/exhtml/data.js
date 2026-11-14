window.SHOWCASE_DATA = {
  categories: [
    { id: 'copy-paste', label: '一、Copy-Paste 源码级' },
    { id: 'cross-framework', label: '二、跨框架 / 无框架移植' },
    { id: 'design-system', label: '三、开箱即用型 Design System' },
    { id: 'headless', label: '四、无样式原语底座' }
  ],
  styles: [
    {
      id: 'coss',
      num: '01',
      name: 'coss ui',
      subtitle: 'Cal.com 官方设计系统 (原 Origin UI)',
      category: 'copy-paste',
      url: 'https://coss.com/ui',
      rating: 5,
      stack: 'Base UI + Tailwind CSS + shadcn CLI',
      summary: '由 Cal.com 团队实战打磨，视觉上比原生 shadcn 更克制、留白与字体层次更细腻，典型的高级 SaaS 黑白灰实用主义。',
      traits: ['克制留白', 'Base UI 底层', '细腻字阶', '高级 SaaS 极简'],
      tokens: {
        '--bg': '#ffffff',
        '--surface': '#fafafa',
        '--border': '#e5e5e5',
        '--accent': '#111111',
        '--radius': '8px',
        '--font-sans': 'Inter, sans-serif'
      },
      snippetTitle: 'Cal.com 风格预约时段卡片 (coss ui)',
      snippet: `// npx shadcn@latest add https://coss.com/ui/r/button.json
<Card className="border-neutral-200/80 shadow-xs">
  <CardHeader className="pb-3">
    <CardTitle className="tracking-tight text-base">30 Min Discovery Call</CardTitle>
    <CardDescription>Select a time slot in Asia/Shanghai</CardDescription>
  </CardHeader>
  <CardContent className="grid grid-cols-3 gap-2">
    <Button variant="outline" size="sm">09:30</Button>
  </CardContent>
</Card>`
    },
    {
      id: 'untitled',
      num: '02',
      name: 'Untitled UI React',
      subtitle: '企业级现代 SaaS 中性灰实用风',
      category: 'copy-paste',
      url: 'https://www.untitledui.com/react',
      rating: 5,
      stack: 'React Aria + Tailwind CSS v4 + TypeScript',
      summary: '偏企业级现代 SaaS 的中性灰实用风，字体阶梯、表单密度以及各种边界状态（空状态、加载态、错误态）非常完整。',
      traits: ['完整边界态', 'React Aria 无障碍', '企业级表单密度', 'Tailwind v4'],
      tokens: {
        '--bg': '#ffffff',
        '--surface': '#f9fafb',
        '--border': '#eaecf0',
        '--accent': '#101828',
        '--radius': '8px',
        '--font-sans': 'Plus Jakarta Sans, Inter'
      },
      snippetTitle: '边界状态与企业级表单 (Untitled UI)',
      snippet: `// npx untitledui@latest add empty-state
<EmptyState size="md">
  <EmptyState.FeaturedIcon icon={FolderSearch} color="gray" />
  <EmptyState.Title>暂无匹配的项目记录</EmptyState.Title>
  <EmptyState.Description>请调整筛选条件或新建一个项目。</EmptyState.Description>
  <EmptyState.Actions>
    <Button color="secondary">重置筛选</Button>
    <Button color="primary">新建项目</Button>
  </EmptyState.Actions>
</EmptyState>`
    },
    {
      id: 'tremor',
      num: '03',
      name: 'Tremor',
      subtitle: 'Vercel 旗下新瑞士数据看板组件',
      category: 'copy-paste',
      url: 'https://tremor.so/',
      rating: 5,
      stack: 'Tailwind CSS v4 + Radix UI + Recharts',
      summary: '纯正的 Vercel / 新瑞士数据可视化风格——极细灰阶边框、tabular-nums 等宽数字、高信息密度的 KPI 卡片、图表和追踪条。',
      traits: ['tabular-nums', '极细灰阶边框', '高密度 KPI', 'SVG 追踪条'],
      tokens: {
        '--bg': '#ffffff',
        '--surface': '#fafafa',
        '--border': '#e5e7eb',
        '--accent': '#18181b',
        '--radius': '6px',
        '--font-sans': 'Inter (tabular-nums)'
      },
      snippetTitle: 'KPI 卡片与服务状态追踪条 (Tremor)',
      snippet: `<Card className="p-5 border-zinc-200 dark:border-zinc-800">
  <p className="text-xs font-medium text-zinc-500">API 正常率 (30天)</p>
  <p className="mt-1 text-2xl font-semibold tabular-nums">99.98%</p>
  <Tracker data={uptimeDays} className="mt-4" />
</Card>`
    },
    {
      id: 'kibo',
      num: '04',
      name: 'Kibo UI',
      subtitle: '重业务逻辑的 shadcn 扩展组件库',
      category: 'copy-paste',
      url: 'https://www.kibo-ui.com/',
      rating: 5,
      stack: 'React + Tailwind CSS + shadcn CLI Registry',
      summary: '完全复用 shadcn/ui 的黑白灰 CSS 变量体系，专门补齐了 Kanban 看板、甘特图、树形目录、代码编辑器、AI 对话等重业务组件。',
      traits: ['Kanban 看板', '甘特图', '树形目录', 'AI 对话控件'],
      tokens: {
        '--bg': '#ffffff',
        '--surface': '#f4f4f5',
        '--border': '#e4e4e7',
        '--accent': '#18181b',
        '--radius': '8px',
        '--font-sans': 'Inter, JetBrains Mono'
      },
      snippetTitle: 'Kibo UI 业务组件 CLI 拉取',
      snippet: `npx kibo-ui add kanban gantt tree-view ai-conversation
// 100% 复用现有 shadcn/ui CSS Variables (--background, --foreground, --border)`
    },
    {
      id: 'park',
      num: '05',
      name: 'Park UI',
      subtitle: '跨框架极简规范 + 在线 Theme Editor',
      category: 'cross-framework',
      url: 'https://park-ui.com/',
      rating: 5,
      stack: 'Ark UI + Tailwind CSS / Panda CSS (React / Vue / Solid)',
      summary: '非常干净的中性极简风，官方提供在线 Theme Editor，可微调灰度色阶、圆角和字体并直接导出配置，同时支持 React、Vue、Solid。',
      traits: ['React / Vue / Solid', 'Ark UI 状态机', '在线 Theme Editor', '中性极简'],
      tokens: {
        '--bg': '#fcfcfc',
        '--surface': '#f4f4f5',
        '--border': '#e4e4e7',
        '--accent': '#18181b',
        '--radius': '6px',
        '--font-sans': 'Inter, sans-serif'
      },
      snippetTitle: 'Park UI 跨框架一致性示例',
      snippet: `// React / Vue / Solid 共享同一套 Ark UI 状态机与 Park Token
import { Button } from '~/components/ui/button'
<Button variant="outline" size="md">Export Theme Config</Button>`
    },
    {
      id: 'basecoat',
      num: '06',
      name: 'Basecoat',
      subtitle: '零 React 依赖的 1:1 shadcn/ui v4 复刻',
      category: 'cross-framework',
      url: 'https://github.com/hunvreus/basecoat',
      rating: 5,
      stack: '纯 HTML + Tailwind CSS + 极少量原生 JS',
      summary: '1:1 复刻 shadcn/ui v4 的黑白灰极简视觉与交互质感，零 React 依赖。Astro、HTMX、Laravel、Rails、Django 或纯静态页面直接复制使用。',
      traits: ['零 React 依赖', '纯 HTML + CSS', '1:1 shadcn v4', '后端模板神器'],
      tokens: {
        '--bg': '#ffffff',
        '--surface': '#f4f4f5',
        '--border': '#e4e4e7',
        '--accent': '#09090b',
        '--radius': '8px',
        '--font-sans': 'Inter, system-ui'
      },
      snippetTitle: '纯 HTML + 原生属性驱动 (Basecoat)',
      snippet: `<!-- 无需 React / JSX，直接用于 Astro, HTMX, Django, Rails -->
<button class="btn" type="button">Primary</button>
<button class="btn-outline" type="button">Outline</button>
<div class="card">
  <header><h2>Card Title</h2></header>
  <section><p>Pure semantic HTML.</p></section>
</div>`
    },
    {
      id: 'radix',
      num: '07',
      name: 'Radix Themes',
      subtitle: 'Token 驱动的现代开发工具质感',
      category: 'design-system',
      url: 'https://www.radix-ui.com/themes',
      rating: 4,
      stack: 'React + 原生 CSS Tokens (无需 Tailwind)',
      summary: 'Radix 原班团队出品，字体排版系统、中性灰调（Gray/Slate/Mauve）与边框对比度极其考究，通过顶层 <Theme> 控制全局缩放、圆角和灰阶。',
      traits: ['<Theme> 全局控色', '精密灰阶对比', '无需 Tailwind', '开发工具质感'],
      tokens: {
        '--bg': '#ffffff',
        '--surface': '#f9f9fb',
        '--border': '#dcdde0',
        '--accent': '#3e63dd',
        '--radius': '6px',
        '--font-sans': 'Inter, sans-serif'
      },
      snippetTitle: '<Theme> 全局 Token 控制器 (Radix Themes)',
      snippet: `<Theme accentColor="indigo" grayColor="slate" radius="medium" scaling="100%">
  <Card size="2">
    <Flex gap="3" align="center">
      <Avatar fallback="R" />
      <Box><Text weight="bold">Radix Themes</Text></Box>
    </Flex>
  </Card>
</Theme>`
    },
    {
      id: 'reshaped',
      num: '08',
      name: 'Reshaped',
      subtitle: '瑞士网格与 Figma 对齐的 Token 系统',
      category: 'design-system',
      url: 'https://reshaped.so/',
      rating: 4,
      stack: 'React + CSS Variables + Figma Variables',
      summary: '标准的瑞士网格与 Design Token 驱动系统，黑白灰层级、间距比例（4px 基准单元）和暗黑模式打磨得非常精准，与 Figma 变量严格对齐。',
      traits: ['瑞士网格', '4px 基准单元', 'Figma 严格对齐', '多层级表面'],
      tokens: {
        '--bg': '#ffffff',
        '--surface': '#f5f6f8',
        '--border': '#dfe2e7',
        '--accent': '#111827',
        '--radius': '8px',
        '--font-sans': 'Inter, sans-serif'
      },
      snippetTitle: 'Reshaped 瑞士网格与 Token 规范',
      snippet: `<View padding={4} borderRadius="medium" backgroundColor="elevation-base">
  <Grid columns={{ s: 1, m: 3 }} gap={4}>
    <Grid.Item><Card>Token-driven layout</Card></Grid.Item>
  </Grid>
</View>`
    },
    {
      id: 'base-ui',
      num: '09',
      name: 'Base UI',
      subtitle: 'MUI 团队维护的无样式原语底座',
      category: 'headless',
      url: 'https://base-ui.com/',
      rating: 5,
      stack: 'React + 任意样式方案 (2026 shadcn 默认底座)',
      summary: '完全无样式（Unstyled），专为搭建 Vercel Geist 或 shadcn 这类高定极简设计系统而生；吸取了 Radix 与 MUI 的经验，API 极度干净。',
      traits: ['完全无样式原语', '干净 Render Props', 'shadcn 2026 底座', '可切换裸态对比'],
      tokens: {
        '--bg': '#ffffff',
        '--surface': '#fafafa',
        '--border': '#e4e4e7',
        '--accent': '#09090b',
        '--radius': '6px',
        '--font-sans': 'Inter, sans-serif'
      },
      snippetTitle: 'Base UI 无样式原语 + 自定义样式层',
      snippet: `import { Switch } from '@base-ui-components/react/switch';
<Switch.Root className="peer inline-flex h-5 w-9 rounded-full border ...">
  <Switch.Thumb className="block size-4 rounded-full bg-white transition" />
</Switch.Root>`
    },
    {
      id: 'react-aria',
      num: '10',
      name: 'React Aria Components',
      subtitle: 'Adobe 顶级无障碍与复杂交互原语',
      category: 'headless',
      url: 'https://react-spectrum.adobe.com/react-aria/',
      rating: 4,
      stack: 'React + 任意 CSS / Tailwind CSS (Apache-2.0)',
      summary: '无预设样式，但提供业界最顶级的无障碍（a11y）、键盘导航和复杂组件交互（日历、日期选择器、ComboBox、虚拟化表格）。',
      traits: ['顶级 A11y 无障碍', '全键盘导航', '复杂日历/ComboBox', '实时朗读播报'],
      tokens: {
        '--bg': '#ffffff',
        '--surface': '#f8fafc',
        '--border': '#cbd5e1',
        '--accent': '#0f172a',
        '--radius': '6px',
        '--font-sans': 'Inter, sans-serif'
      },
      snippetTitle: 'React Aria 无障碍日历与键盘焦点管理',
      snippet: `import { Calendar, CalendarGrid, CalendarCell } from 'react-aria-components';
<Calendar aria-label="预约日期">
  <CalendarGrid>{(date) => <CalendarCell date={date} />}</CalendarGrid>
</Calendar>`
    }
  ],
  kanbanInitial: [
    {
      col: 'todo',
      title: '待处理 (Backlog)',
      items: [
        { id: 'KB-101', title: '迁移 Dialog 至 Base UI 原语', tag: 'Core', priority: 'High' },
        { id: 'KB-102', title: '补充 EmptyState 边界占位图', tag: 'UI', priority: 'Medium' }
      ]
    },
    {
      col: 'progress',
      title: '进行中 (In Progress)',
      items: [
        { id: 'KB-103', title: '对齐 Figma Dark Mode Tokens', tag: 'Tokens', priority: 'High' },
        { id: 'KB-104', title: '实现表格键盘上下键导航', tag: 'A11y', priority: 'Medium' }
      ]
    },
    {
      col: 'done',
      title: '已完成 (Done)',
      items: [
        { id: 'KB-105', title: '发布 Tremor 30 天 Tracker 组件', tag: 'Charts', priority: 'Low' }
      ]
    }
  ],
  ganttTasks: [
    { name: 'Token 规范定义', owner: 'Design', start: 0, span: 3, status: 'done' },
    { name: 'Base UI 原语封装', owner: 'Frontend', start: 2, span: 4, status: 'active' },
    { name: 'A11y 键盘测试', owner: 'QA', start: 5, span: 3, status: 'active' },
    { name: '文档与 CLI 发布', owner: 'DX', start: 7, span: 3, status: 'pending' }
  ],
  treeData: [
    {
      name: 'src',
      type: 'folder',
      open: true,
      children: [
        {
          name: 'components/ui',
          type: 'folder',
          open: true,
          children: [
            { name: 'button.tsx', type: 'file', badge: 'Base UI' },
            { name: 'dialog.tsx', type: 'file', badge: 'A11y' },
            { name: 'kanban.tsx', type: 'file', badge: 'Kibo' }
          ]
        },
        { name: 'styles/tokens.css', type: 'file', badge: 'CSS Vars' },
        { name: 'theme.config.ts', type: 'file', badge: 'Park' }
      ]
    }
  ],
  tableRows: [
    { id: 'PRJ-01', name: 'Cal.com Booking Core', owner: 'Peer Richelsen', status: 'active', latency: '42ms', coverage: 98, updated: '2 分钟前' },
    { id: 'PRJ-02', name: 'Vercel Analytics Edge', owner: 'Lee Robinson', status: 'active', latency: '18ms', coverage: 99, updated: '14 分钟前' },
    { id: 'PRJ-03', name: 'Enterprise Billing UI', owner: 'Jordan Walke', status: 'review', latency: '67ms', coverage: 91, updated: '1 小时前' },
    { id: 'PRJ-04', name: 'Basecoat HTML Kit', owner: 'Ronan Berder', status: 'active', latency: '11ms', coverage: 96, updated: '3 小时前' },
    { id: 'PRJ-05', name: 'Radix Theme Engine', owner: 'Vlad Moroz', status: 'paused', latency: '54ms', coverage: 94, updated: '昨天' },
    { id: 'PRJ-06', name: 'Aria DateRange Grid', owner: 'Devon Govett', status: 'active', latency: '29ms', coverage: 100, updated: '2 天前' }
  ],
  comboboxOptions: [
    'coss ui (Base UI + Tailwind)',
    'Untitled UI React (React Aria)',
    'Tremor (Vercel Data Viz)',
    'Kibo UI (Kanban & Gantt)',
    'Park UI (Ark UI Cross-Framework)',
    'Basecoat (Pure HTML + Tailwind)',
    'Radix Themes (Token System)',
    'Reshaped (Swiss Grid)',
    'Base UI (Unstyled Primitives)',
    'React Aria Components (Adobe A11y)'
  ],
  tremorSeries: [32, 41, 38, 55, 61, 58, 72, 69, 84, 79, 91, 96],
  tremorMonths: ['1月', '2月', '3月', '4月', '5月', '6月', '7月', '8月', '9月', '10月', '11月', '12月'],
  trackerDays: Array.from({ length: 30 }, (_, i) => ({
    day: `11月${i + 1}日`,
    status: i === 11 ? 'degraded' : i === 22 ? 'down' : 'operational',
    uptime: i === 11 ? '99.12%' : i === 22 ? '97.40%' : '100.0%'
  })),
  promptGuides: {
    coss: {
      role: 'Cal.com 实战级极简黑白灰 SaaS 界面规范（coss ui / Base UI + Tailwind）',
      dualTokens: {
        light: { '--bg': '#ffffff', '--surface': '#fcfcfc', '--surface-2': '#f5f5f5', '--border': '#e5e5e5', '--text': '#111111', '--text-muted': '#666666', '--accent': '#111111', '--accent-fg': '#ffffff', '--radius-md': '8px' },
        dark: { '--bg': '#0a0a0a', '--surface': '#111111', '--surface-2': '#1a1a1a', '--border': '#262626', '--text': '#f5f5f5', '--text-muted': '#a3a3a3', '--accent': '#f5f5f5', '--accent-fg': '#0a0a0a', '--radius-md': '8px' }
      },
      designRules: [
        '克制留白与细腻字阶：标题使用 tight tracking（-0.025em），避免大面积高饱和色块、霓虹渐变或重投影（仅保留极轻 shadow-xs）。',
        '圆角统一：按钮/输入框/菜单项 8px，卡片/弹窗 12px，所有数字、时间槽、价格强制使用 font-variant-numeric: tabular-nums。'
      ],
      stackRules: [
        '底层交互原语强制使用 @base-ui-components/react，源码通过 shadcn CLI（coss.com/ui）拷贝至本地 components/ui/，禁止引入黑盒封装库。',
        '使用 Tailwind CSS 语义变量绑定表面与边框色，确保浅色/深色模式下 1px 边框均清晰可见。'
      ],
      forbidden: [
        '禁止使用 Radix 的 asChild 属性（Base UI 已移除 asChild，必须用 render prop）。',
        '禁止在无边框的暗黑卡片上叠加相同纯黑底色导致视觉粘连。',
        '禁止使用浏览器原生 <select>、alert() 或 confirm() 替代风格化弹层。'
      ],
      pitfalls: [
        {
          title: '禁止混用 Radix asChild 与 Base UI render prop',
          bad: '<DialogTrigger asChild><Button>打开</Button></DialogTrigger>',
          good: '<Dialog.Trigger render={<Button />}>打开</Dialog.Trigger>',
          detail: '从旧版 shadcn(Radix) 重构迁移时，所有 Trigger/Tooltip/Menu 必须将 asChild 改写为 render prop，否则无法触发弹窗或丢失事件绑定。'
        },
        {
          title: '切勿破坏 Base UI 的 data-* 状态属性',
          bad: 'style={{ display: open ? "block" : "none" }}',
          good: 'className="data-[open]:animate-in data-[closed]:animate-out"',
          detail: '开关/弹窗/菜单动画依赖 data-open / data-closed / data-starting-style / data-ending-style，硬写 display:none 会打断退出过渡。'
        },
        {
          title: 'Portal 弹层被父级 overflow-hidden 裁切',
          bad: '<div className="overflow-hidden"><SelectPopup /></div>',
          good: '<Select.Portal><Select.Positioner><Select.Popup /></Select.Positioner></Select.Portal>',
          detail: '所有自定义 Select / Popover / ContextMenu 必须挂载至 Portal 并统一 z-index 层级。'
        }
      ]
    },
    untitled: {
      role: '企业级现代 SaaS 中性灰实用界面规范（Untitled UI React + React Aria + Tailwind v4）',
      dualTokens: {
        light: { '--bg': '#ffffff', '--surface': '#f9fafb', '--surface-2': '#f2f4f7', '--border': '#eaecf0', '--text': '#101828', '--text-muted': '#475467', '--accent': '#101828', '--accent-fg': '#ffffff', '--radius-md': '8px' },
        dark: { '--bg': '#0c111d', '--surface': '#161b26', '--surface-2': '#1f242f', '--border': '#1f242f', '--text': '#f5f5f6', '--text-muted': '#94969c', '--accent': '#f5f5f6', '--accent-fg': '#0c111d', '--radius-md': '8px' }
      },
      designRules: [
        '冷调中性灰阶：主背景与次级表面（#F9FAFB / #161B26）配合 crisp 1px #EAECF0 边框与轻微 shadow-xs（0 1px 2px rgba(16,24,40,0.05)）。',
        '所有列表、表格与异步业务区块必须成套实现 4 种边界状态：正常数据态、EmptyState 空状态（含图标+文案+CTA）、Skeleton 骨架屏加载态、Error 重试态。'
      ],
      stackRules: [
        '基于 react-aria-components + Tailwind CSS v4 + TypeScript，表单控件包含清晰 Label、Hint、Inline Error 与禁用态。',
        '图标尺寸、按钮高度（30/36/42px）与表单栅格保持企业级高密度对齐。'
      ],
      forbidden: [
        '禁止只实现“有数据”状态而忽略空状态/加载态/错误态。',
        '禁止在 React Aria Button 上混用原生 onClick / disabled 替代 onPress / isDisabled。',
        '禁止在带 hidden 属性的容器上同时写内联 style="display:flex"。'
      ],
      pitfalls: [
        {
          title: 'React Aria 事件属性为 onPress / isDisabled',
          bad: '<AriaButton onClick={submit} disabled={loading}>提交</AriaButton>',
          good: '<AriaButton onPress={submit} isDisabled={loading}>提交</AriaButton>',
          detail: '混用原生 onClick / disabled 会绕过 React Aria 的键盘 Space/Enter 统一封装与触摸端防穿透机制。'
        },
        {
          title: '内联 display:flex 覆盖 [hidden] 导致骨架屏常驻',
          bad: '<div id="skeleton" hidden style="display:flex">...</div>',
          good: '全局 CSS 声明 [hidden] { display: none !important; }',
          detail: '内联 display 优先级高于 UA 默认 [hidden]，切换 Loading 态与真实内容时极易导致两者同时显示。'
        },
        {
          title: 'Tailwind v4 @theme 变量与旧 tailwind.config.js 冲突',
          bad: '在 tailwind.config.js 中重复定义未同步的 colors.gray',
          good: '统一在 globals.css 的 @theme { --color-border: var(--border); } 中声明',
          detail: '迁移至 Tailwind v4 时若保留旧配置冲突，会导致边框色与阴影 Token 失效。'
        }
      ]
    },
    tremor: {
      role: 'Vercel / 新瑞士高密度数据看板与可视化规范（Tremor + Tailwind v4 + Recharts）',
      dualTokens: {
        light: { '--bg': '#ffffff', '--surface': '#fafafa', '--surface-2': '#f4f4f5', '--border': '#e5e7eb', '--text': '#111827', '--text-muted': '#6b7280', '--accent': '#111827', '--accent-fg': '#ffffff', '--radius-md': '6px' },
        dark: { '--bg': '#000000', '--surface': '#0a0a0a', '--surface-2': '#141414', '--border': '#27272a', '--text': '#fafafa', '--text-muted': '#a1a1aa', '--accent': '#ffffff', '--accent-fg': '#000000', '--radius-md': '6px' }
      },
      designRules: [
        '纯粹瑞士数据卡片美学：极细 1px 边框（#E5E7EB / #27272A）、零重阴影（shadow-none）、6px 利落圆角、高信息密度网格。',
        '所有 KPI、百分比、延迟、金额与坐标读数强制添加 font-variant-numeric: tabular-nums。'
      ],
      stackRules: [
        '使用 Tremor Copy-Paste Blocks（KPI Card、AreaChart、BarChart、Tracker、CategoryBar），可与 Vercel Geist / shadcn 混用。',
        '图表配色保持克制，SVG 线框与坐标轴必须引用 CSS Variables 适配深浅模式。'
      ],
      forbidden: [
        '禁止在 SVG 图表中写死 #111827 或 #e5e7eb（暗黑模式下坐标轴与曲线会直接隐身）。',
        '禁止将 Recharts ResponsiveContainer 放入无固定高度的 Flex 子项中。',
        '禁止省略 tabular-nums 导致实时数值刷新时左右抖动。'
      ],
      pitfalls: [
        {
          title: 'Recharts ResponsiveContainer 高度塌陷为 0px',
          bad: '<div className="flex-1"><ResponsiveContainer width="100%">...</ResponsiveContainer></div>',
          good: '<div className="h-64 min-h-[240px] w-full"><ResponsiveContainer width="100%" height="100%">...</ResponsiveContainer></div>',
          detail: 'Flex/Grid 子容器未明确高度时，图表会渲染成空白区域。'
        },
        {
          title: 'SVG 颜色写死导致暗黑模式不可见',
          bad: '<polyline stroke="#111827" />',
          good: '<polyline stroke="var(--accent)" />',
          detail: '所有图表的线条、网格线和标注文本必须使用 CSS Variables。'
        },
        {
          title: 'Tracker 追踪条小方块在窄屏溢出',
          bad: '固定 width: 12px 导致 90 天追踪条撑破卡片',
          good: '父容器 display:flex; gap:3px; 子块 flex:1; min-width:2px;',
          detail: '30/90 天服务状态条需使用弹性宽度并确保 Tooltip 不被 overflow-hidden 截断。'
        }
      ]
    },
    kibo: {
      role: '重业务逻辑 SaaS 扩展组件规范（Kibo UI + shadcn/ui CSS Variables）',
      dualTokens: {
        light: { '--bg': '#ffffff', '--surface': '#f8f8fa', '--surface-2': '#f1f1f4', '--border': '#e4e4e7', '--text': '#09090b', '--text-muted': '#71717a', '--accent': '#18181b', '--accent-fg': '#ffffff', '--radius-md': '8px' },
        dark: { '--bg': '#09090b', '--surface': '#111113', '--surface-2': '#18181b', '--border': '#27272a', '--text': '#fafafa', '--text-muted': '#a1a1aa', '--accent': '#fafafa', '--accent-fg': '#09090b', '--radius-md': '8px' }
      },
      designRules: [
        '100% 复用 shadcn/ui 的黑白灰语义变量（--background / --card / --border / --muted），保证 Kanban、甘特图、树形目录与基础组件视觉零割裂。',
        '重业务容器需提供清晰层级边框、滚动边界与紧凑操作按钮。'
      ],
      stackRules: [
        '通过 npx kibo-ui add <component> 按需拉取 Kanban、Gantt、TreeView、CodeBlock、AI Conversation 源码至本地仓库。',
        '所有拖拽操作必须附带键盘方向键或按钮降级控制。'
      ],
      forbidden: [
        '禁止在甘特条、看板列底色中硬编码 bg-slate-100 等非语义颜色。',
        '禁止只支持鼠标拖拽而无移动端/键盘降级按钮。',
        '禁止在 AI 对话追加消息时触发整页重绘导致输入框失焦。'
      ],
      pitfalls: [
        {
          title: 'CLI 安装重业务组件时覆盖已有魔改基础组件',
          bad: '直接 --overwrite 覆盖项目中已定制的 button.tsx / badge.tsx',
          good: '先查看 git diff 仅合并 Kibo 新增依赖，保留现有基础组件魔改',
          detail: 'npx kibo-ui add 可能提示更新关联的基础 shadcn 组件，务必先 diff。'
        },
        {
          title: 'AI Chat 追加消息未滚动且导致输入框失焦',
          bad: '每次发送消息调用整页 renderMain() 重建 DOM',
          good: '仅局部更新 #messages 容器并执行 box.scrollTop = box.scrollHeight',
          detail: '局部更新对话列表可保留输入焦点并避免页面其他表单状态丢失。'
        },
        {
          title: 'Kanban 仅支持鼠标拖拽无法在触屏/键盘下使用',
          bad: '仅绑定 draggable / onDragStart',
          good: '同时提供「← 上一步 / 下一步 →」操作按钮与横向滚动容器',
          detail: '确保移动端和无障碍键盘用户也能流转看板任务。'
        }
      ]
    },
    park: {
      role: '跨框架中性极简设计规范（Park UI + Ark UI + Tailwind / Panda CSS）',
      dualTokens: {
        light: { '--bg': '#fcfcfc', '--surface': '#f4f4f5', '--surface-2': '#e4e4e7', '--border': '#e4e4e7', '--text': '#18181b', '--text-muted': '#71717a', '--accent': '#18181b', '--accent-fg': '#ffffff', '--radius-md': '6px' },
        dark: { '--bg': '#09090b', '--surface': '#121215', '--surface-2': '#18181b', '--border': '#27272a', '--text': '#fafafa', '--text-muted': '#a1a1aa', '--accent': '#fafafa', '--accent-fg': '#09090b', '--radius-md': '6px' }
      },
      designRules: [
        '中性极简主义：通过主题配置统一控制灰度色阶（Neutral / Slate / Zinc / Sand）、圆角阶梯（0px–99px）与品牌 Accent。',
        '多框架共享同一份 Token 规范，保证 React / Vue / Solid 视觉 1:1 一致。'
      ],
      stackRules: [
        '底层使用 Ark UI 状态机原语，样式层可选 Tailwind CSS 或 Panda CSS，严禁在跨框架代码中混写目标框架不支持的语法。'
      ],
      forbidden: [
        '禁止把 React 的 asChild 直接复制到 Vue 或 Solid 模板中。',
        '禁止暗黑模式下沿用深色 Accent（如 #18181b）而不反转 --accent-fg。'
      ],
      pitfalls: [
        {
          title: 'React / Vue / Solid 跨框架语法混淆',
          bad: '在 Vue 中写 <DialogTrigger asChild><button>...</button></DialogTrigger>',
          good: 'React 用 asChild；Vue 用 as-child 或具名插槽；Solid 用 asChild 函数',
          detail: '迁移组件时务必依据目标框架核对 Ark UI 官方 Slot 写法。'
        },
        {
          title: '暗黑模式 Accent 对比度失效（黑底黑字）',
          bad: '[data-theme="dark"] { --accent: #18181b; --accent-fg: #09090b; }',
          good: '深色模式下中性 Accent 自动反转为 #fafafa 配 #09090b 前景，彩色 Accent 配 #ffffff',
          detail: 'Theme Editor 切换主题色时必须同步计算前景色对比度。'
        },
        {
          title: 'Tailwind 与 Panda CSS 变量前缀不匹配',
          bad: '混用 --colors-accent 与 --park-accent 导致样式回退',
          good: '导出 Theme Config 时明确锁定单一 CSS Engine 预设',
          detail: '确保 Token 导出脚本与项目样式引擎完全一致。'
        }
      ]
    },
    basecoat: {
      role: '零 React 依赖的纯 HTML + Tailwind v4 原生复刻规范（Basecoat / shadcn v4 视觉）',
      dualTokens: {
        light: { '--bg': '#ffffff', '--surface': '#fafafa', '--surface-2': '#f4f4f5', '--border': '#e4e4e7', '--text': '#09090b', '--text-muted': '#71717a', '--accent': '#18181b', '--accent-fg': '#fafafa', '--radius-md': '8px' },
        dark: { '--bg': '#09090b', '--surface': '#111113', '--surface-2': '#18181b', '--border': '#27272a', '--text': '#fafafa', '--text-muted': '#a1a1aa', '--accent': '#fafafa', '--accent-fg': '#09090b', '--radius-md': '8px' }
      },
      designRules: [
        '1:1 还原 shadcn/ui v4 的黑白灰极简美学，所有交互状态由标准 HTML5 + ARIA 属性（aria-expanded / aria-checked / aria-selected）+ CSS 驱动。',
        '零框架运行时依赖，适用于纯静态页、Astro、HTMX、Django、Rails、Laravel。'
      ],
      stackRules: [
        '纯 HTML5 + CSS Variables / Tailwind + 原生 Vanilla JS（推荐经典 <script> 确保 file:// 双击离线直接运行，不依赖 ES Module CORS）。'
      ],
      forbidden: [
        '绝对禁止生成 React/JSX 语法（如 className、onClick={}、useState、import React）。',
        '禁止在局部 render() 函数内部重复绑定 document.addEventListener。',
        '禁止使用原生 <select> 或 ES Module（<script type="module"> 在 file:// 会被 CORS 拦截）。'
      ],
      pitfalls: [
        {
          title: '误写 JSX 语法导致纯 HTML 页面直接报错',
          bad: '<button className="btn" onClick={() => open()}>打开</button>',
          good: '<button class="btn" type="button" data-open-modal="demo">打开</button>',
          detail: '纯 HTML/后端模板中必须使用标准 class 与 data-* 属性配合事件绑定。'
        },
        {
          title: '局部重渲染重复绑定 document 全局监听造成内存泄漏',
          bad: 'function render() { document.addEventListener("click", closeMenus); }',
          good: '初始化时全局绑定一次 document.addEventListener("click", closeMenus)',
          detail: '每次重渲染都给 document 加监听器会迅速堆积失效闭包。'
        },
        {
          title: 'file:// 协议下 navigator.clipboard 不可用导致复制静默失败',
          bad: 'navigator.clipboard.writeText(str)（无 catch 降级）',
          good: 'Promise catch 中降级为临时 textarea + document.execCommand("copy")',
          detail: '确保离线双击 HTML 打开时一键复制功能 100% 可用。'
        }
      ]
    },
    radix: {
      role: 'Token 驱动的现代开发工具设计规范（Radix Themes）',
      dualTokens: {
        light: { '--bg': '#ffffff', '--surface': '#f9f9fb', '--surface-2': '#f0f0f3', '--border': '#dcdde0', '--text': '#1c2024', '--text-muted': '#60646c', '--accent': '#3e63dd', '--accent-fg': '#ffffff', '--radius-md': '6px' },
        dark: { '--bg': '#111113', '--surface': '#18191b', '--surface-2': '#212225', '--border': '#2e3135', '--text': '#edeef0', '--text-muted': '#b0b4ba', '--accent': '#3e63dd', '--accent-fg': '#ffffff', '--radius-md': '6px' }
      },
      designRules: [
        '通过顶层 <Theme accentColor grayColor radius scaling appearance> 集中掌控全局字号缩放、圆角、灰阶（Slate/Mauve/Gray）与品牌强调色。',
        '精密字阶（1–9）与考究边框对比度，呈现 Linear / Raycast 级开发工具质感。'
      ],
      stackRules: [
        '优先使用 @radix-ui/themes 提供的布局与排版原语（<Flex>、<Grid>、<Card>、<Text>、<Heading>）及 props（size、variant、radius）。'
      ],
      forbidden: [
        '禁止用大量零散 Tailwind 工具类覆盖 Radix 组件的内边距与圆角。',
        '禁止将 Portal 弹层渲染在 <Theme> 根节点之外。',
        '禁止在暗黑模式选用高明度或单色 Accent 时写死白色文字。'
      ],
      pitfalls: [
        {
          title: 'Tailwind 类名与 <Theme> 全局缩放/圆角打架',
          bad: '<Card className="p-6 rounded-2xl">...</Card>',
          good: '<Card size="3">...</Card>（遵循 <Theme radius scaling> 联动）',
          detail: '混用硬编码间距和圆角会让全局 Theme 控制器失效。'
        },
        {
          title: '暗黑模式下 Mono 强调色按钮变成“黑底黑字”',
          bad: '暗黑模式仍设置 --radix-accent: #1c2024; --accent-fg: #ffffff;',
          good: '暗黑模式 Mono 自动切换为 --radix-accent: #edeef0; --radix-accent-fg: #111113;',
          detail: '切换深浅模式时必须同步校验 Accent 与对比前景色的可读性。'
        },
        {
          title: 'Portal 弹窗脱离 <Theme> 作用域失去字体与色彩变量',
          bad: 'createPortal(<Dialog />, document.body) 放在 <Theme> 外部',
          good: '确保 <Theme> 包裹整个应用根节点（含所有 Dialog/Select Portal）',
          detail: '否则弹窗内的文字、圆角和强调色会全部回退为浏览器默认样式。'
        }
      ]
    },
    reshaped: {
      role: '瑞士网格与 Figma 严格对齐的 Design Token 系统规范（Reshaped）',
      dualTokens: {
        light: { '--bg': '#ffffff', '--surface': '#f5f6f8', '--surface-2': '#ecedf0', '--border': '#dfe2e7', '--text': '#111827', '--text-muted': '#5b6270', '--accent': '#111827', '--accent-fg': '#ffffff', '--radius-md': '8px' },
        dark: { '--bg': '#0f1115', '--surface': '#171a21', '--surface-2': '#20242d', '--border': '#2a2f3a', '--text': '#f9fafb', '--text-muted': '#9ca3af', '--accent': '#f9fafb', '--accent-fg': '#0f1115', '--radius-md': '8px' }
      },
      designRules: [
        '严格遵守 4px 空间基准单元（x1=4px, x2=8px, x4=16px, x6=24px）与 12 列瑞士网格。',
        '使用明确的三层表面高度（elevation-base / elevation-surface / elevation-raised）构建空间层次。'
      ],
      stackRules: [
        '所有色彩、圆角、间距均通过 Figma Variables 1:1 映射的 CSS Variables 引用，禁止任何魔法数字。'
      ],
      forbidden: [
        '禁止出现 13px、15px 等非 4px 倍数的魔法间距或硬编码 #hex 颜色。',
        '禁止暗黑模式下上层卡片（raised）比底层背景（base）更黑。'
      ],
      pitfalls: [
        {
          title: '硬编码间距和颜色破坏 Figma 变量同步',
          bad: 'style={{ padding: "15px", background: "#f5f6f8" }}',
          good: '<View padding={4} backgroundColor="elevation-surface">...</View>',
          detail: '所有空间尺寸必须对齐 4px 单位栅格，色彩必须走语义 Token。'
        },
        {
          title: '暗黑模式表面高度反转（浮层比背景更暗）',
          bad: '暗黑下页面 #171a21，弹窗 #0f1115',
          good: '暗黑下 base #0f1115 → surface #171a21 → raised #20242d（逐级提亮）',
          detail: '符合人眼在暗光下的视觉层次预期，避免浮层塌陷。'
        },
        {
          title: '12 列网格移动端未响应式折叠',
          bad: '<Grid columns={12}><Grid.Item colSpan={4} />',
          good: '<Grid columns={{ s: 1, m: 3 }} gap={4}>...',
          detail: '防止窄屏下 3 列卡片被严重挤压换行。'
        }
      ]
    },
    'base-ui': {
      role: '完全无样式原语底座构建规范（Base UI by MUI）',
      dualTokens: {
        light: { '--bg': '#ffffff', '--surface': '#fafafa', '--surface-2': '#f4f4f5', '--border': '#e4e4e7', '--text': '#09090b', '--text-muted': '#71717a', '--accent': '#09090b', '--accent-fg': '#ffffff', '--radius-md': '6px' },
        dark: { '--bg': '#09090b', '--surface': '#111113', '--surface-2': '#18181b', '--border': '#27272a', '--text': '#fafafa', '--text-muted': '#a1a1aa', '--accent': '#fafafa', '--accent-fg': '#09090b', '--radius-md': '6px' }
      },
      designRules: [
        'Base UI 本身 0 预设样式（Unstyled），所有视觉呈现完全由自定义 Tailwind / CSS 样式层接管，必须显式编写完整状态样式。'
      ],
      stackRules: [
        '从 @base-ui-components/react/<part> 按需引入（如 Switch.Root / Select.Root / Dialog.Root），利用 data-* 状态属性与 render prop 构建自定义 UI 库。'
      ],
      forbidden: [
        '禁止只引入 Base UI 原语却不编写 className / CSS 样式层。',
        '禁止在自定义封装组件时吞掉 {...props} 透传（会导致 ARIA 与键盘失效）。',
        '禁止使用已废弃的 Radix asChild 属性。'
      ],
      pitfalls: [
        {
          title: '误以为 Base UI 自带默认视觉样式',
          bad: 'import { Switch } from "@base-ui-components/react/switch"; <Switch.Root />',
          good: '必须为 Switch.Root 与 Switch.Thumb 编写完整的尺寸、圆角、背景与 data-[checked] 样式',
          detail: 'Base UI 是无头行为原语，不写样式会呈现为浏览器裸奔控件。'
        },
        {
          title: '自定义封装组件时丢失 {...props} 透传',
          bad: 'const MyTrigger = ({ children }) => <button>{children}</button>',
          good: 'render={(props) => <button {...props} className="...">{children}</button>}',
          detail: '丢失 props 透传会破坏 aria-expanded、ref 定位与键盘监听。'
        },
        {
          title: '仍使用 Radix asChild 导致宿主替换失败',
          bad: '<Menu.Trigger asChild><Button /></Menu.Trigger>',
          good: '<Menu.Trigger render={<Button />} />',
          detail: 'Base UI 全系使用 render 属性替代 asChild。'
        }
      ]
    },
    'react-aria': {
      role: 'Adobe 顶级无障碍与复杂交互组件规范（React Aria Components）',
      dualTokens: {
        light: { '--bg': '#ffffff', '--surface': '#f8fafc', '--surface-2': '#f1f5f9', '--border': '#cbd5e1', '--text': '#0f172a', '--text-muted': '#475569', '--accent': '#0f172a', '--accent-fg': '#ffffff', '--radius-md': '6px', '--ring': '0 0 0 2px #fff, 0 0 0 4px #2563eb' },
        dark: { '--bg': '#020617', '--surface': '#0f172a', '--surface-2': '#1e293b', '--border': '#1e293b', '--text': '#f8fafc', '--text-muted': '#94a3b8', '--accent': '#f8fafc', '--accent-fg': '#0f172a', '--radius-md': '6px', '--ring': '0 0 0 2px #020617, 0 0 0 4px #60a5fa' }
      },
      designRules: [
        '样式完全可定制，但必须将焦点可见性（Focus-Visible 高对比度双环轮廓）作为一等公民，保证纯键盘与读屏用户体验。'
      ],
      stackRules: [
        '复杂表单与数据组件（Calendar、DateRangePicker、ComboBox、Table 排序/多选）优先采用 react-aria-components，配合 data-focus-visible / data-selected 编写样式。'
      ],
      forbidden: [
        '严禁全局编写 *:focus { outline: none } 抹除键盘焦点环。',
        '严禁无文字的图标按钮、日历、ComboBox 缺少 aria-label。',
        '严禁在 React Aria Button 上用 onClick 替代 onPress。'
      ],
      pitfalls: [
        {
          title: '全局 outline: none 导致键盘导航彻底失明',
          bad: 'button:focus { outline: none; }',
          good: 'button:focus-visible, [data-focus-visible] { outline: none; box-shadow: var(--ring); }',
          detail: '可用 :focus-visible 隐藏鼠标点击框，但必须保留高对比度键盘焦点环。'
        },
        {
          title: '表格全选未同步 indeterminate 且误选隐藏行',
          bad: '点击全选时选中全部 rawRows，子行取消时表头仍打勾',
          good: '全选仅作用于当前过滤后的 visibleRows，并实时同步 checked 与 indeterminate',
          detail: '保证过滤搜索后的批量操作符合直觉且无障碍播报准确。'
        },
        {
          title: '使用 onClick 替代 onPress',
          bad: '<Button onClick={handleSave}>保存</Button>',
          good: '<Button onPress={handleSave}>保存</Button>',
          detail: 'onPress 自动处理鼠标、触摸、键盘 Enter/Space 与防双触，兼容全部无障碍设备。'
        }
      ]
    }
  },
  tremorBarData: [
    { day: 'Mon', val: 68 },
    { day: 'Tue', val: 82 },
    { day: 'Wed', val: 54 },
    { day: 'Thu', val: 94 },
    { day: 'Fri', val: 76 },
    { day: 'Sat', val: 42 },
    { day: 'Sun', val: 61 }
  ],
  tremorDonutData: [
    { label: 'Compute 算力', pct: 48, color: 'var(--accent)' },
    { label: 'Storage 存储', pct: 31, color: '#3b82f6' },
    { label: 'Bandwidth 带宽', pct: 21, color: '#10b981' }
  ],
  timelineItems: [
    { time: '15:20', title: '发布 Design Tokens v2.4', desc: '同步 Light / Dark 双色变量与 4px 间距标尺', status: 'success' },
    { time: '14:05', title: '迁移 Select 为自定义 Listbox', desc: '移除全部原生 <select>，补全键盘上下键支持', status: 'info' },
    { time: '11:40', title: 'A11y 对比度回归检测', desc: '修复 Radix 暗黑模式下 Mono 按钮可读性告警', status: 'warn' }
  ],
  wizardSteps: [
    { step: 1, title: '定义 Tokens', desc: '确立黑白灰背景、边框对比度与圆角阶梯变量' },
    { step: 2, title: '封装无头原语', desc: '基于 Base UI / React Aria 绑定键盘与 ARIA 状态' },
    { step: 3, title: '补全边界态', desc: '覆盖 Empty / Loading / Error 与异步反馈交互' },
    { step: 4, title: '离线门禁验证', desc: '确认零外链依赖、无控制台报错后打包发布' }
  ],
  timeSlots: ['09:00', '09:30', '10:00', '10:30', '11:30', '13:30', '14:30', '15:30', '16:30', '18:00']
};
