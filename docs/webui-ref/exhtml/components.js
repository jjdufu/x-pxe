window.ShowcaseComponents = (function () {
  const D = window.SHOWCASE_DATA;

  function stars(n) {
    return '★'.repeat(n) + '☆'.repeat(5 - n);
  }

  function renderHero(style) {
    const cat = D.categories.find(c => c.id === style.category);
    return `
      <section class="style-hero">
        <div class="hero-top">
          <div>
            <div class="hero-badges">
              <span class="ui-badge ui-badge-solid">#${style.num}</span>
              <span class="ui-badge">${cat ? cat.label : ''}</span>
              ${style.traits.map(t => `<span class="ui-badge">${t}</span>`).join('')}
            </div>
            <h1 class="hero-title">${style.name} <span style="font-weight:400;font-size:15px;color:var(--text-muted)">— ${style.subtitle}</span></h1>
            <p class="hero-sub">${style.summary}</p>
          </div>
          <div class="row-gap">
            <a class="ui-btn ui-btn-outline ui-btn-sm" href="${style.url}" target="_blank" rel="noopener noreferrer">
              官方文档 ↗
            </a>
            <button class="ui-btn ui-btn-primary ui-btn-sm" data-action="toast" data-toast-title="已复制 ${style.name} 风格配置" data-toast-desc="当前 CSS Variables 已就绪，可在顶部 Tokens 栏查看。">
              应用 / 导出风格
            </button>
          </div>
        </div>
        <div class="hero-meta-row">
          <span><strong>技术栈：</strong>${style.stack}</span>
          <span><strong>移植/重构性：</strong><span style="letter-spacing:1px">${stars(style.rating)}</span></span>
          <span><strong>当前主题：</strong><span id="heroThemeBadge">浅色模式</span></span>
        </div>
      </section>
    `;
  }

  function renderSignature(style, state) {
    let body = '';

    if (style.id === 'coss') {
      const slots = ['09:00', '09:30', '10:30', '13:30', '15:00', '16:30'];
      body = `
        <div class="grid-2">
          <div class="ui-card">
            <div class="card-label"><span>Cal.com 风格预约时段选择器</span><span class="ui-badge">Base UI</span></div>
            <div class="row-between" style="margin-bottom:12px">
              <div>
                <div style="font-weight:600;font-size:15px">30 分钟架构评审</div>
                <div class="muted-text">Asia/Shanghai · 视频会议</div>
              </div>
              <div class="tabs-bar">
                <button class="tab-btn ${state.cossDuration === '15m' ? 'active' : ''}" data-coss-dur="15m">15m</button>
                <button class="tab-btn ${state.cossDuration === '30m' ? 'active' : ''}" data-coss-dur="30m">30m</button>
                <button class="tab-btn ${state.cossDuration === '60m' ? 'active' : ''}" data-coss-dur="60m">60m</button>
              </div>
            </div>
            <div style="display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin-bottom:14px">
              ${slots.map(s => `
                <button class="ui-btn ${state.cossSlot === s ? 'ui-btn-primary' : 'ui-btn-outline'} ui-btn-sm tabular" data-coss-slot="${s}">${s}</button>
              `).join('')}
            </div>
            <div class="row-between">
              <span class="muted-text">已选时段：<strong>${state.cossSlot}</strong> (${state.cossDuration})</span>
              <button class="ui-btn ui-btn-primary ui-btn-sm" data-action="toast" data-toast-title="预约已确认" data-toast-desc="已锁定 ${state.cossSlot} · 时长 ${state.cossDuration}">确认预约</button>
            </div>
          </div>
          <div class="ui-card ui-card-surface">
            <div class="card-label"><span>${style.snippetTitle}</span><span class="mono" style="font-size:11px">TSX</span></div>
            <pre class="code-block">${escapeHtml(style.snippet)}</pre>
          </div>
        </div>
      `;
    } else if (style.id === 'untitled') {
      const mode = state.untitledEdge;
      let edgeView = '';
      if (mode === 'empty') {
        edgeView = `
          <div style="text-align:center;padding:24px 12px">
            <div style="width:44px;height:44px;border-radius:10px;border:1px solid var(--border);display:inline-grid;place-items:center;margin-bottom:10px;background:var(--surface)">📂</div>
            <div style="font-weight:600;margin-bottom:4px">暂无匹配的工作区记录</div>
            <p class="muted-text" style="margin-bottom:14px">当前筛选条件下没有找到项目，你可以重置筛选或新建。</p>
            <div class="row-gap" style="justify-content:center">
              <button class="ui-btn ui-btn-outline ui-btn-sm" data-untitled-edge="populated">恢复数据</button>
              <button class="ui-btn ui-btn-primary ui-btn-sm" data-action="open-modal" data-modal="demoModal">新建项目</button>
            </div>
          </div>`;
      } else if (mode === 'loading') {
        edgeView = `
          <div style="display:flex;flex-direction:column;gap:10px;padding:12px 0">
            <div class="skeleton" style="height:18px;width:45%"></div>
            <div class="skeleton" style="height:14px;width:85%"></div>
            <div class="skeleton" style="height:42px;width:100%"></div>
            <div class="skeleton" style="height:42px;width:100%"></div>
          </div>`;
      } else if (mode === 'error') {
        edgeView = `
          <div class="ui-alert ui-alert-danger" style="margin:8px 0">
            <div class="ui-alert-title">请求超时 (ERR_CONNECTION_TIMED_OUT)</div>
            <div class="ui-alert-desc" style="margin-bottom:10px">无法连接至企业数据网关，请检查网络或重试。</div>
            <button class="ui-btn ui-btn-danger ui-btn-sm" data-untitled-edge="populated">重新加载</button>
          </div>`;
      } else {
        edgeView = `
          <div style="display:flex;flex-direction:column;gap:8px">
            <div class="row-between" style="padding:10px;border:1px solid var(--border);border-radius:var(--radius-md)">
              <div><strong>Enterprise SSO</strong><div class="muted-text">SAML 2.0 · 已启用</div></div>
              <span class="ui-badge ui-badge-success"><span class="badge-dot"></span>健康</span>
            </div>
            <div class="row-between" style="padding:10px;border:1px solid var(--border);border-radius:var(--radius-md)">
              <div><strong>Audit Log Stream</strong><div class="muted-text">保留 365 天 · 加密存储</div></div>
              <span class="ui-badge">同步中</span>
            </div>
          </div>`;
      }
      body = `
        <div class="grid-2">
          <div class="ui-card">
            <div class="card-label">
              <span>边界状态切换器 (Edge States)</span>
              <div class="tabs-bar">
                <button class="tab-btn ${mode === 'populated' ? 'active' : ''}" data-untitled-edge="populated">正常</button>
                <button class="tab-btn ${mode === 'empty' ? 'active' : ''}" data-untitled-edge="empty">空状态</button>
                <button class="tab-btn ${mode === 'loading' ? 'active' : ''}" data-untitled-edge="loading">加载态</button>
                <button class="tab-btn ${mode === 'error' ? 'active' : ''}" data-untitled-edge="error">错误态</button>
              </div>
            </div>
            ${edgeView}
          </div>
          <div class="ui-card ui-card-surface">
            <div class="card-label"><span>${style.snippetTitle}</span><span class="mono" style="font-size:11px">TSX</span></div>
            <pre class="code-block">${escapeHtml(style.snippet)}</pre>
          </div>
        </div>
      `;
    } else if (style.id === 'tremor') {
      const series = D.tremorSeries;
      const max = Math.max(...series);
      const pts = series.map((v, i) => `${(i / (series.length - 1)) * 480},${110 - (v / max) * 90}`).join(' ');
      body = `
        <div class="grid-4" style="margin-bottom:16px">
          <div class="ui-card">
            <div class="muted-text">总请求量 (30d)</div>
            <div class="tabular" style="font-size:24px;font-weight:700;margin:4px 0">4,829,104</div>
            <span class="ui-badge ui-badge-success">+14.2%</span>
          </div>
          <div class="ui-card">
            <div class="muted-text">P99 延迟</div>
            <div class="tabular" style="font-size:24px;font-weight:700;margin:4px 0">28.4 ms</div>
            <span class="ui-badge ui-badge-success">-4.1 ms</span>
          </div>
          <div class="ui-card">
            <div class="muted-text">错误率</div>
            <div class="tabular" style="font-size:24px;font-weight:700;margin:4px 0">0.02%</div>
            <span class="ui-badge">SLA 达标</span>
          </div>
          <div class="ui-card">
            <div class="muted-text">活跃边缘节点</div>
            <div class="tabular" style="font-size:24px;font-weight:700;margin:4px 0">38 / 38</div>
            <span class="ui-badge ui-badge-solid">100% 在线</span>
          </div>
        </div>
        <div class="grid-2" style="margin-bottom:16px">
          <div class="ui-card">
            <div class="card-label"><span>交互式吞吐趋势图 (SVG Area)</span><span class="tabular" id="tremorHoverVal">悬停节点查看数值</span></div>
            <svg viewBox="0 0 480 120" style="width:100%;height:130px;overflow:visible">
              <polyline fill="none" stroke="var(--accent)" stroke-width="2" points="${pts}" />
              ${series.map((v, i) => {
                const cx = (i / (series.length - 1)) * 480;
                const cy = 110 - (v / max) * 90;
                return `<circle cx="${cx}" cy="${cy}" r="4.5" fill="var(--bg)" stroke="var(--accent)" stroke-width="2" style="cursor:pointer" data-tremor-pt="${D.tremorMonths[i]}: ${v}k req/s" />`;
              }).join('')}
            </svg>
          </div>
          <div class="ui-card">
            <div class="card-label"><span>30 天服务健康追踪条 (Tracker)</span><span class="tabular">99.84% Uptime</span></div>
            <div class="tracker-bar" style="margin:18px 0 12px">
              ${D.trackerDays.map(d => `<div class="tracker-cell ${d.status}" title="${d.day} · 可用率 ${d.uptime}"></div>`).join('')}
            </div>
            <div class="row-between muted-text" style="margin-bottom:16px">
              <span>30 天前</span>
              <span id="trackerHint">将鼠标悬停在色块上查看每日状态</span>
              <span>今日</span>
            </div>
            <div class="card-label" style="margin-bottom:8px"><span>资源分布条 (CategoryBar)</span><span class="tabular">100% 配额</span></div>
            <div class="cat-bar-track">
              ${D.tremorDonutData.map(seg => `<div class="cat-bar-seg" style="width:${seg.pct}%;background:${seg.color}" title="${seg.label}: ${seg.pct}%"></div>`).join('')}
            </div>
          </div>
        </div>
        <div class="grid-2">
          <div class="ui-card">
            <div class="card-label"><span>7 天请求柱状图 (BarChart)</span><span class="tabular" id="tremorBarVal">悬停柱条查看</span></div>
            <div class="bar-chart-grid">
              ${D.tremorBarData.map(b => `
                <div class="bar-col-wrap">
                  <div class="bar-col" style="height:${b.val}%" data-tremor-bar="${b.day}: ${b.val}k req"></div>
                  <span class="muted-text tabular" style="font-size:11px">${b.day}</span>
                </div>
              `).join('')}
            </div>
          </div>
          <div class="ui-card">
            <div class="card-label"><span>算力占用环形图 (DonutChart)</span><span class="tabular" id="tremorDonutVal">总计 100%</span></div>
            <div class="row-gap" style="gap:20px;align-items:center">
              <svg viewBox="0 0 100 100" style="width:104px;height:104px;flex-shrink:0">
                ${(() => {
                  let offset = 0;
                  const r = 36;
                  const c = 2 * Math.PI * r;
                  return D.tremorDonutData.map(seg => {
                    const dash = (seg.pct / 100) * c;
                    const el = `<circle cx="50" cy="50" r="${r}" fill="none" stroke="${seg.color}" stroke-width="14" stroke-dasharray="${dash} ${c - dash}" stroke-dashoffset="${-offset}" transform="rotate(-90 50 50)" style="cursor:pointer" data-tremor-donut="${seg.label}: ${seg.pct}%" />`;
                    offset += dash;
                    return el;
                  }).join('');
                })()}
                <text x="50" y="54" text-anchor="middle" fill="var(--text)" font-size="13" font-weight="700" class="tabular">100%</text>
              </svg>
              <div style="display:flex;flex-direction:column;gap:8px;flex:1">
                ${D.tremorDonutData.map(seg => `
                  <div class="row-between" style="font-size:12.5px">
                    <span class="row-gap"><span class="token-swatch" style="background:${seg.color}"></span>${seg.label}</span>
                    <strong class="tabular">${seg.pct}%</strong>
                  </div>
                `).join('')}
              </div>
            </div>
          </div>
        </div>
      `;
    } else if (style.id === 'kibo') {
      body = `
        <div class="ui-card" style="margin-bottom:16px">
          <div class="card-label"><span>可交互 Kanban 看板 (点击左右箭头流转任务)</span><span class="ui-badge">Kibo Kanban</span></div>
          <div class="kanban-grid">
            ${state.kanban.map((col, ci) => `
              <div class="kanban-col">
                <div class="kanban-col-head">
                  <span>${col.title}</span>
                  <span class="ui-badge tabular">${col.items.length}</span>
                </div>
                ${col.items.map(item => `
                  <div class="kanban-card">
                    <div class="row-between">
                      <span class="mono" style="font-size:11px;color:var(--text-muted)">${item.id}</span>
                      <span class="ui-badge">${item.tag}</span>
                    </div>
                    <div class="kanban-card-title">${item.title}</div>
                    <div class="kanban-move-btns">
                      ${ci > 0 ? `<button class="ui-btn ui-btn-outline ui-btn-sm" data-kanban-move="${item.id}" data-dir="-1">← 上一步</button>` : ''}
                      ${ci < 2 ? `<button class="ui-btn ui-btn-outline ui-btn-sm" data-kanban-move="${item.id}" data-dir="1">下一步 →</button>` : ''}
                    </div>
                  </div>
                `).join('')}
              </div>
            `).join('')}
          </div>
        </div>
        <div class="grid-3">
          <div class="ui-card">
            <div class="card-label"><span>甘特图时间线 (Gantt)</span></div>
            ${D.ganttTasks.map(t => `
              <div class="gantt-row">
                <span style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${t.name}</span>
                <div class="gantt-track">
                  <div class="gantt-bar" style="grid-column:${t.start + 1} / span ${t.span}">${t.owner}</div>
                </div>
              </div>
            `).join('')}
          </div>
          <div class="ui-card">
            <div class="card-label"><span>树形目录 (Tree View)</span></div>
            ${renderTreeNodes(D.treeData)}
          </div>
          <div class="ui-card">
            <div class="card-label"><span>AI 对话控件 (AI Prompt)</span></div>
            <div class="ai-chat-box">
              <div class="ai-messages" id="kiboAiMessages">
                ${state.aiMessages.map(m => `<div class="ai-msg ${m.role}">${escapeHtml(m.text)}</div>`).join('')}
              </div>
              <div class="input-group">
                <input type="text" id="kiboAiInput" placeholder="输入指令生成组件..." />
                <button class="ui-btn ui-btn-primary ui-btn-sm" id="kiboAiSendBtn" type="button" style="border-radius:0">发送</button>
              </div>
            </div>
          </div>
        </div>
      `;
    } else if (style.id === 'park') {
      const fwSnippets = {
        react: `import { Button } from '~/components/ui/button'\n<Button variant="solid">Park UI React</Button>`,
        vue: `<script setup>\nimport { Button } from '~/components/ui/button'\n</script>\n<template><Button variant="solid">Park UI Vue</Button></template>`,
        solid: `import { Button } from '~/components/ui/button'\nexport const App = () => <Button variant="solid">Park UI Solid</Button>`
      };
      body = `
        <div class="grid-2">
          <div class="ui-card">
            <div class="card-label"><span>在线 Theme Editor (实时调控全局外观)</span><span class="ui-badge">Ark UI</span></div>
            <div class="form-field">
              <label class="ui-label">圆角阶梯 (Border Radius)</label>
              <div class="tabs-bar">
                ${[['0px', 'None'], ['4px', 'Sm'], ['6px', 'Md'], ['12px', 'Lg'], ['99px', 'Full']].map(([val, lbl]) => `
                  <button class="tab-btn ${state.parkRadius === val ? 'active' : ''}" data-park-radius="${val}">${lbl}</button>
                `).join('')}
              </div>
            </div>
            <div class="form-field">
              <label class="ui-label">品牌强调色 (Accent)</label>
              <div class="row-gap">
                ${[['#18181b', 'Zinc'], ['#0f172a', 'Slate'], ['#2563eb', 'Blue'], ['#059669', 'Emerald'], ['#7c3aed', 'Violet']].map(([hex, name]) => `
                  <button class="ui-btn ui-btn-outline ui-btn-sm" data-park-accent="${hex}">
                    <span class="token-swatch" style="background:${hex}"></span>${name}
                  </button>
                `).join('')}
              </div>
            </div>
          </div>
          <div class="ui-card ui-card-surface">
            <div class="card-label">
              <span>跨框架代码导出</span>
              <div class="tabs-bar">
                <button class="tab-btn ${state.parkFw === 'react' ? 'active' : ''}" data-park-fw="react">React</button>
                <button class="tab-btn ${state.parkFw === 'vue' ? 'active' : ''}" data-park-fw="vue">Vue</button>
                <button class="tab-btn ${state.parkFw === 'solid' ? 'active' : ''}" data-park-fw="solid">Solid</button>
              </div>
            </div>
            <pre class="code-block">${escapeHtml(fwSnippets[state.parkFw])}</pre>
          </div>
        </div>
      `;
    } else if (style.id === 'basecoat') {
      const showRaw = state.basecoatShowHtml;
      body = `
        <div class="grid-2">
          <div class="ui-card">
            <div class="card-label"><span>零 React 依赖 · 纯 HTML + 原生 JS</span><span class="ui-badge ui-badge-success">0 KB Framework</span></div>
            <p class="muted-text" style="margin-bottom:14px">直接复制标准语义化 HTML 与 Tailwind 类名，适用于 Astro、HTMX、Laravel、Rails、Django 或当前原生网页。</p>
            ${showRaw ? `<pre class="code-block" style="margin-bottom:12px">${escapeHtml('<button class="btn" type="button">原生 Button</button>\n<div role="status" class="toast">...</div>')}</pre>` : ''}
            <div class="row-gap">
              <button class="ui-btn ui-btn-primary" data-action="toast" data-toast-title="原生 HTML 交互触发" data-toast-desc="零构建步骤，直接运行在浏览器。">原生 Button</button>
              <button class="ui-btn ui-btn-outline" id="toggleBasecoatHtmlBtn">${showRaw ? '隐藏 HTML 源码' : '查看 HTML 源码'}</button>
            </div>
          </div>
          <div class="ui-card ui-card-surface">
            <div class="card-label"><span>${style.snippetTitle}</span><span class="mono" style="font-size:11px">HTML5</span></div>
            <pre class="code-block">${escapeHtml(style.snippet)}</pre>
          </div>
        </div>
      `;
    } else if (style.id === 'radix') {
      body = `
        <div class="grid-2">
          <div class="ui-card">
            <div class="card-label"><span>&lt;Theme&gt; 全局控制器</span><span class="ui-badge">Token Driven</span></div>
            <div class="form-field">
              <label class="ui-label">Accent Color</label>
              <div class="row-gap">
                ${[['#3e63dd', 'Indigo'], ['#0090ff', 'Blue'], ['#29a383', 'Jade'], ['#e5484d', 'Crimson'], ['#1c2024', 'Mono']].map(([hex, lbl]) => `
                  <button class="ui-btn ui-btn-outline ui-btn-sm" data-radix-accent="${hex}">
                    <span class="token-swatch" style="background:${hex}"></span>${lbl}
                  </button>
                `).join('')}
              </div>
            </div>
            <div class="grid-2">
              <div class="form-field">
                <label class="ui-label">Radius</label>
                <div class="tabs-bar">
                  ${[['0px', 'None'], ['4px', 'Sm'], ['6px', 'Md'], ['10px', 'Lg']].map(([r, lbl]) => `
                    <button class="tab-btn ${state.radixRadius === r ? 'active' : ''}" data-radix-radius="${r}">${lbl}</button>
                  `).join('')}
                </div>
              </div>
              <div class="form-field">
                <label class="ui-label">Scaling (${Math.round(state.radixScale * 100)}%)</label>
                <div class="tabs-bar">
                  ${[['0.9', '90%'], ['1', '100%'], ['1.08', '108%']].map(([sc, lbl]) => `
                    <button class="tab-btn ${String(state.radixScale) === sc ? 'active' : ''}" data-radix-scale="${sc}">${lbl}</button>
                  `).join('')}
                </div>
              </div>
            </div>
          </div>
          <div class="ui-card ui-card-surface">
            <div class="card-label"><span>${style.snippetTitle}</span><span class="mono" style="font-size:11px">JSX</span></div>
            <pre class="code-block">${escapeHtml(style.snippet)}</pre>
          </div>
        </div>
      `;
    } else if (style.id === 'reshaped') {
      body = `
        <div class="ui-card">
          <div class="card-label">
            <span>瑞士 12 列网格与 Surface 层级系统</span>
            <button class="ui-btn ui-btn-outline ui-btn-sm" id="toggleSwissGridBtn">
              ${state.showSwissGrid ? '隐藏红线网格' : '显示 12 列网格叠加层'}
            </button>
          </div>
          <div class="swiss-grid-demo ${state.showSwissGrid ? 'show-overlay' : ''}" style="padding:12px;border:1px solid var(--border);border-radius:var(--radius-md)">
            <div class="ui-card" style="grid-column:span 4;padding:12px"><strong>4 Cols</strong><div class="muted-text">elevation-base</div></div>
            <div class="ui-card ui-card-surface" style="grid-column:span 4;padding:12px"><strong>4 Cols</strong><div class="muted-text">elevation-surface</div></div>
            <div class="ui-card" style="grid-column:span 4;padding:12px;box-shadow:var(--shadow-sm)"><strong>4 Cols</strong><div class="muted-text">elevation-raised</div></div>
          </div>
        </div>
      `;
    } else if (style.id === 'base-ui') {
      body = `
        <div class="grid-2">
          <div class="ui-card">
            <div class="card-label">
              <span>无样式原语 vs 精装样式 (实时对比)</span>
              <button class="ui-btn ${state.baseUiUnstyled ? 'ui-btn-danger' : 'ui-btn-primary'} ui-btn-sm" id="toggleUnstyledBtn">
                ${state.baseUiUnstyled ? '当前：裸奔无样式原语 (点击恢复)' : '切换为无样式原语 (Unstyled)'}
              </button>
            </div>
            <div class="unstyled-zone" data-unstyled="${state.baseUiUnstyled}">
              <div class="row-gap" style="margin-bottom:10px">
                <button class="ui-btn ui-btn-primary" data-base-log="Button.onClick">Primary Action</button>
                <button class="ui-btn ui-btn-outline" data-base-log="Dialog.Trigger">Open Trigger</button>
                <label class="check-row"><input type="checkbox" class="ui-checkbox" checked data-base-log="Checkbox.onCheckedChange" /> 原生 A11y 复选框</label>
              </div>
              <input class="ui-input" type="text" value="Base UI Input Primitive" data-base-log="Input.onFocus" />
            </div>
          </div>
          <div class="ui-card ui-card-surface">
            <div class="card-label"><span>原语状态与事件日志</span><span class="mono" style="font-size:11px">ARIA State</span></div>
            <div class="code-block" id="baseUiLogBox" style="min-height:110px">${state.baseUiLogs.join('\n')}</div>
          </div>
        </div>
      `;
    } else if (style.id === 'react-aria') {
      body = `
        <div class="grid-2">
          <div class="ui-card">
            <div class="card-label"><span>无障碍键盘交互与读屏播报器 (ARIA Live Region)</span><span class="ui-badge">Apache-2.0</span></div>
            <p class="muted-text" style="margin-bottom:12px">尝试使用 <kbd>Tab</kbd> 键聚焦下方控件，观察高对比度 Focus-Visible 轮廓与右侧实时屏幕阅读器播报。</p>
            <div class="row-gap">
              <button class="ui-btn ui-btn-primary" data-aria-announce="已激活按钮：提交无障碍表单">可聚焦按钮</button>
              <button class="ui-btn ui-btn-outline" data-aria-announce="菜单已展开，共 3 个选项，使用上下方向键导航">ComboBox 触发器</button>
              <button type="button" class="ui-switch" role="switch" aria-checked="true" data-switch data-aria-announce="开关状态已切换"></button>
            </div>
          </div>
          <div class="ui-card ui-card-surface">
            <div class="card-label"><span>屏幕阅读器实时播报 (Screen Reader Log)</span></div>
            <div class="ui-alert ui-alert-info" aria-live="polite" id="ariaAnnouncerBox">
              <div class="ui-alert-title">ARIA Live Announcer</div>
              <div class="ui-alert-desc">${state.ariaAnnouncement}</div>
            </div>
          </div>
        </div>
      `;
    }

    return `
      <section class="showcase-section" id="sec-signature">
        <div class="section-head">
          <h2 class="section-title">专属风格特性与核心模块</h2>
          <p class="section-desc">针对 ${style.name} 设计语言定制的标志性交互组件</p>
        </div>
        ${body}
      </section>
    `;
  }

  function renderTokensSpec(style) {
    const swatches = [
      ['--bg', '画布背景'],
      ['--surface', '次级表面'],
      ['--surface-2', '交互填充'],
      ['--border', '默认边框'],
      ['--accent', '主强调色'],
      ['--danger', '危险语义'],
      ['--success', '成功语义'],
      ['--warning', '警告语义'],
      ['--info', '信息语义']
    ];
    const spacings = [
      ['1x', '4px'], ['2x', '8px'], ['3x', '12px'],
      ['4x', '16px'], ['6x', '24px'], ['8x', '32px'], ['12x', '48px']
    ];
    return `
      <section class="showcase-section" id="sec-tokens-spec">
        <div class="section-head">
          <div>
            <h2 class="section-title">CSS 设计规范标尺标本 (Design Tokens Specimen)</h2>
            <p class="section-desc">实时反映 ${style.name} 在当前明暗模式下的字体阶梯、色板、间距、圆角、投影与焦点环规范</p>
          </div>
          <button type="button" class="ui-btn ui-btn-outline ui-btn-sm" id="exportFullCssBtn">导出双色 CSS Tokens</button>
        </div>
        <div class="grid-2">
          <!-- Typography & Swatches -->
          <div class="ui-card">
            <div class="card-label"><span>排版字阶标本 (Typography Scale)</span><span class="mono" style="font-size:11px">tracking &amp; tabular</span></div>
            <div class="spec-typo-row">
              <span style="font-size:26px;font-weight:700;letter-spacing:var(--tracking-tight)">Display / H1</span>
              <span class="mono muted-text">26px · 700 · tight</span>
            </div>
            <div class="spec-typo-row">
              <span style="font-size:20px;font-weight:600;letter-spacing:var(--tracking-tight)">Heading / H2</span>
              <span class="mono muted-text">20px · 600</span>
            </div>
            <div class="spec-typo-row">
              <span style="font-size:15px;font-weight:600">Subhead / H3</span>
              <span class="mono muted-text">15px · 600</span>
            </div>
            <div class="spec-typo-row">
              <span style="font-size:13.5px">Body 正文文本与表单内容</span>
              <span class="mono muted-text">13.5px · 400 · 1.5</span>
            </div>
            <div class="spec-typo-row">
              <span class="muted-text" style="font-size:12px">Caption / 辅助说明与时间戳</span>
              <span class="mono muted-text">12px · muted</span>
            </div>
            <div class="spec-typo-row">
              <span class="tabular mono" style="font-size:13px;font-weight:600">012,345.67 ms · 99.98%</span>
              <span class="mono muted-text">tabular-nums · mono</span>
            </div>
          </div>

          <!-- Semantic Color Swatches -->
          <div class="ui-card">
            <div class="card-label"><span>语义色板与表面层级 (Color &amp; Elevation)</span><span class="ui-badge">Live CSS Vars</span></div>
            <div class="spec-swatch-grid">
              ${swatches.map(([v, label]) => `
                <div class="spec-swatch-card">
                  <div class="spec-swatch-box" style="background:var(${v})"></div>
                  <div class="mono" style="font-weight:600">${v}</div>
                  <div class="muted-text" style="font-size:11px">${label}</div>
                </div>
              `).join('')}
            </div>
          </div>

          <!-- Spacing Scale -->
          <div class="ui-card">
            <div class="card-label"><span>空间间距标尺 (4px 基准栅格)</span><span class="mono" style="font-size:11px">4px – 48px</span></div>
            ${spacings.map(([unit, px]) => `
              <div class="spec-spacing-row">
                <span class="mono" style="width:36px">${unit}</span>
                <span class="mono muted-text tabular" style="width:42px">${px}</span>
                <div class="spec-spacing-bar" style="width:${parseInt(px, 10) * 4}px"></div>
              </div>
            `).join('')}
          </div>

          <!-- Radius, Shadow & Focus Ring -->
          <div class="ui-card">
            <div class="card-label"><span>圆角阶梯、投影与 Focus-Visible 环</span></div>
            <div class="spec-radius-grid" style="margin-bottom:14px">
              <div class="spec-radius-box" style="border-radius:var(--radius-sm)">sm</div>
              <div class="spec-radius-box" style="border-radius:var(--radius-md)">md</div>
              <div class="spec-radius-box" style="border-radius:var(--radius-lg)">lg</div>
              <div class="spec-radius-box" style="border-radius:9999px">full</div>
            </div>
            <div class="grid-3" style="gap:8px;margin-bottom:14px">
              <div class="spec-radius-box" style="box-shadow:var(--shadow-xs);height:42px">shadow-xs</div>
              <div class="spec-radius-box" style="box-shadow:var(--shadow-sm);height:42px">shadow-sm</div>
              <div class="spec-radius-box" style="box-shadow:var(--shadow-md);height:42px">shadow-md</div>
            </div>
            <div class="row-between">
              <span class="muted-text">焦点环测试（点击或 Tab 聚焦右侧按钮）：</span>
              <button type="button" class="ui-btn ui-btn-outline ui-btn-sm" style="box-shadow:var(--ring)">Focus Ring 预览</button>
            </div>
          </div>
        </div>
      </section>
    `;
  }

  function buildAgentPrompt(style, state) {
    const guide = D.promptGuides[style.id];
    if (!guide) return '';
    const modeDesc = {
      greenfield: '从零构建全新界面，严格按下列设计 Token 与组件原语交付完整可交互代码。',
      refactor: '将现有项目（如原生 shadcn/ui、Tailwind 杂糅代码或旧组件库）安全重构迁移至该设计系统，保留业务逻辑的同时消除视觉与 API 冲突。',
      setup: '初始化设计系统基础设施：建立全局 CSS Tokens、基础控件封装与工程约束，确保后续业务页面直接复用。'
    };

    const lines = [];
    lines.push(`# [UI/UX SYSTEM PROMPT] ${style.name} (${style.subtitle})`);
    lines.push(`- 角色与目标：${guide.role}`);
    lines.push(`- 任务模式：${modeDesc[state.promptMode] || modeDesc.greenfield}`);
    lines.push(`- 技术栈基线：${style.stack}`);
    lines.push('');

    lines.push('## 1. 硬性边界约束 (MUST / NEVER)');
    guide.designRules.forEach(r => lines.push(`- [MUST] ${r}`));
    guide.stackRules.forEach(r => lines.push(`- [MUST] ${r}`));
    (guide.forbidden || []).forEach(f => lines.push(`- [NEVER] ${f}`));
    if (state.promptMode === 'refactor') {
      lines.push('- [REFACTOR] 先抽离/替换全局 CSS Variables 与基础组件 Props（如 asChild vs render、onClick vs onPress），禁止新旧两套样式变量混用导致主题切换断裂。');
    }
    lines.push('');

    if (state.promptIncludeTokens && guide.dualTokens) {
      lines.push('## 2. 双色模式 CSS Tokens 契约 (Light + Dark)');
      lines.push('```css');
      lines.push(':root {');
      Object.entries(guide.dualTokens.light).forEach(([k, v]) => lines.push(`  ${k}: ${v};`));
      lines.push('}');
      lines.push('[data-theme="dark"] {');
      Object.entries(guide.dualTokens.dark).forEach(([k, v]) => lines.push(`  ${k}: ${v};`));
      lines.push('}');
      lines.push('```');
      lines.push('');
    }

    if (state.promptBanNativeSelect) {
      lines.push('## 3. 交互控件与防内存泄漏契约');
      lines.push('- [CONTROLS] 禁止使用浏览器原生 `<select>` / `alert()` / `confirm()`；所有 Select、Dropdown、Combobox、ContextMenu 必须使用自定义浮层（支持 `↑`/`↓`/`Enter`/`ESC` 键盘导航、外部点击自动收起及 `aria-expanded` / `aria-selected` 同步）。');
      lines.push('- [HIDDEN] 全局样式必须声明 `[hidden] { display: none !important; }`，防止内联 `display:flex` 覆盖 `hidden` 属性。');
      lines.push('- [STATE & LEAKS] 严禁在局部重渲染函数内重复绑定 `document.addEventListener`（应在初始化时绑定单次全局监听器或使用事件委托）；局部组件刷新不得重置无关表单输入或丢失滚动位置。');
      lines.push('- [TABLE & CHECKBOX] 多选表格的全选框仅作用于当前过滤后的可见行，并实时同步 `checked` 与 `indeterminate` 半选态；所有数据列使用 `tabular-nums`。');
      lines.push('- [OFFLINE & STORAGE] 不依赖外部在线字体/CDN（系统字体降级兜底），`localStorage` 读写加 `try/catch`，剪贴板复制提供 `textarea + execCommand` 降级兼容。');
      lines.push('');
    }

    if (state.promptIncludePitfalls) {
      lines.push('## 4. 高频 BUG 防御与代码正反例 (❌ Anti-Pattern vs ✅ Fix)');
      guide.pitfalls.forEach((p, idx) => {
        lines.push(`${idx + 1}. **${p.title}**`);
        lines.push(`   - 说明：${p.detail}`);
        if (p.bad) lines.push(`   - ❌ 错误写法：\`${p.bad}\``);
        if (p.good) lines.push(`   - ✅ 正确写法：\`${p.good}\``);
      });
      lines.push('');
    }

    if (state.promptIncludeVerify) {
      lines.push('## 5. 交付前强制自检门禁 (Verification Gate)');
      lines.push('完成编码后，必须逐项核对以下清单后方可结束任务：');
      lines.push('- [ ] Light / Dark 双模式下主按钮、边框、输入框、弹窗文字对比度全部达标（无黑底黑字/白底白字）。');
      lines.push('- [ ] 代码中零原生 `<select>`、零阻塞式外部网络请求依赖（纯离线也可正常打开运行）。');
      lines.push('- [ ] 键盘 `Tab` / `↑` `↓` / `Enter` / `ESC` 可完整操作下拉、命令面板、弹窗与抽屉，且保留 `:focus-visible` 轮廓环。');
      lines.push('- [ ] 已完整覆盖 Populated（正常）/ Empty（空状态）/ Loading（骨架屏）/ Error（错误重试）边界态。');
      lines.push('- [ ] 已运行语法/类型/构建检查并确认零报错，移动端（<768px）无横向溢出。');
    }

    return lines.join('\n');
  }

  function renderPromptSection(style, state) {
    const guide = D.promptGuides[style.id];
    if (!guide) return '';
    const promptText = buildAgentPrompt(style, state);
    return `
      <section class="showcase-section" id="sec-prompt">
        <div class="section-head">
          <h2 class="section-title">AI Agent 强健提示词与重构避坑指南</h2>
          <p class="section-desc">内含双色 Token 契约、MUST/NEVER 硬约束、❌/✅ 正反代码对照与交付前自检门禁</p>
        </div>
        <div class="grid-2">
          <div class="ui-card">
            <div class="card-label">
              <span>提示词生成器 (Robust Prompt Builder)</span>
              <div class="tabs-bar">
                <button type="button" class="tab-btn ${state.promptMode === 'greenfield' ? 'active' : ''}" data-prompt-mode="greenfield">全新构建</button>
                <button type="button" class="tab-btn ${state.promptMode === 'refactor' ? 'active' : ''}" data-prompt-mode="refactor">重构迁移</button>
                <button type="button" class="tab-btn ${state.promptMode === 'setup' ? 'active' : ''}" data-prompt-mode="setup">底座初始化</button>
              </div>
            </div>
            <div class="row-gap" style="margin-bottom:12px">
              <label class="check-row"><input type="checkbox" class="ui-checkbox" data-prompt-opt="promptIncludeTokens" ${state.promptIncludeTokens ? 'checked' : ''} /> 双色 Token 契约</label>
              <label class="check-row"><input type="checkbox" class="ui-checkbox" data-prompt-opt="promptBanNativeSelect" ${state.promptBanNativeSelect ? 'checked' : ''} /> 控件与防泄漏约束</label>
              <label class="check-row"><input type="checkbox" class="ui-checkbox" data-prompt-opt="promptIncludePitfalls" ${state.promptIncludePitfalls ? 'checked' : ''} /> ❌/✅ 正反例避坑</label>
              <label class="check-row"><input type="checkbox" class="ui-checkbox" data-prompt-opt="promptIncludeVerify" ${state.promptIncludeVerify ? 'checked' : ''} /> 自检门禁</label>
            </div>
            <pre class="prompt-preview" id="agentPromptPreview">${escapeHtml(promptText)}</pre>
            <div class="row-between" style="margin-top:12px;flex-wrap:wrap">
              <span class="muted-text tabular">共 ${promptText.length} 字符 · 结构化防漂移约束</span>
              <div class="row-gap">
                <button type="button" class="ui-btn ui-btn-outline ui-btn-sm" id="copyPromptRuleBtn">复制为 Rules 文件</button>
                <button type="button" class="ui-btn ui-btn-primary ui-btn-sm" id="copyAgentPromptBtn">一键复制给 AI Agent</button>
              </div>
            </div>
          </div>

          <div class="ui-card ui-card-surface">
            <div class="card-label">
              <span>${style.name} 常见 BUG 与 ❌ / ✅ 代码对照</span>
              <span class="ui-badge ui-badge-warn">${guide.pitfalls.length} 项高危避坑</span>
            </div>
            <div class="pitfall-list">
              ${guide.pitfalls.map((p, i) => `
                <div class="pitfall-card">
                  <div class="pitfall-title">${i + 1}. ${escapeHtml(p.title)}</div>
                  <div class="pitfall-detail">${escapeHtml(p.detail)}</div>
                  ${p.bad || p.good ? `
                    <div class="pitfall-diff">
                      ${p.bad ? `<div class="diff-line diff-bad"><span>❌</span><code>${escapeHtml(p.bad)}</code></div>` : ''}
                      ${p.good ? `<div class="diff-line diff-good"><span>✅</span><code>${escapeHtml(p.good)}</code></div>` : ''}
                    </div>
                  ` : ''}
                </div>
              `).join('')}
            </div>
          </div>
        </div>
      </section>
    `;
  }

  function renderTreeNodes(nodes) {
    return `<div style="margin-left:-12px">${nodes.map(n => {
      if (n.type === 'folder') {
        return `
          <div class="tree-node">
            <div class="tree-item" data-tree-toggle>📁 <strong>${n.name}</strong></div>
            <div class="tree-children">${renderTreeNodes(n.children || [])}</div>
          </div>`;
      }
      return `
        <div class="tree-node">
          <div class="tree-item">📄 <span>${n.name}</span> <span class="ui-badge" style="margin-left:auto;font-size:10px">${n.badge}</span></div>
        </div>`;
    }).join('')}</div>`;
  }

  function renderAllControls(state) {
    const wizardIdx = Math.max(1, Math.min(D.wizardSteps.length, state.wizardStep || 1));
    const activeWizard = D.wizardSteps[wizardIdx - 1];
    const tMode = state.tableMode || 'normal';
    const tCols = state.tableCols || { latency: true, coverage: true, updated: true };
    return `
      <!-- 1. Buttons & Actions -->
      <section class="showcase-section" id="sec-buttons">
        <div class="section-head">
          <h2 class="section-title">一、按钮与操作控件 (Buttons & Actions)</h2>
          <p class="section-desc">覆盖所有变体、尺寸、加载态、按钮组与弹层触发器</p>
        </div>
        <div class="grid-2">
          <div class="ui-card">
            <div class="card-label"><span>按钮变体与状态</span></div>
            <div class="row-gap" style="margin-bottom:14px">
              <button class="ui-btn ui-btn-primary">Primary 主要</button>
              <button class="ui-btn ui-btn-secondary">Secondary 次要</button>
              <button class="ui-btn ui-btn-outline">Outline 描边</button>
              <button class="ui-btn ui-btn-ghost">Ghost 幽灵</button>
              <button class="ui-btn ui-btn-danger">Destructive 危险</button>
              <button class="ui-btn ui-btn-primary" disabled>Disabled 禁用</button>
            </div>
            <div class="row-gap">
              <button class="ui-btn ui-btn-primary ui-btn-sm">Small</button>
              <button class="ui-btn ui-btn-primary">Default</button>
              <button class="ui-btn ui-btn-primary ui-btn-lg">Large</button>
              <button class="ui-btn ui-btn-outline" id="asyncLoadingBtn">点击触发异步 Loading</button>
            </div>
          </div>

          <div class="ui-card">
            <div class="card-label"><span>按钮组、Toggle 工具栏、右键菜单与弹层触发</span></div>
            <div class="row-gap" style="margin-bottom:12px">
              <div class="btn-group">
                <button class="ui-btn ui-btn-outline ui-btn-sm">日视图</button>
                <button class="ui-btn ui-btn-outline ui-btn-sm">周视图</button>
                <button class="ui-btn ui-btn-outline ui-btn-sm">月视图</button>
              </div>
              <div class="toggle-group" role="group" aria-label="文本格式">
                <button type="button" class="toggle-btn active" data-toggle-btn>B</button>
                <button type="button" class="toggle-btn" data-toggle-btn style="font-style:italic">I</button>
                <button type="button" class="toggle-btn" data-toggle-btn style="text-decoration:underline">U</button>
                <button type="button" class="toggle-btn mono" data-toggle-btn>&lt;/&gt;</button>
              </div>
              <button class="ui-btn ui-btn-outline ui-btn-icon" title="复制" data-action="toast" data-toast-title="已复制到剪贴板" data-toast-variant="success">📋</button>
              <button class="ui-btn ui-btn-outline ui-btn-icon" title="设置" data-action="open-modal" data-modal="demoDrawer">⚙️</button>
            </div>
            <div class="row-gap" style="margin-bottom:12px">
              <button class="ui-btn ui-btn-secondary ui-btn-sm" data-action="open-modal" data-modal="demoModal">打开 Modal 弹窗</button>
              <button class="ui-btn ui-btn-danger ui-btn-sm" data-action="open-modal" data-modal="alertDialogModal">AlertDialog 危险确认</button>
              <button class="ui-btn ui-btn-secondary ui-btn-sm" data-action="open-modal" data-modal="demoDrawer">Drawer 抽屉</button>
              <button class="ui-btn ui-btn-secondary ui-btn-sm" id="openCmdTrigger2">⌘K 命令面板</button>
              <div class="hover-card-wrap">
                <button type="button" class="ui-btn ui-btn-outline ui-btn-sm">悬停 HoverCard</button>
                <div class="hover-card-panel">
                  <div class="row-gap" style="margin-bottom:8px">
                    <div class="ui-avatar">PR<span class="avatar-status"></span></div>
                    <div>
                      <div style="font-weight:600;font-size:13px">@peer_cal</div>
                      <div class="muted-text" style="font-size:11.5px">Design System Core</div>
                    </div>
                  </div>
                  <p class="muted-text" style="margin-bottom:10px">维护黑白灰极简原语与无障碍交互规范。</p>
                  <div class="row-between muted-text tabular" style="font-size:11.5px">
                    <span><strong>142</strong> 组件</span>
                    <span><strong>99.8%</strong> A11y</span>
                    <button type="button" class="ui-btn ui-btn-primary ui-btn-sm" style="height:24px;padding:0 8px" data-action="toast" data-toast-title="已关注 @peer_cal" data-toast-variant="success">关注</button>
                  </div>
                </div>
              </div>
              <div class="ui-dropdown" data-dropdown>
                <button class="ui-btn ui-btn-outline ui-btn-sm" type="button" data-dropdown-trigger aria-expanded="false">更多操作 ▾</button>
                <div class="ui-dropdown-menu" role="menu" hidden>
                  <button type="button" class="ui-dropdown-item" role="menuitem" data-action="toast" data-toast-title="已复制组件代码" data-toast-variant="success"><span>复制源码</span><kbd>⌘C</kbd></button>
                  <button type="button" class="ui-dropdown-item" role="menuitem" data-action="toast" data-toast-title="已创建副本" data-toast-variant="info"><span>创建副本</span><kbd>⌘D</kbd></button>
                  <div class="ui-dropdown-divider"></div>
                  <button type="button" class="ui-dropdown-item danger" role="menuitem" data-action="open-modal" data-modal="alertDialogModal"><span>删除组件</span><kbd>⌫</kbd></button>
                </div>
              </div>
            </div>
            <div class="context-zone" id="demoContextZone">
              🖱️ <strong>自定义右键菜单 (Context Menu)</strong>：在此虚线区域内点击鼠标右键，体验随风格变化的右键浮层。
            </div>
          </div>
        </div>
      </section>

      <!-- 2. Form Controls -->
      <section class="showcase-section" id="sec-forms">
        <div class="section-head">
          <h2 class="section-title">二、完整表单与输入控件 (Form & Selection Controls)</h2>
          <p class="section-desc">输入框、密码显隐、多选 Combobox、开关、滑块、OTP、日历与文件拖拽</p>
        </div>
        <div class="grid-3">
          <!-- Column 1: Text, Password, Textarea -->
          <div class="ui-card">
            <div class="card-label"><span>文本与验证输入</span></div>
            <div class="form-field">
              <label class="ui-label">工作区域名 (前缀组合)</label>
              <div class="input-group">
                <span class="input-addon">https://</span>
                <input type="text" value="${escapeHtml(state.clearableVal)}" id="clearableInput" />
                <button class="input-action-btn" type="button" id="clearInputBtn" title="清空">✕</button>
              </div>
            </div>
            <div class="form-field">
              <label class="ui-label">密码输入 (显隐切换)</label>
              <div class="input-group">
                <input type="password" value="super-secret-token" id="pwdInput" />
                <button class="input-action-btn" type="button" id="togglePwdBtn">显示</button>
              </div>
            </div>
            <div class="form-field">
              <label class="ui-label">校验错误态</label>
              <input class="ui-input has-error" type="email" value="invalid-email@" />
              <span class="ui-error-msg">请输入有效的邮箱地址格式</span>
            </div>
            <div class="form-field">
              <label class="ui-label"><span>多行文本域</span><span class="muted-text tabular" id="textareaCount">${state.textareaVal.length} / 120</span></label>
              <textarea class="ui-textarea" id="demoTextarea" rows="2" maxlength="120" placeholder="输入组件需求描述...">${escapeHtml(state.textareaVal)}</textarea>
            </div>
          </div>

          <!-- Column 2: Custom Select, Combobox, Checkboxes, Radios, Switches -->
          <div class="ui-card">
            <div class="card-label"><span>自定义 Select、多选与开关</span></div>
            <div class="form-field">
              <label class="ui-label" id="primitiveSelectLabel">底层原语库 (自定义 Select)</label>
              <div class="ui-select" data-select id="formPrimitiveSelect" data-value="${escapeHtml(state.selectPrimitive)}">
                <button type="button" class="ui-select-trigger" aria-haspopup="listbox" aria-expanded="false" aria-labelledby="primitiveSelectLabel">
                  <span class="ui-select-value">${escapeHtml(state.selectPrimitiveLabel)}</span>
                  <svg class="ui-select-chevron" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m6 9 6 6 6-6"/></svg>
                </button>
                <div class="ui-select-menu" role="listbox" hidden>
                  ${[
                    ['base-ui', 'Base UI (推荐底层)'],
                    ['radix', 'Radix Primitives'],
                    ['react-aria', 'React Aria'],
                    ['ark-ui', 'Ark UI']
                  ].map(([val, lbl]) => `
                    <button type="button" class="ui-select-option ${state.selectPrimitive === val ? 'selected' : ''}" role="option" aria-selected="${state.selectPrimitive === val}" data-value="${val}">
                      <span>${lbl}</span><span class="ui-select-check">✓</span>
                    </button>
                  `).join('')}
                </div>
              </div>
            </div>
            <div class="form-field">
              <label class="ui-label">多选 Combobox (点击标签可移除)</label>
              <div class="combobox-wrap" id="comboboxWrap">
                <div class="combobox-tags" id="comboboxTags"></div>
                <input class="ui-input" type="text" id="comboboxInput" placeholder="搜索并添加技术栈..." autocomplete="off" />
                <div class="combobox-dropdown" id="comboboxDropdown" hidden></div>
              </div>
            </div>
            <div class="form-field">
              <label class="ui-label">复选框 (含 Indeterminate 半选态)</label>
              <div style="display:flex;flex-direction:column;gap:6px">
                <label class="check-row"><input type="checkbox" class="ui-checkbox" id="cbParent" /> 全选所有组件模块</label>
                <div style="padding-left:20px;display:flex;gap:14px">
                  <label class="check-row"><input type="checkbox" class="ui-checkbox cb-child" checked /> 表单</label>
                  <label class="check-row"><input type="checkbox" class="ui-checkbox cb-child" /> 图表</label>
                </div>
              </div>
            </div>
            <div class="form-field">
              <label class="ui-label">开关 Switch 控件</label>
              <div class="row-between">
                <span class="muted-text">启用 CSS Variables 主题</span>
                <button type="button" class="ui-switch" role="switch" aria-checked="true" data-switch></button>
              </div>
            </div>
          </div>

          <!-- Column 3: Slider, Dual Range, Stepper, Rating, OTP, Dropzone -->
          <div class="ui-card">
            <div class="card-label"><span>数值、区间、评分、OTP 与上传</span></div>
            <div class="form-field">
              <label class="ui-label"><span>滑块 Slider</span><span class="tabular mono" id="sliderReadout">${state.sliderVal}%</span></label>
              <input class="ui-range" type="range" min="0" max="100" value="${state.sliderVal}" id="demoSlider" />
            </div>
            <div class="form-field">
              <label class="ui-label"><span>双向区间滑块 (Range)</span><span class="tabular mono" id="dualRangeReadout">¥${state.rangeMin} – ¥${state.rangeMax}</span></label>
              <div class="dual-range-wrap">
                <div class="dual-range-track"></div>
                <div class="dual-range-fill" id="dualRangeFill" style="left:${(state.rangeMin / 1000) * 100}%;right:${100 - (state.rangeMax / 1000) * 100}%"></div>
                <input class="dual-range-input" type="range" min="0" max="1000" step="10" value="${state.rangeMin}" id="dualRangeMin" />
                <input class="dual-range-input" type="range" min="0" max="1000" step="10" value="${state.rangeMax}" id="dualRangeMax" />
              </div>
            </div>
            <div class="row-between" style="margin-bottom:14px">
              <div>
                <div class="ui-label">步进器 Stepper</div>
                <div class="muted-text">并发线程: <strong class="tabular" id="stepVal">${state.stepperVal}</strong></div>
              </div>
              <div class="stepper">
                <button type="button" id="stepMinus">-</button>
                <span class="tabular" id="stepValBox">${state.stepperVal}</span>
                <button type="button" id="stepPlus">+</button>
              </div>
            </div>
            <div class="row-between" style="margin-bottom:14px">
              <span class="ui-label">星级评分 (Rating)</span>
              <div class="row-gap">
                <div class="rating-group" id="ratingGroup">
                  ${[1, 2, 3, 4, 5].map(n => `<button type="button" class="rating-star ${n <= state.rating ? 'filled' : ''}" data-rate="${n}" aria-label="${n}星">★</button>`).join('')}
                </div>
                <span class="tabular mono muted-text" id="ratingReadout">${state.rating}.0</span>
              </div>
            </div>
            <div class="form-field">
              <label class="ui-label">6 位验证码 (OTP 自动跳格)</label>
              <div class="otp-group" id="otpGroup">
                ${Array.from({ length: 6 }, (_, i) => `<input class="otp-box" maxlength="1" inputmode="numeric" data-otp-idx="${i}" value="${i < 3 ? i + 1 : ''}" />`).join('')}
              </div>
            </div>
            <div class="form-field">
              <label class="ui-label">文件拖拽上传 (Dropzone)</label>
              <div class="dropzone" id="demoDropzone">
                <div style="font-size:13px;font-weight:500">点击或拖拽文件到此处</div>
                <div class="muted-text" id="dropzoneText">支持 JSON / CSS / TSX (最大 10MB)</div>
                <input type="file" id="dropzoneFileInput" hidden />
              </div>
            </div>
          </div>
        </div>

        <!-- Calendar (Single / Range) + TimePicker + Radio Cards Row -->
        <div class="grid-2" style="margin-top:16px">
          <div class="ui-card">
            <div class="card-label"><span>单选卡片组、分段控件与时间选择器</span></div>
            <div class="radio-card-grid" style="margin-bottom:14px">
              ${[
                ['mit', 'MIT 开源', '零限制商用'],
                ['apache', 'Apache 2.0', '专利授权保护'],
                ['pro', 'Enterprise', '含 SLA 支持']
              ].map(([id, t, s]) => `
                <button type="button" class="radio-card ${state.radioPlan === id ? 'selected' : ''}" data-radio-plan="${id}">
                  <span class="radio-card-title">${t}</span>
                  <span class="radio-card-sub">${s}</span>
                </button>
              `).join('')}
            </div>
            <div class="row-between" style="margin-bottom:14px">
              <span class="muted-text">已选方案：<strong id="radioPlanLabel">${state.radioPlan.toUpperCase()}</strong></span>
              <div class="tabs-bar">
                <button class="tab-btn active" data-seg>月付</button>
                <button class="tab-btn" data-seg>年付 (8折)</button>
              </div>
            </div>
            <div class="form-field">
              <label class="ui-label" id="timePickerLabel">时间选择器 (Custom TimePicker)</label>
              <div class="ui-select" data-select id="formTimeSelect" data-value="${escapeHtml(state.timeSlot)}">
                <button type="button" class="ui-select-trigger" aria-haspopup="listbox" aria-expanded="false" aria-labelledby="timePickerLabel">
                  <span class="ui-select-value">${escapeHtml(state.timeSlot)} (Asia/Shanghai)</span>
                  <svg class="ui-select-chevron" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m6 9 6 6 6-6"/></svg>
                </button>
                <div class="ui-select-menu" role="listbox" hidden>
                  ${D.timeSlots.map(t => `
                    <button type="button" class="ui-select-option ${state.timeSlot === t ? 'selected' : ''}" role="option" aria-selected="${state.timeSlot === t}" data-value="${t}">
                      <span class="tabular">${t} (Asia/Shanghai)</span><span class="ui-select-check">✓</span>
                    </button>
                  `).join('')}
                </div>
              </div>
            </div>
          </div>

          <div class="ui-card">
            <div class="card-label">
              <span>日历 / 区间选择器 (Date &amp; Range Picker)</span>
              <div class="tabs-bar">
                <button type="button" class="tab-btn ${state.calMode === 'single' ? 'active' : ''}" data-cal-mode="single">单选</button>
                <button type="button" class="tab-btn ${state.calMode === 'range' ? 'active' : ''}" data-cal-mode="range">区间</button>
              </div>
            </div>
            <div class="calendar-box">
              <div class="cal-header">
                <span>2026 年 11 月</span>
                <span class="tabular muted-text" id="selectedDateLabel">${state.calMode === 'range' ? `11-${String(state.rangeStart).padStart(2, '0')} → 11-${String(state.rangeEnd).padStart(2, '0')}` : `2026-11-${String(state.selectedDay).padStart(2, '0')}`}</span>
              </div>
              <div class="cal-grid" id="calGridContainer">
                ${['日', '一', '二', '三', '四', '五', '六'].map(d => `<div class="cal-dow">${d}</div>`).join('')}
                ${Array.from({ length: 30 }, (_, i) => {
                  const day = i + 1;
                  const isEdge = state.calMode === 'range' ? (day === state.rangeStart || day === state.rangeEnd) : (day === state.selectedDay);
                  const inRange = state.calMode === 'range' && day > state.rangeStart && day < state.rangeEnd;
                  return `<button type="button" class="cal-day ${day === 14 ? 'today' : ''} ${isEdge ? 'selected range-edge' : ''} ${inRange ? 'in-range' : ''}" data-cal-day="${day}">${day}</button>`;
                }).join('')}
              </div>
              <div class="row-gap" style="margin-top:10px;justify-content:flex-end">
                <button type="button" class="ui-btn ui-btn-ghost ui-btn-sm" data-cal-preset="week">近 7 天</button>
                <button type="button" class="ui-btn ui-btn-ghost ui-btn-sm" data-cal-preset="month">全月</button>
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- 3. Data Display & Feedback -->
      <section class="showcase-section" id="sec-data">
        <div class="section-head">
          <h2 class="section-title">三、数据展示与反馈控件 (Data Display & Feedback)</h2>
          <p class="section-desc">徽章、头像组、提示条、进度条/骨架屏、Tabs、手风琴、Tooltip/Popover 与 Toast</p>
        </div>
        <div class="grid-3">
          <div class="ui-card">
            <div class="card-label"><span>徽章、头像与提示反馈</span></div>
            <div class="row-gap" style="margin-bottom:14px">
              <span class="ui-badge">Default</span>
              <span class="ui-badge ui-badge-solid">Solid</span>
              <span class="ui-badge ui-badge-success"><span class="badge-dot"></span>Running</span>
              <span class="ui-badge ui-badge-warn">Pending</span>
              <span class="ui-badge ui-badge-danger">Failed</span>
            </div>
            <div class="row-between" style="margin-bottom:14px">
              <div class="avatar-group">
                <div class="ui-avatar">PR<span class="avatar-status"></span></div>
                <div class="ui-avatar">LR</div>
                <div class="ui-avatar">DG</div>
                <div class="ui-avatar">+5</div>
              </div>
              <div class="row-gap">
                <div class="tooltip-wrap">
                  <button class="ui-btn ui-btn-outline ui-btn-sm">悬停 Tooltip</button>
                  <div class="tooltip-bubble">纯 CSS 定位气泡提示</div>
                </div>
                <div class="tooltip-wrap">
                  <button class="ui-btn ui-btn-outline ui-btn-sm" id="popoverTrigger">Popover</button>
                  <div class="popover-panel" id="demoPopover" hidden>
                    <strong>组件信息面板</strong>
                    <p class="muted-text" style="margin-top:4px">支持点击外部自动关闭与动态样式继承。</p>
                  </div>
                </div>
              </div>
            </div>
            <div class="row-gap">
              <button class="ui-btn ui-btn-primary ui-btn-sm" data-action="toast" data-toast-title="部署成功" data-toast-desc="Production 构建已上线 (1.4s)" data-toast-variant="success">成功 Toast</button>
              <button class="ui-btn ui-btn-outline ui-btn-sm" data-action="toast" data-toast-title="配额预警" data-toast-desc="API 调用量已达 85%" data-toast-variant="warn">警告</button>
              <button class="ui-btn ui-btn-outline ui-btn-sm" data-action="toast" data-toast-title="构建失败" data-toast-desc="Token 校验未通过" data-toast-variant="danger">错误</button>
              <button class="ui-btn ui-btn-secondary ui-btn-sm" id="asyncPromiseToastBtn">异步 Promise</button>
            </div>
          </div>

          <div class="ui-card">
            <div class="card-label"><span>Tabs 选项卡 & Accordion 手风琴</span></div>
            <div class="tabs-bar" style="margin-bottom:10px">
              <button class="tab-btn active" data-tab-group="demo" data-tab-target="t1">概览</button>
              <button class="tab-btn" data-tab-group="demo" data-tab-target="t2">性能</button>
              <button class="tab-btn" data-tab-group="demo" data-tab-target="t3">无障碍</button>
            </div>
            <div class="tab-panel" data-tab-panel="demo" id="t1">
              <div class="ui-alert ui-alert-info">
                <div class="ui-alert-title">Token 驱动架构</div>
                <div class="ui-alert-desc">切换左侧风格时，所有组件自动继承新设计规范。</div>
              </div>
            </div>
            <div class="tab-panel" data-tab-panel="demo" id="t2" hidden>
              <div class="ui-alert ui-alert-success">
                <div class="ui-alert-title">零运行时开销</div>
                <div class="ui-alert-desc">原生 CSS Variables 切换耗时 &lt; 1ms，无虚拟 DOM 重绘瓶颈。</div>
              </div>
            </div>
            <div class="tab-panel" data-tab-panel="demo" id="t3" hidden>
              <div class="ui-alert ui-alert-warn">
                <div class="ui-alert-title">WAI-ARIA 规范</div>
                <div class="ui-alert-desc">支持 role="switch"、aria-modal 与键盘 ESC 退出。</div>
              </div>
            </div>
            <div style="margin-top:12px">
              <div class="accordion-item">
                <button class="accordion-trigger" data-acc-btn>如何迁移到我的项目？ <span>+</span></button>
                <div class="accordion-body" hidden>复制对应风格目录下的 CSS Variables 到 globals.css 即可无缝换肤。</div>
              </div>
              <div class="accordion-item">
                <button class="accordion-trigger" data-acc-btn>是否支持暗黑模式？ <span>+</span></button>
                <div class="accordion-body" hidden>右上角可一键切换浅色/深色，10 套风格均内置双色 Token。</div>
              </div>
            </div>
          </div>

          <div class="ui-card">
            <div class="card-label">
              <span>进度条、骨架屏、面包屑与分页</span>
              <button class="ui-btn ui-btn-ghost ui-btn-sm" id="toggleSkeletonBtn">切换骨架屏</button>
            </div>
            <nav class="breadcrumbs" aria-label="面包屑">
              <a href="#sec-signature">工作区</a><span>/</span>
              <a href="#sec-forms">组件库</a><span>/</span>
              <strong>当前风格</strong>
            </nav>
            <div id="progressDemoContent">
              <div class="row-between" style="margin-bottom:6px;font-size:12.5px">
                <span>构建打包进度</span>
                <span class="tabular mono" id="progressPct">${state.progress}%</span>
              </div>
              <div class="progress-track" style="margin-bottom:12px">
                <div class="progress-fill" id="progressFill" style="width:${state.progress}%"></div>
              </div>
              <div class="row-gap" style="margin-bottom:16px">
                <button class="ui-btn ui-btn-outline ui-btn-sm" id="incProgressBtn">+10% 进度</button>
                <button class="ui-btn ui-btn-outline ui-btn-sm" id="resetProgressBtn">重置</button>
              </div>
            </div>
            <div id="skeletonDemoContent" hidden style="display:flex;flex-direction:column;gap:8px;margin-bottom:16px">
              <div class="skeleton" style="height:14px;width:60%"></div>
              <div class="skeleton" style="height:10px;width:100%"></div>
              <div class="skeleton" style="height:28px;width:40%"></div>
            </div>
            <div class="row-between">
              <span class="muted-text">第 <strong id="pageNumLabel">1</strong> / 5 页</span>
              <div class="pagination" id="demoPagination">
                <button class="page-btn active" data-page="1">1</button>
                <button class="page-btn" data-page="2">2</button>
                <button class="page-btn" data-page="3">3</button>
                <button class="page-btn" data-page="4">4</button>
                <button class="page-btn" data-page="5">5</button>
              </div>
            </div>
          </div>
        </div>

        <div class="grid-2" style="margin-top:16px">
          <!-- Steps Wizard -->
          <div class="ui-card" id="stepsWizardCard">
            <div class="row-between" style="margin-bottom:12px">
              <div class="control-label" style="margin:0">Steps / Wizard 分步向导</div>
              <span class="ui-badge ui-badge-outline tabular-nums" id="wizardStepCounter">步骤 ${wizardIdx} / ${D.wizardSteps.length}</span>
            </div>
            <div class="steps-bar">
              ${D.wizardSteps.map(ws => {
                const cls = ws.step < wizardIdx ? 'done' : (ws.step === wizardIdx ? 'active' : '');
                return `
                  <button type="button" class="step-item ${cls}" data-wizard-step="${ws.step}">
                    <div style="display:flex;align-items:center;margin-bottom:4px">
                      <span class="step-num">${ws.step < wizardIdx ? '✓' : ws.step}</span>
                      <strong style="font-size:12px">${escapeHtml(ws.title)}</strong>
                    </div>
                  </button>
                `;
              }).join('')}
            </div>
            <div class="surface-box" id="wizardStepBody" style="margin-bottom:12px;font-size:12.5px">
              <strong>Step ${activeWizard.step}: ${escapeHtml(activeWizard.title)}</strong>
              <div class="muted-text" style="margin-top:4px">${escapeHtml(activeWizard.desc)}</div>
            </div>
            <div class="row-between">
              <button type="button" class="ui-btn ui-btn-outline ui-btn-sm" id="wizardPrevBtn" ${wizardIdx <= 1 ? 'disabled' : ''}>← 上一步</button>
              <button type="button" class="ui-btn ui-btn-primary ui-btn-sm" id="wizardNextBtn">${wizardIdx >= D.wizardSteps.length ? '完成校验 ✓' : '下一步 →'}</button>
            </div>
          </div>

          <!-- Timeline -->
          <div class="ui-card">
            <div class="row-between" style="margin-bottom:12px">
              <div class="control-label" style="margin:0">Timeline 动态时间轴</div>
              <span class="ui-badge ui-badge-outline">Audit Log</span>
            </div>
            <div class="timeline-list">
              ${D.timelineItems.map(item => `
                <div class="timeline-item">
                  <span class="timeline-dot ${escapeHtml(item.status)}"></span>
                  <div class="row-between">
                    <strong style="font-size:12.5px">${escapeHtml(item.title)}</strong>
                    <span class="muted-text mono-text" style="font-size:11px">${escapeHtml(item.time)}</span>
                  </div>
                  <div class="muted-text" style="font-size:12px;margin-top:2px">${escapeHtml(item.desc)}</div>
                </div>
              `).join('')}
            </div>
          </div>
        </div>
      </section>

      <!-- 4. Interactive Data Table -->
      <section class="showcase-section" id="sec-table">
        <div class="section-head">
          <h2 class="section-title">四、交互式数据表格 (Interactive Data Table)</h2>
          <p class="section-desc">支持搜索过滤、列排序、列显隐开关、行操作菜单与正常 / 空状态 / 加载态切换</p>
        </div>
        <div class="ui-card">
          <div class="row-between" style="margin-bottom:14px;flex-wrap:wrap;gap:10px">
            <div class="row-gap" style="flex-wrap:wrap">
              <input class="ui-input" style="width:220px" type="text" id="tableSearchInput" placeholder="搜索项目或负责人..." value="${escapeHtml(state.tableQuery)}" />
              <span class="muted-text" id="tableSelectedCount">已选 0 行</span>
              <div class="ui-tabs" role="tablist" aria-label="表格状态视图">
                <button type="button" class="ui-tab ${tMode === 'normal' ? 'active' : ''}" data-table-mode="normal">正常</button>
                <button type="button" class="ui-tab ${tMode === 'empty' ? 'active' : ''}" data-table-mode="empty">空状态</button>
                <button type="button" class="ui-tab ${tMode === 'loading' ? 'active' : ''}" data-table-mode="loading">加载态</button>
              </div>
            </div>
            <div class="row-gap" style="flex-wrap:wrap">
              <div class="ui-dropdown" data-dropdown>
                <button type="button" class="ui-btn ui-btn-outline ui-btn-sm" data-dropdown-trigger aria-expanded="false">列显隐 ▾</button>
                <div class="ui-dropdown-menu" role="menu" hidden style="min-width:150px;padding:6px 8px">
                  <label class="checkbox-row" style="padding:4px 4px;font-size:12px;cursor:pointer">
                    <input type="checkbox" class="ui-checkbox" data-table-col="latency" ${tCols.latency !== false ? 'checked' : ''} /> 延迟 (Latency)
                  </label>
                  <label class="checkbox-row" style="padding:4px 4px;font-size:12px;cursor:pointer">
                    <input type="checkbox" class="ui-checkbox" data-table-col="coverage" ${tCols.coverage !== false ? 'checked' : ''} /> 测试覆盖率
                  </label>
                  <label class="checkbox-row" style="padding:4px 4px;font-size:12px;cursor:pointer">
                    <input type="checkbox" class="ui-checkbox" data-table-col="updated" ${tCols.updated !== false ? 'checked' : ''} /> 最近更新
                  </label>
                </div>
              </div>
              <button class="ui-btn ui-btn-outline ui-btn-sm" id="sortCoverageBtn">按覆盖率排序 ⇅</button>
              <button class="ui-btn ui-btn-primary ui-btn-sm" data-action="open-modal" data-modal="demoModal">+ 新建记录</button>
            </div>
          </div>
          <div class="table-wrap">
            <table class="ui-table">
              <thead>
                <tr>
                  <th style="width:36px"><input type="checkbox" class="ui-checkbox" id="tableSelectAll" /></th>
                  <th>ID</th>
                  <th>项目名称</th>
                  <th>负责人</th>
                  <th>状态</th>
                  <th data-col="latency" ${tCols.latency === false ? 'hidden' : ''}>延迟</th>
                  <th data-col="coverage" ${tCols.coverage === false ? 'hidden' : ''}>测试覆盖率</th>
                  <th data-col="updated" ${tCols.updated === false ? 'hidden' : ''}>最近更新</th>
                  <th style="width:54px;text-align:right">操作</th>
                </tr>
              </thead>
              <tbody id="dataTableBody">
                <!-- Rendered by app.js -->
              </tbody>
            </table>
          </div>
        </div>
      </section>
    `;
  }

  function renderMatrix() {
    return `
      <section class="style-hero">
        <h1 class="hero-title">10 大 UI 设计系统全景对比矩阵</h1>
        <p class="hero-sub">涵盖 Copy-Paste 源码级、跨框架移植、Token 驱动 Design System 与无样式原语底座，点击任意行即可立即切换预览。</p>
      </section>
      <div class="ui-card">
        <div class="table-wrap">
          <table class="ui-table">
            <thead>
              <tr>
                <th>#</th>
                <th>设计系统</th>
                <th>所属分类</th>
                <th>核心技术栈</th>
                <th>移植性</th>
                <th>核心特点</th>
                <th>操作</th>
              </tr>
            </thead>
            <tbody>
              ${D.styles.map(s => {
                const cat = D.categories.find(c => c.id === s.category);
                return `
                  <tr>
                    <td class="mono">${s.num}</td>
                    <td><strong>${s.name}</strong><div class="muted-text">${s.subtitle}</div></td>
                    <td><span class="ui-badge">${cat ? cat.label : ''}</span></td>
                    <td style="font-size:12px">${s.stack}</td>
                    <td>${stars(s.rating)}</td>
                    <td style="font-size:12px;max-width:260px">${s.traits.join(' · ')}</td>
                    <td><button class="ui-btn ui-btn-primary ui-btn-sm" data-select-style="${s.id}">切换预览</button></td>
                  </tr>
                `;
              }).join('')}
            </tbody>
          </table>
        </div>
      </div>
    `;
  }

  function escapeHtml(str) {
    return String(str)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  return { renderHero, renderSignature, renderTokensSpec, renderPromptSection, buildAgentPrompt, renderAllControls, renderMatrix, escapeHtml };
})();
