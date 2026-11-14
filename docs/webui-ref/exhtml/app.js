(function () {
  const D = window.SHOWCASE_DATA;
  const C = window.ShowcaseComponents;

  function loadSaved(key, fallback) {
    try { return localStorage.getItem(key) || fallback; } catch (e) { return fallback; }
  }
  function savePref(key, val) {
    try { localStorage.setItem(key, val); } catch (e) { /* ignore */ }
  }

  const state = {
    view: 'style', // 'style' | 'matrix'
    styleId: loadSaved('exhtml_style', 'coss'),
    theme: loadSaved('exhtml_theme', 'light'),
    showTokens: false,
    // Prompt builder state
    promptMode: 'greenfield', // 'greenfield' | 'refactor' | 'setup'
    promptIncludeTokens: true,
    promptIncludePitfalls: true,
    promptBanNativeSelect: true,
    promptIncludeVerify: true,
    // Style-specific interactive state
    cossSlot: '09:30',
    cossDuration: '30m',
    untitledEdge: 'populated',
    kanban: JSON.parse(JSON.stringify(D.kanbanInitial)),
    aiMessages: [
      { role: 'bot', text: '你好！我是 Kibo AI 助手，输入需求即可预览生成结果。' }
    ],
    parkRadius: '6px',
    parkAccent: '#18181b',
    parkFw: 'react',
    radixAccent: '#3e63dd',
    radixRadius: '6px',
    radixScale: 1,
    showSwissGrid: false,
    baseUiUnstyled: false,
    baseUiLogs: ['[Init] Base UI Primitives ready.', '[State] open=false, disabled=false'],
    basecoatShowHtml: false,
    ariaAnnouncement: '等待交互：点击左侧控件或按 Tab 键切换焦点。',
    // Shared kitchen-sink state
    clearableVal: 'acme-ui.dev',
    textareaVal: '',
    selectPrimitive: 'base-ui',
    selectPrimitiveLabel: 'Base UI (推荐底层)',
    sliderVal: 64,
    stepperVal: 4,
    radioPlan: 'mit',
    selectedDay: 14,
    rangeMin: 120,
    rangeMax: 680,
    rating: 4,
    timeSlot: '09:30',
    calMode: 'single',
    rangeStart: 10,
    rangeEnd: 18,
    rangePendingStart: null,
    wizardStep: 1,
    comboboxSelected: ['coss ui (Base UI + Tailwind)', 'Tremor (Vercel Data Viz)'],
    tableQuery: '',
    tableSortDesc: false,
    tableMode: 'normal',
    tableCols: { latency: true, coverage: true, updated: true },
    tableRows: D.tableRows.map(r => ({ ...r })),
    pendingDeleteId: null,
    tableSelectedIds: new Set(),
    progress: 72,
    cmdActiveIdx: 0
  };

  if (!D.styles.some(s => s.id === state.styleId)) state.styleId = 'coss';

  // DOM refs
  const htmlEl = document.documentElement;
  const sidebar = document.getElementById('sidebar');
  const sidebarBackdrop = document.getElementById('sidebarBackdrop');
  const sidebarNav = document.getElementById('sidebarNav');
  const mainCanvas = document.getElementById('mainCanvas');
  const topBreadcrumb = document.getElementById('topBreadcrumb');
  const tokenBar = document.getElementById('tokenBar');
  const tokenBarInner = document.getElementById('tokenBarInner');
  const themeLabel = document.getElementById('themeLabel');
  const openMatrixBtn = document.getElementById('openMatrixBtn');
  const quickJumpPills = document.getElementById('quickJumpPills');
  const customContextMenu = document.getElementById('customContextMenu');

  function getCurrentStyle() {
    return D.styles.find(s => s.id === state.styleId) || D.styles[0];
  }

  function copyText(text, okTitle, okDesc) {
    const done = () => showToast(okTitle || '已复制到剪贴板', okDesc || '', 'success');
    const fallback = () => {
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.left = '-9999px';
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand('copy'); done(); }
      catch (e) { showToast('复制失败', '请手动全选复制文本框内容。', 'danger'); }
      ta.remove();
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(done).catch(fallback);
    } else {
      fallback();
    }
  }

  function applyThemeAndTokens() {
    htmlEl.setAttribute('data-style', state.styleId);
    htmlEl.setAttribute('data-theme', state.theme);
    savePref('exhtml_style', state.styleId);
    savePref('exhtml_theme', state.theme);
    themeLabel.textContent = state.theme === 'light' ? '浅色' : '深色';

    // Park UI dynamic CSS variables
    if (state.styleId === 'park') {
      htmlEl.style.setProperty('--park-radius', state.parkRadius);
      const isDarkDefault = state.theme === 'dark' && state.parkAccent === '#18181b';
      htmlEl.style.setProperty('--park-accent', isDarkDefault ? '#fafafa' : state.parkAccent);
      htmlEl.style.setProperty('--park-accent-fg', isDarkDefault ? '#09090b' : '#ffffff');
    } else {
      htmlEl.style.removeProperty('--park-radius');
      htmlEl.style.removeProperty('--park-accent');
      htmlEl.style.removeProperty('--park-accent-fg');
    }

    // Radix Themes dynamic CSS variables (fix Mono dark contrast)
    if (state.styleId === 'radix') {
      const isRadixMonoDark = state.theme === 'dark' && state.radixAccent === '#1c2024';
      htmlEl.style.setProperty('--radix-accent', isRadixMonoDark ? '#edeef0' : state.radixAccent);
      htmlEl.style.setProperty('--radix-accent-fg', isRadixMonoDark ? '#111113' : '#ffffff');
      htmlEl.style.setProperty('--radix-radius', state.radixRadius);
      htmlEl.style.setProperty('--ui-scale', String(state.radixScale));
    } else {
      htmlEl.style.removeProperty('--radix-accent');
      htmlEl.style.removeProperty('--radix-accent-fg');
      htmlEl.style.removeProperty('--radix-radius');
      htmlEl.style.setProperty('--ui-scale', '1');
    }

    const heroBadge = document.getElementById('heroThemeBadge');
    if (heroBadge) heroBadge.textContent = state.theme === 'light' ? '浅色模式 (Light)' : '深色模式 (Dark)';
    if (state.showTokens) renderTokenBar();
  }

  function renderSidebar() {
    sidebarNav.innerHTML = D.categories.map(cat => {
      const items = D.styles.filter(s => s.category === cat.id);
      return `
        <div class="nav-group">
          <div class="nav-group-label">${cat.label}</div>
          ${items.map(s => `
            <button type="button" class="nav-item ${state.view === 'style' && state.styleId === s.id ? 'active' : ''}" data-select-style="${s.id}">
              <span class="nav-item-left">
                <span class="nav-num">${s.num}</span>
                <span class="nav-name">${s.name}</span>
              </span>
              <span class="nav-dot"></span>
            </button>
          `).join('')}
        </div>
      `;
    }).join('');
    openMatrixBtn.classList.toggle('active', state.view === 'matrix');
  }

  function renderTopbar() {
    if (state.view === 'matrix') {
      topBreadcrumb.innerHTML = `<span>全部风格</span> <span>/</span> <strong>对比矩阵</strong>`;
      quickJumpPills.style.display = 'none';
    } else {
      const s = getCurrentStyle();
      const cat = D.categories.find(c => c.id === s.category);
      topBreadcrumb.innerHTML = `<span>${cat ? cat.label.split('、')[1] : ''}</span> <span>/</span> <strong>${s.num}. ${s.name}</strong>`;
      quickJumpPills.style.display = '';
    }
  }

  function renderTokenBar() {
    const s = getCurrentStyle();
    tokenBar.hidden = !state.showTokens;
    if (!state.showTokens) return;
    const cs = getComputedStyle(htmlEl);
    const liveTokens = {
      '--bg': cs.getPropertyValue('--bg').trim() || s.tokens['--bg'],
      '--surface': cs.getPropertyValue('--surface').trim() || s.tokens['--surface'],
      '--border': cs.getPropertyValue('--border').trim() || s.tokens['--border'],
      '--accent': cs.getPropertyValue('--accent').trim() || s.tokens['--accent'],
      '--radius-md': cs.getPropertyValue('--radius-md').trim() || s.tokens['--radius']
    };
    tokenBarInner.innerHTML = `
      <span style="font-size:12px;font-weight:600;margin-right:4px">${s.name} Live Tokens (${state.theme}):</span>
      ${Object.entries(liveTokens).map(([k, v]) => `
        <span class="token-chip">
          ${v.startsWith('#') || v.startsWith('rgb') ? `<span class="token-swatch" style="background:${v}"></span>` : ''}
          <span>${k}:</span> <strong>${v}</strong>
        </span>
      `).join('')}
      <button type="button" class="ui-btn ui-btn-outline ui-btn-sm" id="copyLiveTokensBtn" style="margin-left:auto">复制 CSS Variables</button>
    `;
    const copyTokBtn = document.getElementById('copyLiveTokensBtn');
    if (copyTokBtn) {
      copyTokBtn.onclick = () => {
        const cssText = `:root {\n${Object.entries(liveTokens).map(([k, v]) => `  ${k}: ${v};`).join('\n')}\n}`;
        copyText(cssText, '已复制 CSS Variables', `${s.name} (${state.theme}) 变量已复制到剪贴板。`);
      };
    }
  }

  function renderMain() {
    applyThemeAndTokens();
    renderSidebar();
    renderTopbar();
    renderTokenBar();

    if (state.view === 'matrix') {
      mainCanvas.innerHTML = C.renderMatrix();
      bindCanvasEvents();
      return;
    }

    const s = getCurrentStyle();
    mainCanvas.innerHTML =
      C.renderHero(s) +
      `<div id="signatureWrap">${C.renderSignature(s, state)}</div>` +
      C.renderTokensSpec(s) +
      `<div id="promptWrap">${C.renderPromptSection(s, state)}</div>` +
      C.renderAllControls(state);

    bindCanvasEvents();
    renderTableRows();
    renderCombobox();
    syncIndeterminate();
  }

  function renderSignatureOnly() {
    const wrap = document.getElementById('signatureWrap');
    if (!wrap) return renderMain();
    wrap.innerHTML = C.renderSignature(getCurrentStyle(), state);
    bindSignatureEvents(wrap);
    enhanceCodeBlocks(wrap);
  }

  function renderPromptOnly() {
    const wrap = document.getElementById('promptWrap');
    if (!wrap) return renderMain();
    wrap.innerHTML = C.renderPromptSection(getCurrentStyle(), state);
    bindPromptEvents(wrap);
  }

  function showToast(title, desc, variant) {
    const container = document.getElementById('toastContainer');
    const el = document.createElement('div');
    el.className = `toast ${variant ? 'toast-' + variant : ''}`;
    el.innerHTML = `<div style="padding-right:18px"><div style="font-weight:600;margin-bottom:2px">${C.escapeHtml(title)}</div>${desc ? `<div class="muted-text">${C.escapeHtml(desc)}</div>` : ''}</div><button type="button" class="toast-close" aria-label="关闭">✕</button>`;
    container.appendChild(el);
    const timer = setTimeout(() => {
      if (!el.isConnected) return;
      el.style.opacity = '0';
      el.style.transform = 'translateY(6px)';
      el.style.transition = 'all 0.2s';
      setTimeout(() => el.remove(), 200);
    }, 3200);
    el.querySelector('.toast-close').onclick = () => {
      clearTimeout(timer);
      el.remove();
    };
    return el;
  }

  function openModal(id) {
    const el = document.getElementById(id);
    if (el) {
      el.hidden = false;
      const firstInput = el.querySelector('input, button');
      if (firstInput) setTimeout(() => firstInput.focus(), 20);
    }
  }
  function closeModal(id) {
    const el = document.getElementById(id);
    if (el) el.hidden = true;
  }

  function getFilteredTableRows() {
    const q = state.tableQuery.trim().toLowerCase();
    let rows = state.tableRows.filter(r =>
      r.name.toLowerCase().includes(q) ||
      r.owner.toLowerCase().includes(q) ||
      r.id.toLowerCase().includes(q)
    );
    if (state.tableSortDesc) {
      rows = [...rows].sort((a, b) => b.coverage - a.coverage);
    }
    return rows;
  }

  function syncTableSelectAll() {
    const selectAll = document.getElementById('tableSelectAll');
    if (!selectAll) return;
    const visible = state.tableMode === 'normal' ? getFilteredTableRows() : [];
    const checkedVisible = visible.filter(r => state.tableSelectedIds.has(r.id)).length;
    selectAll.checked = visible.length > 0 && checkedVisible === visible.length;
    selectAll.indeterminate = checkedVisible > 0 && checkedVisible < visible.length;
  }

  function renderTableRows() {
    const tbody = document.getElementById('dataTableBody');
    if (!tbody) return;
    const c = state.tableCols;
    document.querySelectorAll('#sec-table thead th[data-col]').forEach(th => {
      th.hidden = c[th.dataset.col] === false;
    });
    const colCount = 6 + (c.latency !== false ? 1 : 0) + (c.coverage !== false ? 1 : 0) + (c.updated !== false ? 1 : 0);

    if (state.tableMode === 'loading') {
      tbody.innerHTML = Array.from({ length: 4 }, () => `
        <tr>
          <td><div class="skeleton" style="width:15px;height:15px"></div></td>
          <td><div class="skeleton" style="width:54px;height:12px"></div></td>
          <td><div class="skeleton" style="width:120px;height:12px"></div></td>
          <td><div class="skeleton" style="width:68px;height:12px"></div></td>
          <td><div class="skeleton" style="width:56px;height:18px;border-radius:99px"></div></td>
          <td${c.latency === false ? ' hidden' : ''}><div class="skeleton" style="width:42px;height:12px"></div></td>
          <td${c.coverage === false ? ' hidden' : ''}><div class="skeleton" style="width:36px;height:12px"></div></td>
          <td${c.updated === false ? ' hidden' : ''}><div class="skeleton" style="width:52px;height:12px"></div></td>
          <td><div class="skeleton" style="width:24px;height:16px;margin-left:auto"></div></td>
        </tr>
      `).join('');
    } else if (state.tableMode === 'empty') {
      tbody.innerHTML = `
        <tr><td colspan="${colCount}" style="padding:36px 16px;text-align:center">
          <div style="font-weight:600;margin-bottom:4px">暂无匹配的设计系统记录 (Empty State)</div>
          <div class="muted-text" style="margin-bottom:12px">当前处于空状态演示视图，点击下方按钮恢复数据列表。</div>
          <button type="button" class="ui-btn ui-btn-outline ui-btn-sm" id="tableResetEmptyBtn">恢复全部记录</button>
        </td></tr>
      `;
    } else {
      const rows = getFilteredTableRows();
      const statusMap = {
        active: '<span class="ui-badge ui-badge-success"><span class="badge-dot"></span>Active</span>',
        review: '<span class="ui-badge ui-badge-warn">In Review</span>',
        paused: '<span class="ui-badge">Paused</span>'
      };
      tbody.innerHTML = rows.length ? rows.map(r => `
        <tr>
          <td><input type="checkbox" class="ui-checkbox row-cb" data-row-id="${r.id}" ${state.tableSelectedIds.has(r.id) ? 'checked' : ''} /></td>
          <td class="mono">${r.id}</td>
          <td><strong>${C.escapeHtml(r.name)}</strong></td>
          <td>${C.escapeHtml(r.owner)}</td>
          <td>${statusMap[r.status] || r.status}</td>
          <td class="tabular mono"${c.latency === false ? ' hidden' : ''}>${r.latency}</td>
          <td class="tabular"${c.coverage === false ? ' hidden' : ''}>${r.coverage}%</td>
          <td class="muted-text"${c.updated === false ? ' hidden' : ''}>${r.updated}</td>
          <td style="text-align:right">
            <div class="ui-dropdown" data-dropdown>
              <button type="button" class="ui-btn ui-btn-ghost ui-btn-sm" data-dropdown-trigger aria-expanded="false" aria-label="行操作">⋯</button>
              <div class="ui-dropdown-menu" role="menu" hidden style="min-width:144px;right:0;left:auto">
                <button type="button" class="ui-dropdown-item" role="menuitem" data-row-act="view" data-row-id="${r.id}"><span>查看详情</span></button>
                <button type="button" class="ui-dropdown-item" role="menuitem" data-row-act="copy" data-row-id="${r.id}"><span>复制 ID</span></button>
                <button type="button" class="ui-dropdown-item" role="menuitem" data-row-act="cycle" data-row-id="${r.id}"><span>切换状态</span></button>
                <div class="ui-dropdown-divider"></div>
                <button type="button" class="ui-dropdown-item danger" role="menuitem" data-row-act="delete" data-row-id="${r.id}"><span>删除记录</span></button>
              </div>
            </div>
          </td>
        </tr>
      `).join('') : `
        <tr><td colspan="${colCount}" style="text-align:center;padding:28px">
          <div class="muted-text" style="margin-bottom:8px">没有匹配「${C.escapeHtml(state.tableQuery)}」的记录</div>
          <button type="button" class="ui-btn ui-btn-outline ui-btn-sm" id="tableResetEmptyBtn">清空筛选条件</button>
        </td></tr>
      `;
    }

    const cntEl = document.getElementById('tableSelectedCount');
    if (cntEl) cntEl.textContent = `已选 ${state.tableSelectedIds.size} 行`;
    syncTableSelectAll();

    const resetBtn = document.getElementById('tableResetEmptyBtn');
    if (resetBtn) {
      resetBtn.onclick = () => {
        state.tableMode = 'normal';
        state.tableQuery = '';
        const tSearch = document.getElementById('tableSearchInput');
        if (tSearch) tSearch.value = '';
        document.querySelectorAll('[data-table-mode]').forEach(b => b.classList.toggle('active', b.dataset.tableMode === 'normal'));
        renderTableRows();
      };
    }

    bindCustomSelectsAndDropdowns(tbody);
    tbody.querySelectorAll('[data-row-act]').forEach(btn => {
      btn.addEventListener('click', () => {
        const id = btn.dataset.rowId;
        const act = btn.dataset.rowAct;
        const row = state.tableRows.find(r => r.id === id);
        if (!row) return;
        if (act === 'view') {
          showToast(`${row.name} (${row.id})`, `负责人: ${row.owner} · 覆盖率 ${row.coverage}% · 延迟 ${row.latency}`, 'info');
        } else if (act === 'copy') {
          copyText(row.id, '已复制行 ID', row.id);
        } else if (act === 'cycle') {
          const order = ['active', 'review', 'paused'];
          row.status = order[(order.indexOf(row.status) + 1) % order.length];
          renderTableRows();
          showToast('状态已更新', `${row.name} -> ${row.status}`, 'success');
        } else if (act === 'delete') {
          state.pendingDeleteId = row.id;
          const targetEl = document.getElementById('alertDialogTarget');
          if (targetEl) targetEl.textContent = ` 待删除目标：${row.name} (${row.id})。`;
          openModal('alertDialogModal');
        }
      });
    });
  }

  function renderCombobox() {
    const tagsEl = document.getElementById('comboboxTags');
    const dropdownEl = document.getElementById('comboboxDropdown');
    const inputEl = document.getElementById('comboboxInput');
    if (!tagsEl || !dropdownEl || !inputEl) return;

    tagsEl.innerHTML = state.comboboxSelected.map(item => `
      <span class="ui-badge">${C.escapeHtml(item.split(' (')[0])}
        <button type="button" class="badge-close" data-remove-tag="${C.escapeHtml(item)}">✕</button>
      </span>
    `).join('');

    const q = inputEl.value.trim().toLowerCase();
    const filtered = D.comboboxOptions.filter(opt => opt.toLowerCase().includes(q));
    dropdownEl.innerHTML = filtered.length ? filtered.map(opt => {
      const sel = state.comboboxSelected.includes(opt);
      return `<button type="button" class="combobox-option ${sel ? 'active' : ''}" data-combo-opt="${C.escapeHtml(opt)}">
        <span>${C.escapeHtml(opt)}</span>
        <span>${sel ? '✓' : '+'}</span>
      </button>`;
    }).join('') : `<div class="muted-text" style="padding:8px 10px;text-align:center">无匹配项</div>`;
  }

  function closeAllFloatingMenus(exceptEl) {
    document.querySelectorAll('[data-select]').forEach(sel => {
      if (sel !== exceptEl) {
        const trig = sel.querySelector('.ui-select-trigger');
        const menu = sel.querySelector('.ui-select-menu');
        if (trig) trig.setAttribute('aria-expanded', 'false');
        if (menu) menu.hidden = true;
      }
    });
    document.querySelectorAll('[data-dropdown]').forEach(dd => {
      if (dd !== exceptEl) {
        const trig = dd.querySelector('[data-dropdown-trigger]');
        const menu = dd.querySelector('.ui-dropdown-menu');
        if (trig) trig.setAttribute('aria-expanded', 'false');
        if (menu) menu.hidden = true;
      }
    });
    const comboDrop = document.getElementById('comboboxDropdown');
    const comboWrap = document.getElementById('comboboxWrap');
    if (comboDrop && comboWrap !== exceptEl) comboDrop.hidden = true;
    const popPanel = document.getElementById('demoPopover');
    if (popPanel && popPanel !== exceptEl) popPanel.hidden = true;
    if (customContextMenu && customContextMenu !== exceptEl) customContextMenu.hidden = true;
  }

  function bindCustomSelectsAndDropdowns(root) {
    root.querySelectorAll('[data-select]').forEach(sel => {
      const trig = sel.querySelector('.ui-select-trigger');
      const menu = sel.querySelector('.ui-select-menu');
      const valSpan = sel.querySelector('.ui-select-value');
      const options = [...sel.querySelectorAll('.ui-select-option')];
      if (!trig || !menu) return;

      trig.onclick = e => {
        e.stopPropagation();
        const isOpen = !menu.hidden;
        closeAllFloatingMenus(sel);
        menu.hidden = isOpen;
        trig.setAttribute('aria-expanded', String(!isOpen));
      };

      const selectOption = opt => {
        const val = opt.dataset.value;
        const label = opt.querySelector('span').textContent;
        sel.dataset.value = val;
        if (valSpan) valSpan.textContent = label;
        options.forEach(o => {
          const isSel = o === opt;
          o.classList.toggle('selected', isSel);
          o.setAttribute('aria-selected', String(isSel));
        });
        if (sel.id === 'formPrimitiveSelect') {
          state.selectPrimitive = val;
          state.selectPrimitiveLabel = label;
        } else if (sel.id === 'formTimeSelect') {
          state.timeSlot = val;
        }
        menu.hidden = true;
        trig.setAttribute('aria-expanded', 'false');
        trig.focus();
      };

      options.forEach(opt => {
        opt.onclick = e => {
          e.stopPropagation();
          selectOption(opt);
        };
      });

      trig.onkeydown = e => {
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
          e.preventDefault();
          closeAllFloatingMenus(sel);
          menu.hidden = false;
          trig.setAttribute('aria-expanded', 'true');
          const curIdx = options.findIndex(o => o.classList.contains('selected'));
          const nextIdx = e.key === 'ArrowDown'
            ? (curIdx + 1) % options.length
            : (curIdx - 1 + options.length) % options.length;
          selectOption(options[nextIdx]);
        }
      };
    });

    root.querySelectorAll('[data-dropdown]').forEach(dd => {
      const trig = dd.querySelector('[data-dropdown-trigger]');
      const menu = dd.querySelector('.ui-dropdown-menu');
      if (!trig || !menu) return;
      trig.onclick = e => {
        e.stopPropagation();
        const isOpen = !menu.hidden;
        closeAllFloatingMenus(dd);
        menu.hidden = isOpen;
        trig.setAttribute('aria-expanded', String(!isOpen));
      };
      if (menu.querySelector('[data-table-col]')) {
        menu.onclick = e => e.stopPropagation();
      }
      menu.querySelectorAll('.ui-dropdown-item').forEach(item => {
        item.addEventListener('click', () => {
          menu.hidden = true;
          trig.setAttribute('aria-expanded', 'false');
        });
      });
    });
  }

  function syncIndeterminate() {
    const parent = document.getElementById('cbParent');
    const children = [...document.querySelectorAll('.cb-child')];
    if (!parent || !children.length) return;
    const checkedCount = children.filter(c => c.checked).length;
    parent.checked = checkedCount === children.length;
    parent.indeterminate = checkedCount > 0 && checkedCount < children.length;
  }

  function enhanceCodeBlocks(root) {
    root.querySelectorAll('pre.code-block').forEach(pre => {
      if (pre.parentElement.classList.contains('code-wrap')) return;
      const wrap = document.createElement('div');
      wrap.className = 'code-wrap';
      pre.parentNode.insertBefore(wrap, pre);
      wrap.appendChild(pre);
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'code-copy-btn';
      btn.textContent = '复制';
      btn.onclick = () => copyText(pre.textContent, '代码已复制', '可直接粘贴到你的项目中使用。');
      wrap.appendChild(btn);
    });
  }

  function bindPromptEvents(root) {
    root.querySelectorAll('[data-prompt-mode]').forEach(btn => {
      btn.onclick = () => {
        state.promptMode = btn.dataset.promptMode;
        renderPromptOnly();
      };
    });
    root.querySelectorAll('[data-prompt-opt]').forEach(cb => {
      cb.onchange = () => {
        state[cb.dataset.promptOpt] = cb.checked;
        renderPromptOnly();
      };
    });
    const copyBtn = root.querySelector('#copyAgentPromptBtn');
    if (copyBtn) {
      copyBtn.onclick = () => {
        const s = getCurrentStyle();
        const text = C.buildAgentPrompt(s, state);
        copyText(text, `已复制 ${s.name} 提示词`, '可直接粘贴到 AI Agent 对话中执行开发或重构。');
      };
    }
    const ruleBtn = root.querySelector('#copyPromptRuleBtn');
    if (ruleBtn) {
      ruleBtn.onclick = () => {
        const s = getCurrentStyle();
        const text = `---\ndescription: ${s.name} UI/UX Rules & Guardrails\nalwaysApply: true\n---\n\n${C.buildAgentPrompt(s, state)}`;
        copyText(text, `已复制 ${s.name} Rules 片段`, '可直接保存为 .cursor/rules 或 .gemini/rules Markdown 文件。');
      };
    }
  }

  function bindSignatureEvents(root) {
    // Style-specific triggers (scoped to #signatureWrap to avoid losing form inputs)
    root.querySelectorAll('[data-coss-slot]').forEach(btn => {
      btn.onclick = () => { state.cossSlot = btn.dataset.cossSlot; renderSignatureOnly(); };
    });
    root.querySelectorAll('[data-coss-dur]').forEach(btn => {
      btn.onclick = () => { state.cossDuration = btn.dataset.cossDur; renderSignatureOnly(); };
    });
    root.querySelectorAll('[data-untitled-edge]').forEach(btn => {
      btn.onclick = () => { state.untitledEdge = btn.dataset.untitledEdge; renderSignatureOnly(); };
    });
    root.querySelectorAll('[data-tremor-pt]').forEach(pt => {
      pt.onmouseenter = () => {
        const el = document.getElementById('tremorHoverVal');
        if (el) el.textContent = pt.dataset.tremorPt;
      };
    });
    root.querySelectorAll('[data-tremor-bar]').forEach(bar => {
      bar.onmouseenter = () => {
        const el = document.getElementById('tremorBarVal');
        if (el) el.textContent = bar.dataset.tremorBar;
      };
    });
    root.querySelectorAll('[data-tremor-donut]').forEach(seg => {
      seg.onmouseenter = () => {
        const el = document.getElementById('tremorDonutVal');
        if (el) el.textContent = seg.dataset.tremorDonut;
      };
    });

    // Kibo Kanban
    root.querySelectorAll('[data-kanban-move]').forEach(btn => {
      btn.onclick = () => {
        const id = btn.dataset.kanbanMove;
        const dir = Number(btn.dataset.dir);
        for (let i = 0; i < state.kanban.length; i++) {
          const idx = state.kanban[i].items.findIndex(x => x.id === id);
          if (idx !== -1) {
            const [item] = state.kanban[i].items.splice(idx, 1);
            state.kanban[i + dir].items.push(item);
            break;
          }
        }
        renderSignatureOnly();
      };
    });

    // Kibo Tree
    root.querySelectorAll('[data-tree-toggle]').forEach(el => {
      el.onclick = () => {
        const children = el.nextElementSibling;
        if (children) children.hidden = !children.hidden;
      };
    });

    // Kibo AI Chat
    const aiSend = root.querySelector('#kiboAiSendBtn');
    const aiInput = root.querySelector('#kiboAiInput');
    const aiBox = root.querySelector('#kiboAiMessages');
    if (aiSend && aiInput && aiBox) {
      const sendAi = () => {
        const text = aiInput.value.trim();
        if (!text) return;
        state.aiMessages.push({ role: 'user', text });
        state.aiMessages.push({ role: 'bot', text: `已收到「${text}」，正在使用 Kibo UI + shadcn Tokens 生成对应组件代码...` });
        aiInput.value = '';
        aiBox.innerHTML = state.aiMessages.map(m => `<div class="ai-msg ${m.role}">${C.escapeHtml(m.text)}</div>`).join('');
        aiBox.scrollTop = aiBox.scrollHeight;
      };
      aiSend.onclick = sendAi;
      aiInput.onkeydown = e => { if (e.key === 'Enter') sendAi(); };
    }

    // Park UI controls
    root.querySelectorAll('[data-park-radius]').forEach(b => {
      b.onclick = () => { state.parkRadius = b.dataset.parkRadius; applyThemeAndTokens(); renderSignatureOnly(); };
    });
    root.querySelectorAll('[data-park-accent]').forEach(b => {
      b.onclick = () => { state.parkAccent = b.dataset.parkAccent; applyThemeAndTokens(); showToast('Park UI Accent 已更新', b.dataset.parkAccent, 'info'); };
    });
    root.querySelectorAll('[data-park-fw]').forEach(b => {
      b.onclick = () => { state.parkFw = b.dataset.parkFw; renderSignatureOnly(); };
    });

    // Basecoat HTML inspect
    const bcBtn = root.querySelector('#toggleBasecoatHtmlBtn');
    if (bcBtn) {
      bcBtn.onclick = () => { state.basecoatShowHtml = !state.basecoatShowHtml; renderSignatureOnly(); };
    }

    // Radix controls
    root.querySelectorAll('[data-radix-accent]').forEach(b => {
      b.onclick = () => { state.radixAccent = b.dataset.radixAccent; applyThemeAndTokens(); showToast('Radix Accent 已更新', b.dataset.radixAccent, 'info'); };
    });
    root.querySelectorAll('[data-radix-radius]').forEach(b => {
      b.onclick = () => { state.radixRadius = b.dataset.radixRadius; applyThemeAndTokens(); renderSignatureOnly(); };
    });
    root.querySelectorAll('[data-radix-scale]').forEach(b => {
      b.onclick = () => { state.radixScale = Number(b.dataset.radixScale); applyThemeAndTokens(); renderSignatureOnly(); };
    });

    // Reshaped grid
    const swissBtn = root.querySelector('#toggleSwissGridBtn');
    if (swissBtn) swissBtn.onclick = () => { state.showSwissGrid = !state.showSwissGrid; renderSignatureOnly(); };

    // Base UI unstyled toggle & logs
    const unstyledBtn = root.querySelector('#toggleUnstyledBtn');
    if (unstyledBtn) {
      unstyledBtn.onclick = () => {
        state.baseUiUnstyled = !state.baseUiUnstyled;
        state.baseUiLogs.unshift(`[Mode] unstyled=${state.baseUiUnstyled}`);
        renderSignatureOnly();
      };
    }
    root.querySelectorAll('[data-base-log]').forEach(el => {
      el.addEventListener('click', () => {
        state.baseUiLogs.unshift(`[Event] ${el.dataset.baseLog} @ ${new Date().toLocaleTimeString()}`);
        const box = document.getElementById('baseUiLogBox');
        if (box) box.textContent = state.baseUiLogs.slice(0, 6).join('\n');
      });
    });

    // React Aria Announcer
    root.querySelectorAll('[data-aria-announce]').forEach(el => {
      el.addEventListener('click', () => {
        state.ariaAnnouncement = el.dataset.ariaAnnounce;
        const box = document.getElementById('ariaAnnouncerBox');
        if (box) box.querySelector('.ui-alert-desc').textContent = state.ariaAnnouncement;
      });
    });

    // Rebind modal/toast triggers inside signature section
    root.querySelectorAll('[data-action="toast"]').forEach(btn => {
      btn.onclick = () => showToast(btn.dataset.toastTitle || '操作成功', btn.dataset.toastDesc || '', btn.dataset.toastVariant);
    });
    root.querySelectorAll('[data-action="open-modal"]').forEach(btn => {
      btn.onclick = () => openModal(btn.dataset.modal);
    });
    root.querySelectorAll('[data-switch]').forEach(sw => {
      sw.onclick = () => {
        const next = sw.getAttribute('aria-checked') !== 'true';
        sw.setAttribute('aria-checked', String(next));
      };
    });
  }

  function bindCanvasEvents() {
    const sigWrap = document.getElementById('signatureWrap');
    if (sigWrap) bindSignatureEvents(sigWrap);
    const promptWrap = document.getElementById('promptWrap');
    if (promptWrap) bindPromptEvents(promptWrap);
    enhanceCodeBlocks(mainCanvas);

    // Kitchen Sink: Toast, Async Promise Toast, Toggle Group, Context Menu & Modals
    document.querySelectorAll('[data-action="toast"]').forEach(btn => {
      btn.onclick = () => showToast(btn.dataset.toastTitle || '操作成功', btn.dataset.toastDesc || '', btn.dataset.toastVariant);
    });
    const promiseToastBtn = document.getElementById('asyncPromiseToastBtn');
    if (promiseToastBtn) {
      promiseToastBtn.onclick = () => {
        const t = showToast('正在同步 Design Tokens...', '请稍候，正在校验变量对比度', 'info');
        setTimeout(() => {
          if (t && t.isConnected) {
            t.className = 'toast toast-success';
            if (t.firstElementChild) {
              t.firstElementChild.innerHTML = `<div style="font-weight:600;margin-bottom:2px">Token 同步完成</div><div class="muted-text">10 套风格校验全部通过 (0 警告)</div>`;
            }
          }
        }, 1100);
      };
    }
    const exportCssBtn = document.getElementById('exportFullCssBtn');
    if (exportCssBtn) {
      exportCssBtn.onclick = () => {
        const s = getCurrentStyle();
        const guide = D.promptGuides && D.promptGuides[s.id];
        if (!guide || !guide.dualTokens) return;
        const cssText = [
          `/* ${s.name} Light + Dark CSS Design Tokens */`,
          ':root {',
          ...Object.entries(guide.dualTokens.light).map(([k, v]) => `  ${k}: ${v};`),
          '}',
          '[data-theme="dark"] {',
          ...Object.entries(guide.dualTokens.dark).map(([k, v]) => `  ${k}: ${v};`),
          '}'
        ].join('\n');
        copyText(cssText, `已导出 ${s.name} 双色 CSS Tokens`, '可直接粘贴到 globals.css');
      };
    }
    document.querySelectorAll('[data-toggle-btn]').forEach(btn => {
      btn.onclick = () => btn.classList.toggle('active');
    });
    const ctxZone = document.getElementById('demoContextZone');
    if (ctxZone && customContextMenu) {
      ctxZone.oncontextmenu = e => {
        e.preventDefault();
        e.stopPropagation();
        closeAllFloatingMenus(customContextMenu);
        customContextMenu.hidden = false;
        const menuW = 230;
        const menuH = 165;
        const x = Math.min(e.clientX, window.innerWidth - menuW - 8);
        const y = Math.min(e.clientY, window.innerHeight - menuH - 8);
        customContextMenu.style.left = `${x}px`;
        customContextMenu.style.top = `${y}px`;
      };
    }
    document.querySelectorAll('[data-action="open-modal"]').forEach(btn => {
      btn.onclick = () => {
        if (btn.dataset.modal === 'alertDialogModal') {
          state.pendingDeleteId = null;
          const targetEl = document.getElementById('alertDialogTarget');
          if (targetEl) targetEl.textContent = '';
        }
        openModal(btn.dataset.modal);
      };
    });
    const cmd2 = document.getElementById('openCmdTrigger2');
    if (cmd2) cmd2.onclick = openCmdPalette;

    // Async Loading Button
    const asyncBtn = document.getElementById('asyncLoadingBtn');
    if (asyncBtn) {
      asyncBtn.onclick = () => {
        asyncBtn.disabled = true;
        asyncBtn.innerHTML = `<span class="spinner"></span> 正在提交...`;
        setTimeout(() => {
          asyncBtn.disabled = false;
          asyncBtn.textContent = '点击触发异步 Loading';
          showToast('异步请求完成', '数据已成功保存。');
        }, 1200);
      };
    }

    // Clearable & Password
    const clearBtn = document.getElementById('clearInputBtn');
    const clearInp = document.getElementById('clearableInput');
    if (clearBtn && clearInp) {
      clearInp.oninput = () => { state.clearableVal = clearInp.value; };
      clearBtn.onclick = () => { state.clearableVal = ''; clearInp.value = ''; clearInp.focus(); };
    }

    const pwdBtn = document.getElementById('togglePwdBtn');
    const pwdInp = document.getElementById('pwdInput');
    if (pwdBtn && pwdInp) {
      pwdBtn.onclick = () => {
        const show = pwdInp.type === 'password';
        pwdInp.type = show ? 'text' : 'password';
        pwdBtn.textContent = show ? '隐藏' : '显示';
      };
    }

    // Textarea count
    const ta = document.getElementById('demoTextarea');
    const taCnt = document.getElementById('textareaCount');
    if (ta && taCnt) {
      ta.oninput = () => {
        state.textareaVal = ta.value;
        taCnt.textContent = `${ta.value.length} / 120`;
      };
    }

    // Switches
    document.querySelectorAll('[data-switch]').forEach(sw => {
      sw.onclick = () => {
        const next = sw.getAttribute('aria-checked') !== 'true';
        sw.setAttribute('aria-checked', String(next));
      };
    });

    // Custom Selects & Dropdown Menus inside canvas
    bindCustomSelectsAndDropdowns(mainCanvas);

    // Slider
    const slider = document.getElementById('demoSlider');
    const sliderRead = document.getElementById('sliderReadout');
    if (slider && sliderRead) slider.oninput = () => {
      state.sliderVal = Number(slider.value);
      sliderRead.textContent = `${state.sliderVal}%`;
    };

    // Stepper & Dual-Thumb Range Slider & Rating
    const syncDualRange = () => {
      const fill = document.getElementById('dualRangeFill');
      const readout = document.getElementById('dualRangeReadout');
      if (fill) {
        fill.style.left = `${(state.rangeMin / 1000) * 100}%`;
        fill.style.right = `${100 - (state.rangeMax / 1000) * 100}%`;
      }
      if (readout) readout.textContent = `¥${state.rangeMin} – ¥${state.rangeMax}`;
    };
    const rMin = document.getElementById('dualRangeMin');
    const rMax = document.getElementById('dualRangeMax');
    if (rMin && rMax) {
      rMin.oninput = () => {
        state.rangeMin = Math.min(Number(rMin.value), state.rangeMax - 20);
        rMin.value = state.rangeMin;
        syncDualRange();
      };
      rMax.oninput = () => {
        state.rangeMax = Math.max(Number(rMax.value), state.rangeMin + 20);
        rMax.value = state.rangeMax;
        syncDualRange();
      };
    }

    const stepVal = document.getElementById('stepVal');
    const stepValBox = document.getElementById('stepValBox');
    const stepM = document.getElementById('stepMinus');
    const stepP = document.getElementById('stepPlus');
    if (stepVal && stepM && stepP) {
      const updateStep = () => {
        stepVal.textContent = state.stepperVal;
        if (stepValBox) stepValBox.textContent = state.stepperVal;
      };
      stepM.onclick = () => { state.stepperVal = Math.max(1, state.stepperVal - 1); updateStep(); };
      stepP.onclick = () => { state.stepperVal = Math.min(32, state.stepperVal + 1); updateStep(); };
    }

    const ratingStars = [...document.querySelectorAll('[data-rate]')];
    const ratingRead = document.getElementById('ratingReadout');
    ratingStars.forEach(btn => {
      btn.onclick = () => {
        state.rating = Number(btn.dataset.rate);
        ratingStars.forEach(s => s.classList.toggle('filled', Number(s.dataset.rate) <= state.rating));
        if (ratingRead) ratingRead.textContent = `${state.rating}.0`;
      };
    });

    // OTP
    const otpBoxes = [...document.querySelectorAll('.otp-box')];
    otpBoxes.forEach((box, idx) => {
      box.oninput = () => {
        box.value = box.value.replace(/\D/g, '').slice(0, 1);
        if (box.value && otpBoxes[idx + 1]) otpBoxes[idx + 1].focus();
      };
      box.onkeydown = e => {
        if (e.key === 'Backspace' && !box.value && otpBoxes[idx - 1]) otpBoxes[idx - 1].focus();
      };
    });

    // Indeterminate Checkboxes
    const cbParent = document.getElementById('cbParent');
    const cbChildren = [...document.querySelectorAll('.cb-child')];
    if (cbParent) {
      cbParent.onchange = () => {
        cbChildren.forEach(c => { c.checked = cbParent.checked; });
        syncIndeterminate();
      };
      cbChildren.forEach(c => { c.onchange = syncIndeterminate; });
    }

    // Combobox
    const comboWrap = document.getElementById('comboboxWrap');
    const comboInput = document.getElementById('comboboxInput');
    const comboDrop = document.getElementById('comboboxDropdown');
    if (comboWrap && comboInput && comboDrop) {
      comboInput.onfocus = () => { closeAllFloatingMenus(comboWrap); comboDrop.hidden = false; renderCombobox(); };
      comboInput.oninput = () => { comboDrop.hidden = false; renderCombobox(); };
      comboWrap.onclick = e => {
        e.stopPropagation();
        const rm = e.target.closest('[data-remove-tag]');
        if (rm) {
          state.comboboxSelected = state.comboboxSelected.filter(x => x !== rm.dataset.removeTag);
          renderCombobox();
          return;
        }
        const opt = e.target.closest('[data-combo-opt]');
        if (opt) {
          const val = opt.dataset.comboOpt;
          if (state.comboboxSelected.includes(val)) {
            state.comboboxSelected = state.comboboxSelected.filter(x => x !== val);
          } else {
            state.comboboxSelected.push(val);
          }
          renderCombobox();
        }
      };
    }

    // Radio cards & segmented (in-place update)
    document.querySelectorAll('[data-radio-plan]').forEach(card => {
      card.onclick = () => {
        state.radioPlan = card.dataset.radioPlan;
        document.querySelectorAll('[data-radio-plan]').forEach(c => c.classList.toggle('selected', c === card));
        const lbl = document.getElementById('radioPlanLabel');
        if (lbl) lbl.textContent = state.radioPlan.toUpperCase();
      };
    });
    document.querySelectorAll('[data-seg]').forEach(btn => {
      btn.onclick = () => {
        btn.parentElement.querySelectorAll('.tab-btn').forEach(x => x.classList.remove('active'));
        btn.classList.add('active');
      };
    });

    // Calendar Single / Range (in-place update)
    const syncCalendarUI = () => {
      document.querySelectorAll('[data-cal-mode]').forEach(m => m.classList.toggle('active', m.dataset.calMode === state.calMode));
      document.querySelectorAll('[data-cal-day]').forEach(b => {
        const day = Number(b.dataset.calDay);
        const isEdge = state.calMode === 'range' ? (day === state.rangeStart || day === state.rangeEnd) : (day === state.selectedDay);
        const inRange = state.calMode === 'range' && day > state.rangeStart && day < state.rangeEnd;
        b.classList.toggle('selected', isEdge);
        b.classList.toggle('range-edge', isEdge);
        b.classList.toggle('in-range', inRange);
      });
      const lbl = document.getElementById('selectedDateLabel');
      if (lbl) {
        if (state.calMode === 'range') {
          lbl.textContent = state.rangePendingStart
            ? `选结束日 (起点 11-${String(state.rangePendingStart).padStart(2, '0')})`
            : `11-${String(state.rangeStart).padStart(2, '0')} → 11-${String(state.rangeEnd).padStart(2, '0')}`;
        } else {
          lbl.textContent = `2026-11-${String(state.selectedDay).padStart(2, '0')}`;
        }
      }
    };
    document.querySelectorAll('[data-cal-mode]').forEach(btn => {
      btn.onclick = () => {
        state.calMode = btn.dataset.calMode;
        state.rangePendingStart = null;
        syncCalendarUI();
      };
    });
    document.querySelectorAll('[data-cal-preset]').forEach(btn => {
      btn.onclick = () => {
        state.calMode = 'range';
        state.rangePendingStart = null;
        if (btn.dataset.calPreset === 'week') { state.rangeStart = 8; state.rangeEnd = 14; }
        else { state.rangeStart = 1; state.rangeEnd = 30; }
        syncCalendarUI();
      };
    });
    document.querySelectorAll('[data-cal-day]').forEach(btn => {
      btn.onclick = () => {
        const d = Number(btn.dataset.calDay);
        if (state.calMode === 'single') {
          state.selectedDay = d;
        } else if (!state.rangePendingStart) {
          state.rangePendingStart = d;
          state.rangeStart = d;
          state.rangeEnd = d;
        } else {
          state.rangeStart = Math.min(state.rangePendingStart, d);
          state.rangeEnd = Math.max(state.rangePendingStart, d);
          state.rangePendingStart = null;
        }
        syncCalendarUI();
      };
    });

    // Dropzone
    const dz = document.getElementById('demoDropzone');
    const dzInput = document.getElementById('dropzoneFileInput');
    const dzText = document.getElementById('dropzoneText');
    if (dz && dzInput && dzText) {
      dz.onclick = () => dzInput.click();
      dzInput.onchange = () => {
        if (dzInput.files[0]) {
          dzText.textContent = `已选择: ${dzInput.files[0].name}`;
          showToast('文件已就绪', dzInput.files[0].name);
        }
      };
      dz.ondragover = e => { e.preventDefault(); dz.classList.add('dragover'); };
      dz.ondragleave = () => dz.classList.remove('dragover');
      dz.ondrop = e => {
        e.preventDefault(); dz.classList.remove('dragover');
        const f = e.dataTransfer.files[0];
        if (f) { dzText.textContent = `已拖入: ${f.name}`; showToast('文件已上传', f.name); }
      };
    }

    // Tabs
    document.querySelectorAll('[data-tab-group]').forEach(btn => {
      btn.onclick = () => {
        const g = btn.dataset.tabGroup;
        const t = btn.dataset.tabTarget;
        document.querySelectorAll(`[data-tab-group="${g}"]`).forEach(x => x.classList.remove('active'));
        btn.classList.add('active');
        document.querySelectorAll(`[data-tab-panel="${g}"]`).forEach(p => { p.hidden = p.id !== t; });
      };
    });

    // Accordion
    document.querySelectorAll('[data-acc-btn]').forEach(btn => {
      btn.onclick = () => {
        const body = btn.nextElementSibling;
        const open = !body.hidden;
        body.hidden = open;
        btn.querySelector('span').textContent = open ? '+' : '−';
      };
    });

    // Popover
    const popBtn = document.getElementById('popoverTrigger');
    const popPanel = document.getElementById('demoPopover');
    if (popBtn && popPanel) {
      popBtn.onclick = e => {
        e.stopPropagation();
        const willOpen = popPanel.hidden;
        closeAllFloatingMenus(popPanel);
        popPanel.hidden = !willOpen;
      };
      popPanel.onclick = e => e.stopPropagation();
    }

    // Progress & Skeleton
    const skelBtn = document.getElementById('toggleSkeletonBtn');
    const progBox = document.getElementById('progressDemoContent');
    const skelBox = document.getElementById('skeletonDemoContent');
    if (skelBtn && progBox && skelBox) {
      skelBtn.onclick = () => {
        const showSkel = !skelBox.hidden;
        skelBox.hidden = showSkel;
        progBox.hidden = !showSkel;
      };
    }
    const incProg = document.getElementById('incProgressBtn');
    const rstProg = document.getElementById('resetProgressBtn');
    const fill = document.getElementById('progressFill');
    const pct = document.getElementById('progressPct');
    if (incProg && rstProg && fill && pct) {
      incProg.onclick = () => {
        state.progress = Math.min(100, state.progress + 10);
        fill.style.width = `${state.progress}%`;
        pct.textContent = `${state.progress}%`;
      };
      rstProg.onclick = () => {
        state.progress = 20;
        fill.style.width = '20%';
        pct.textContent = '20%';
      };
    }

    // Pagination
    document.querySelectorAll('#demoPagination .page-btn').forEach(btn => {
      btn.onclick = () => {
        document.querySelectorAll('#demoPagination .page-btn').forEach(x => x.classList.remove('active'));
        btn.classList.add('active');
        const lbl = document.getElementById('pageNumLabel');
        if (lbl) lbl.textContent = btn.dataset.page;
      };
    });

    // Steps Wizard (in-place update)
    const syncWizardUI = () => {
      const total = D.wizardSteps.length;
      state.wizardStep = Math.max(1, Math.min(total, state.wizardStep));
      const cur = D.wizardSteps[state.wizardStep - 1];
      document.querySelectorAll('[data-wizard-step]').forEach(btn => {
        const s = Number(btn.dataset.wizardStep);
        btn.classList.toggle('done', s < state.wizardStep);
        btn.classList.toggle('active', s === state.wizardStep);
        const numEl = btn.querySelector('.step-num');
        if (numEl) numEl.textContent = s < state.wizardStep ? '✓' : String(s);
      });
      const counter = document.getElementById('wizardStepCounter');
      if (counter) counter.textContent = `步骤 ${state.wizardStep} / ${total}`;
      const body = document.getElementById('wizardStepBody');
      if (body && cur) {
        body.innerHTML = `<strong>Step ${cur.step}: ${C.escapeHtml(cur.title)}</strong><div class="muted-text" style="margin-top:4px">${C.escapeHtml(cur.desc)}</div>`;
      }
      const prevBtn = document.getElementById('wizardPrevBtn');
      const nextBtn = document.getElementById('wizardNextBtn');
      if (prevBtn) prevBtn.disabled = state.wizardStep <= 1;
      if (nextBtn) nextBtn.textContent = state.wizardStep >= total ? '完成校验 ✓' : '下一步 →';
    };
    document.querySelectorAll('[data-wizard-step]').forEach(btn => {
      btn.onclick = () => { state.wizardStep = Number(btn.dataset.wizardStep); syncWizardUI(); };
    });
    const wizPrev = document.getElementById('wizardPrevBtn');
    const wizNext = document.getElementById('wizardNextBtn');
    if (wizPrev) wizPrev.onclick = () => { state.wizardStep--; syncWizardUI(); };
    if (wizNext) wizNext.onclick = () => {
      if (state.wizardStep >= D.wizardSteps.length) {
        showToast('分步向导已完成', '所有 Design Tokens、原语封装与离线校验已就绪。', 'success');
      } else {
        state.wizardStep++;
        syncWizardUI();
      }
    };

    // Data Table
    const tSearch = document.getElementById('tableSearchInput');
    if (tSearch) {
      tSearch.oninput = () => {
        state.tableQuery = tSearch.value;
        if (state.tableMode !== 'normal') {
          state.tableMode = 'normal';
          document.querySelectorAll('[data-table-mode]').forEach(b => b.classList.toggle('active', b.dataset.tableMode === 'normal'));
        }
        renderTableRows();
      };
    }
    document.querySelectorAll('[data-table-mode]').forEach(btn => {
      btn.onclick = () => {
        state.tableMode = btn.dataset.tableMode;
        document.querySelectorAll('[data-table-mode]').forEach(b => b.classList.toggle('active', b === btn));
        renderTableRows();
      };
    });
    document.querySelectorAll('[data-table-col]').forEach(cb => {
      cb.onchange = () => {
        state.tableCols[cb.dataset.tableCol] = cb.checked;
        renderTableRows();
      };
    });
    const sortBtn = document.getElementById('sortCoverageBtn');
    if (sortBtn) {
      sortBtn.onclick = () => { state.tableSortDesc = !state.tableSortDesc; renderTableRows(); };
    }
    const selectAll = document.getElementById('tableSelectAll');
    if (selectAll) {
      selectAll.onchange = () => {
        const visible = state.tableMode === 'normal' ? getFilteredTableRows() : [];
        if (selectAll.checked) visible.forEach(r => state.tableSelectedIds.add(r.id));
        else visible.forEach(r => state.tableSelectedIds.delete(r.id));
        renderTableRows();
      };
    }
    const tbody = document.getElementById('dataTableBody');
    if (tbody) {
      tbody.onchange = e => {
        if (e.target.classList.contains('row-cb')) {
          const id = e.target.dataset.rowId;
          if (e.target.checked) state.tableSelectedIds.add(id);
          else state.tableSelectedIds.delete(id);
          const cntEl = document.getElementById('tableSelectedCount');
          if (cntEl) cntEl.textContent = `已选 ${state.tableSelectedIds.size} 行`;
          syncTableSelectAll();
        }
      };
    }

    // Matrix select buttons
    document.querySelectorAll('[data-select-style]').forEach(btn => {
      btn.onclick = () => {
        state.view = 'style';
        state.styleId = btn.dataset.selectStyle;
        sidebar.classList.remove('open');
        sidebarBackdrop.classList.remove('open');
        window.scrollTo({ top: 0, behavior: 'smooth' });
        renderMain();
      };
    });
  }

  // Command Palette
  const cmdModal = document.getElementById('cmdModal');
  const cmdSearchInput = document.getElementById('cmdSearchInput');
  const cmdList = document.getElementById('cmdList');

  function openCmdPalette() {
    cmdModal.hidden = false;
    cmdSearchInput.value = '';
    state.cmdActiveIdx = 0;
    renderCmdList('');
    setTimeout(() => cmdSearchInput.focus(), 20);
  }
  function updateCmdHighlight() {
    const items = [...cmdList.querySelectorAll('.cmd-item')];
    if (!items.length) return;
    state.cmdActiveIdx = Math.max(0, Math.min(state.cmdActiveIdx, items.length - 1));
    items.forEach((el, idx) => {
      el.classList.toggle('highlighted', idx === state.cmdActiveIdx);
      if (idx === state.cmdActiveIdx) el.scrollIntoView({ block: 'nearest' });
    });
  }
  function renderCmdList(q) {
    const query = q.trim().toLowerCase();
    const items = D.styles.filter(s =>
      s.name.toLowerCase().includes(query) ||
      s.subtitle.toLowerCase().includes(query) ||
      s.stack.toLowerCase().includes(query)
    );
    cmdList.innerHTML = items.map(s => `
      <button type="button" class="cmd-item" data-cmd-style="${s.id}">
        <span><strong>${s.num}. ${s.name}</strong> <span class="muted-text">— ${s.subtitle}</span></span>
        <span class="ui-badge">切换</span>
      </button>
    `).join('') + `
      <button type="button" class="cmd-item" id="cmdCopyPrompt">
        <span><strong>复制当前风格 AI Agent 提示词</strong> <span class="muted-text">(${getCurrentStyle().name})</span></span>
        <kbd>Prompt</kbd>
      </button>
      <button type="button" class="cmd-item" id="cmdToggleTheme">
        <span><strong>切换浅色 / 深色模式</strong></span>
        <kbd>Theme</kbd>
      </button>
      <button type="button" class="cmd-item" id="cmdOpenMatrix">
        <span><strong>打开 10 大风格对比矩阵</strong></span>
        <kbd>Matrix</kbd>
      </button>
    `;
    state.cmdActiveIdx = 0;
    updateCmdHighlight();

    cmdList.querySelectorAll('[data-cmd-style]').forEach(b => {
      b.onclick = () => {
        state.view = 'style';
        state.styleId = b.dataset.cmdStyle;
        cmdModal.hidden = true;
        renderMain();
      };
    });
    document.getElementById('cmdCopyPrompt').onclick = () => {
      const s = getCurrentStyle();
      cmdModal.hidden = true;
      copyText(C.buildAgentPrompt(s, state), `已复制 ${s.name} 提示词`, '可直接粘贴到 AI Agent。');
    };
    document.getElementById('cmdToggleTheme').onclick = () => {
      state.theme = state.theme === 'light' ? 'dark' : 'light';
      cmdModal.hidden = true;
      applyThemeAndTokens();
    };
    document.getElementById('cmdOpenMatrix').onclick = () => {
      state.view = 'matrix';
      cmdModal.hidden = true;
      renderMain();
    };
  }

  // Global listeners
  document.getElementById('openCmdBtn').onclick = openCmdPalette;
  cmdSearchInput.oninput = () => renderCmdList(cmdSearchInput.value);
  cmdSearchInput.onkeydown = e => {
    const items = [...cmdList.querySelectorAll('.cmd-item')];
    if (!items.length) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      state.cmdActiveIdx = (state.cmdActiveIdx + 1) % items.length;
      updateCmdHighlight();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      state.cmdActiveIdx = (state.cmdActiveIdx - 1 + items.length) % items.length;
      updateCmdHighlight();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      items[state.cmdActiveIdx]?.click();
    }
  };

  document.getElementById('toggleThemeBtn').onclick = () => {
    state.theme = state.theme === 'light' ? 'dark' : 'light';
    applyThemeAndTokens();
  };
  document.getElementById('toggleTokensBtn').onclick = () => {
    state.showTokens = !state.showTokens;
    renderTokenBar();
  };
  openMatrixBtn.onclick = () => {
    state.view = 'matrix';
    sidebar.classList.remove('open');
    sidebarBackdrop.classList.remove('open');
    renderMain();
  };

  // Quick jump pill highlight
  quickJumpPills.querySelectorAll('.jump-pill').forEach(pill => {
    pill.onclick = () => {
      quickJumpPills.querySelectorAll('.jump-pill').forEach(p => p.classList.remove('active'));
      pill.classList.add('active');
    };
  });

  // Custom Context Menu Actions
  if (customContextMenu) {
    customContextMenu.querySelectorAll('[data-ctx-action]').forEach(btn => {
      btn.onclick = () => {
        const act = btn.dataset.ctxAction;
        customContextMenu.hidden = true;
        if (act === 'inspect') {
          state.showTokens = true;
          renderTokenBar();
          showToast('Token 面板已展开', '可在顶部栏查看并复制实时变量。', 'info');
        } else if (act === 'copy-prompt') {
          const s = getCurrentStyle();
          copyText(C.buildAgentPrompt(s, state), `已复制 ${s.name} 提示词`, '已包含重构避坑规范。');
        } else if (act === 'toggle-theme') {
          state.theme = state.theme === 'light' ? 'dark' : 'light';
          applyThemeAndTokens();
        } else if (act === 'reset') {
          state.progress = 72;
          state.sliderVal = 64;
          state.rangeMin = 120;
          state.rangeMax = 680;
          state.rating = 4;
          state.wizardStep = 1;
          state.tableMode = 'normal';
          state.tableRows = D.tableRows.map(r => ({ ...r }));
          state.tableSelectedIds.clear();
          state.pendingDeleteId = null;
          state.stepperVal = 4;
          renderMain();
          showToast('已重置控件状态', '所有演示控件与表格数据已恢复初始值。', 'warn');
        }
      };
    });
  }

  // Mobile sidebar
  document.getElementById('openSidebarBtn').onclick = () => {
    sidebar.classList.add('open');
    sidebarBackdrop.classList.add('open');
  };
  const closeSb = () => {
    sidebar.classList.remove('open');
    sidebarBackdrop.classList.remove('open');
  };
  document.getElementById('closeSidebarBtn').onclick = closeSb;
  sidebarBackdrop.onclick = closeSb;

  // Modals close & actions
  document.querySelectorAll('[data-close-modal]').forEach(b => {
    b.onclick = () => {
      if (b.dataset.closeModal === 'alertDialogModal') state.pendingDeleteId = null;
      closeModal(b.dataset.closeModal);
    };
  });
  document.querySelectorAll('.modal-backdrop').forEach(bd => {
    bd.onclick = e => { if (e.target === bd && bd.id !== 'alertDialogModal') bd.hidden = true; };
  });
  document.getElementById('confirmDemoModalBtn').onclick = () => {
    const name = document.getElementById('modalProjectName').value || 'new-project';
    const visEl = document.getElementById('modalVisibilitySelect');
    const vis = visEl ? visEl.dataset.value : 'private';
    closeModal('demoModal');
    showToast('工作区已创建', `项目 ${name} (${vis}) 初始化成功`, 'success');
  };
  const confirmAlertBtn = document.getElementById('confirmAlertDialogBtn');
  if (confirmAlertBtn) {
    confirmAlertBtn.onclick = () => {
      if (state.pendingDeleteId) {
        const target = state.tableRows.find(r => r.id === state.pendingDeleteId);
        state.tableRows = state.tableRows.filter(r => r.id !== state.pendingDeleteId);
        state.tableSelectedIds.delete(state.pendingDeleteId);
        state.pendingDeleteId = null;
        closeModal('alertDialogModal');
        renderTableRows();
        showToast('记录已永久删除', target ? `${target.name} (${target.id})` : '', 'danger');
      } else {
        closeModal('alertDialogModal');
        showToast('高危操作已确认', '演示模式已模拟执行不可逆删除流程。', 'danger');
      }
    };
  }
  document.getElementById('saveDrawerBtn').onclick = () => {
    closeModal('demoDrawer');
    showToast('配置已保存', 'Figma Tokens 与无障碍设置已更新。', 'success');
  };

  // Global floating menu close & modal select init
  bindCustomSelectsAndDropdowns(document.getElementById('demoModal'));
  document.addEventListener('click', () => closeAllFloatingMenus());
  window.addEventListener('scroll', () => { if (customContextMenu && !customContextMenu.hidden) customContextMenu.hidden = true; }, { passive: true });

  // Keyboard shortcuts (⌘K / Ctrl+K & ESC)
  document.addEventListener('keydown', e => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      if (cmdModal.hidden) openCmdPalette();
      else cmdModal.hidden = true;
    } else if (e.key === 'Escape') {
      closeAllFloatingMenus();
      cmdModal.hidden = true;
      closeModal('demoModal');
      closeModal('alertDialogModal');
      closeModal('demoDrawer');
    }
  });

  // Initial render
  renderMain();
})();
