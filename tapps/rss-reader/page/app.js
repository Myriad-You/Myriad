// 入口：建外壳、接事件。读的逻辑在 reading.js，管理在 admin.js，绘制在 view.js。

const { el, q, nextFrame } = require('./dom.js');
const model = require('./model.js');
const { paintListHeader } = require('./list.js');
const { paintTypography, trackProgress, followScroll, currentItem } = require('./reader.js');
const { createManageUi } = require('./manage.js');
const { markBrokenIcon } = require('./nav.js');
const reading = require('./reading.js');
const admin = require('./admin.js');
const view = require('./view.js');

function start(root) {
  const state = model.createState({
    write: reading.granted('phantasi:write'),
    manage: reading.granted('phantasi:manage'),
  });
  const ui = {
    layout: 'wide',
    searchOpen: false,
    confirmMarkAll: false,
    typeOpen: false,
    helpOpen: false,
    manage: createManageUi(),
  };
  const ctx = { root, state, ui };

  view.buildShell(ctx);
  root.addEventListener('click', (event) => onClick(ctx, event));
  root.addEventListener('input', (event) => onInput(ctx, event));
  root.addEventListener('change', (event) => onChange(ctx, event));
  root.addEventListener('scroll', (event) => onScroll(ctx, event), true);
  root.addEventListener('error', onImageError, true);
  if (typeof document.addEventListener === 'function') {
    document.addEventListener('keydown', (event) => onKey(ctx, event));
  }
  if (typeof window !== 'undefined') {
    applyInsets(root, window._TAPP_INITIAL_SAFE_INSETS);
    if (typeof window.addEventListener === 'function') {
      window.addEventListener('resize', () => view.paintLayout(ctx));
      window.addEventListener('tapp:resize', (event) => {
        const detail = (event && event.detail) || {};
        applyInsets(root, {
          top: detail.safeInsetTop,
          right: detail.safeInsetRight,
          bottom: detail.safeInsetBottom,
          left: detail.safeInsetLeft,
        });
      });
    }
  }
  if (Tapp.ui && typeof Tapp.ui.onLocaleChange === 'function') {
    Tapp.ui.onLocaleChange(() => {
      view.buildShell(ctx);
      view.paintAll(ctx);
      void Tapp.ui.setTitle(Tapp.i18n.t('title')).catch(() => {});
    });
  }
  void Promise.resolve(Tapp.ui.setTitle(Tapp.i18n.t('title'))).catch(() => {});
  void reading.boot(ctx);
  return ctx;
}

/**
 * 宿主全屏时会给内容区加内边距，把悬浮的标题条让出来。page.css 把那层内边距清掉，
 * 改由这里把同样的数值交给各栏的栏头和边缘，背景就能一直铺到屏幕边上。
 */
function applyInsets(root, insets) {
  if (!root.style || typeof root.style.setProperty !== 'function') return;
  const value = insets || {};
  for (const side of ['top', 'right', 'bottom', 'left']) {
    const px = Math.max(0, Number(value[side]) || 0);
    root.style.setProperty(`--rss-inset-${side}`, `${px}px`);
  }
}

function numberAttr(node, name) {
  const value = node.getAttribute(name);
  return value == null || value === '' ? null : Number(value);
}

function onClick(ctx, event) {
  const target = event.target && event.target.closest ? event.target.closest('[data-action]') : null;
  if (!target || !ctx.root.contains(target) || target.disabled) {
    closePopovers(ctx, event.target);
    return;
  }
  const action = target.getAttribute('data-action');
  if (action !== 'toggle-type' && action !== 'font-step' && action !== 'font-family' && action !== 'measure') closePopovers(ctx, target);
  run(ctx, action, target);
}

function run(ctx, action, target) {
  const { state, ui } = ctx;
  const id = numberAttr(target, 'data-id');
  switch (action) {
    case 'scope': {
      const kind = target.getAttribute('data-kind');
      const raw = target.getAttribute('data-value');
      const value = kind === 'source' ? Number(raw) : kind === 'category' ? raw : null;
      reading.setScope(ctx, { kind, value });
      return;
    }
    case 'toggle-folder': {
      const name = target.getAttribute('data-value');
      state.collapsed = state.collapsed.includes(name)
        ? state.collapsed.filter((item) => item !== name)
        : state.collapsed.concat(name);
      reading.savePrefs(ctx);
      view.paintListSide(ctx);
      return;
    }
    case 'toggle-nav':
      toggleNav(ctx);
      return;
    case 'close-drawer':
      state.drawer = false;
      view.paintLayout(ctx);
      return;
    case 'unread-only':
      reading.setUnreadOnly(ctx, target.getAttribute('data-value') === 'true');
      return;
    case 'toggle-search':
      setSearch(ctx, !ui.searchOpen);
      return;
    case 'clear-search':
      clearSearch(ctx);
      return;
    case 'reload':
      void reading.reloadAll(ctx);
      return;
    case 'ask-mark-all':
      ui.confirmMarkAll = !ui.confirmMarkAll;
      paintListHeader(ctx.root, state, ui);
      if (ui.confirmMarkAll) focusLater(ctx, '[data-action="mark-all"]');
      return;
    case 'cancel-mark-all':
      ui.confirmMarkAll = false;
      paintListHeader(ctx.root, state, ui);
      focusLater(ctx, '[data-action="ask-mark-all"]');
      return;
    case 'mark-all':
      void reading.markAll(ctx);
      return;
    case 'open':
      void reading.openArticle(ctx, id, { focus: ui.layout === 'single' });
      return;
    case 'more':
      void reading.loadItems(ctx, true);
      return;
    case 'back':
      goBack(ctx);
      return;
    case 'prev':
      void reading.step(ctx, -1);
      return;
    case 'next':
      void reading.step(ctx, 1);
      return;
    case 'retry-article':
      if (state.selectedId != null) {
        const selected = state.selectedId;
        state.selectedId = null;
        void reading.openArticle(ctx, selected);
      }
      return;
    case 'toggle-read':
      reading.toggleRead(ctx);
      return;
    case 'toggle-star':
      reading.toggleStar(ctx);
      return;
    case 'open-original':
      void reading.openOriginal(ctx);
      return;
    case 'toggle-type':
      ui.typeOpen = !ui.typeOpen;
      paintTypography(ctx.root, state, ui);
      return;
    case 'font-step':
      state.fontStep = model.clampStep(state.fontStep + Number(target.getAttribute('data-value')));
      paintTypography(ctx.root, state, ui);
      reading.savePrefs(ctx);
      nextFrame(() => trackProgress(ctx.root));
      return;
    case 'font-family':
      state.fontFamily = target.getAttribute('data-value') === 'sans' ? 'sans' : 'serif';
      paintTypography(ctx.root, state, ui);
      reading.savePrefs(ctx);
      return;
    case 'measure':
      if (model.MEASURES.includes(target.getAttribute('data-value'))) state.measure = target.getAttribute('data-value');
      paintTypography(ctx.root, state, ui);
      reading.savePrefs(ctx);
      return;
    case 'toggle-focus':
      toggleFocus(ctx);
      return;
    case 'toggle-compact':
      state.compact = !state.compact;
      view.paintLayout(ctx);
      paintListHeader(ctx.root, state, ui);
      reading.savePrefs(ctx);
      return;
    case 'export-opml':
      void reading.exportOpml(ctx);
      return;
    case 'show-help':
      setHelp(ctx, true);
      return;
    case 'close-help':
      setHelp(ctx, false);
      return;
    case 'open-manage':
      admin.openManage(ctx, target.getAttribute('data-tab'), { intent: target.getAttribute('data-intent') });
      return;
    case 'edit-scope-source':
      if (state.scope.kind === 'source') admin.openManage(ctx, 'sources', { sourceId: state.scope.value });
      return;
    case 'close-manage':
      admin.closeManage(ctx);
      return;
    case 'manage-tab':
      admin.switchTab(ctx, target.getAttribute('data-tab'));
      return;
    case 'discover':
      void admin.discover(ctx);
      return;
    case 'reset-discover':
      admin.resetDiscover(ctx);
      return;
    case 'add-source':
      void admin.addSource(ctx);
      return;
    case 'expand-source':
      admin.expandSource(ctx, id);
      return;
    case 'save-source':
      void admin.saveSource(ctx, id);
      return;
    case 'refresh-source':
      void admin.refreshSource(ctx, id);
      return;
    case 'delete-source':
      void admin.deleteSource(ctx, id);
      return;
    case 'add-category':
      void admin.addCategory(ctx);
      return;
    case 'rename-category':
      void admin.renameCategory(ctx, id);
      return;
    case 'delete-category':
      void admin.deleteCategory(ctx, id);
      return;
    case 'refresh-all':
      void admin.refreshAll(ctx);
      return;
    default:
  }
}

function closePopovers(ctx, target) {
  if (!ctx.ui.typeOpen) return;
  const panel = q(ctx.root, 'type-panel');
  if (panel && target && panel.contains(target)) return;
  ctx.ui.typeOpen = false;
  paintTypography(ctx.root, ctx.state, ctx.ui);
}

function focusLater(ctx, selector) {
  nextFrame(() => {
    const node = ctx.root.querySelector(selector);
    if (node && typeof node.focus === 'function') node.focus();
  });
}

function toggleNav(ctx) {
  const { state, ui } = ctx;
  if (ui.layout === 'wide') {
    state.navCollapsed = !state.navCollapsed;
    reading.savePrefs(ctx);
  } else {
    state.drawer = !state.drawer;
    if (state.drawer) focusLater(ctx, '.rss-nav [aria-current="page"], .rss-nav .rss-nav-item');
  }
  view.paintLayout(ctx);
}

function toggleFocus(ctx) {
  const { state, ui } = ctx;
  if (ui.layout === 'single') return;
  state.focus = !state.focus;
  if (state.focus) state.drawer = false;
  view.paintLayout(ctx);
  reading.savePrefs(ctx);
}

/**
 * 空格往下翻一屏，已经到底就接着下一篇；Shift+空格往回翻。
 * 还没打开文章时先打开当前选中的那篇。
 */
function pageArticle(ctx, dir) {
  const { state, ui } = ctx;
  const scroll = q(ctx.root, 'reader-scroll');
  const article = currentItem(state);
  if (!article || (ui.layout === 'single' && state.view !== 'article')) {
    if (state.selectedId != null) void reading.openArticle(ctx, state.selectedId, { reveal: true });
    else if (dir > 0) void reading.step(ctx, 1);
    return;
  }
  if (!scroll) return;
  const max = (scroll.scrollHeight || 0) - (scroll.clientHeight || 0);
  const top = scroll.scrollTop || 0;
  if (dir > 0 && top >= max - 4) {
    void reading.step(ctx, 1);
    return;
  }
  const distance = Math.max(120, (scroll.clientHeight || 0) * 0.85) * dir;
  if (typeof scroll.scrollBy === 'function') scroll.scrollBy({ top: distance, behavior: 'smooth' });
  else scroll.scrollTop = top + distance;
}

function goBack(ctx) {
  const { state } = ctx;
  state.view = 'list';
  view.paintLayout(ctx);
  if (state.selectedId != null) focusLater(ctx, `.rss-row[data-id="${state.selectedId}"]`);
}

function setSearch(ctx, open) {
  ctx.ui.searchOpen = open;
  if (!open && ctx.state.query) {
    ctx.state.query = '';
    const input = ctx.root.querySelector('[data-field="search"]');
    if (input) input.value = '';
  }
  view.paintListSide(ctx);
  // 同步聚焦：按 / 之后紧接着打的字不能落到快捷键上。
  const input = open ? ctx.root.querySelector('[data-field="search"]') : null;
  if (input && typeof input.focus === 'function') input.focus();
}

function clearSearch(ctx) {
  ctx.state.query = '';
  const input = ctx.root.querySelector('[data-field="search"]');
  if (input) input.value = '';
  view.paintListSide(ctx);
  focusLater(ctx, '[data-field="search"]');
}

function setHelp(ctx, open) {
  ctx.ui.helpOpen = open;
  view.paintLayout(ctx);
  if (open) focusLater(ctx, '[data-role="help"] [data-action="close-help"]');
}

function onInput(ctx, event) {
  const input = event.target;
  if (!input || input.getAttribute('data-field') !== 'search') return;
  ctx.state.query = input.value || '';
  view.paintListSide(ctx);
}

function onChange(ctx, event) {
  const input = event.target;
  if (input && input.getAttribute('data-action') === 'import-opml' && ctx.state.canManage) {
    admin.importOpml(ctx, input);
  }
}

/** 源图标取不到时换成首字母，不留破图；缩略图取不到就直接去掉。 */
function onImageError(event) {
  const img = event.target;
  if (!img || img.tagName !== 'IMG' || !img.parentNode) return;
  const letter = img.getAttribute('data-fallback');
  if (letter != null) {
    markBrokenIcon(img.getAttribute('src'));
    img.replaceWith(el('span', { className: 'rss-avatar is-mono', 'aria-hidden': 'true' }, letter));
  } else if (img.classList.contains('rss-row__thumb')) {
    img.remove();
  }
}

let progressQueued = false;

function onScroll(ctx, event) {
  const target = event.target;
  if (!target || typeof target.getAttribute !== 'function' || target.getAttribute('data-role') !== 'reader-scroll') return;
  if (progressQueued) return;
  progressQueued = true;
  nextFrame(() => {
    progressQueued = false;
    trackProgress(ctx.root);
    followScroll(ctx.root, ctx.ui.typeOpen);
  });
}

function isTyping(target) {
  if (!target) return false;
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable === true;
}

/** 弹层开着时 Tab 只在弹层里转。 */
function trapFocus(container, event) {
  const nodes = [...container.querySelectorAll('button, input, select, textarea, [tabindex]:not([tabindex="-1"])')]
    .filter((node) => !node.disabled && node.offsetParent !== null);
  if (nodes.length === 0) return;
  const first = nodes[0];
  const last = nodes[nodes.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

function onKey(ctx, event) {
  const { state, ui } = ctx;
  const target = event.target;

  const dialog = admin.isOpen(ctx) ? admin.dialog(ctx) : ui.helpOpen ? q(ctx.root, 'help') : null;
  if (dialog) {
    if (event.key === 'Escape') {
      event.preventDefault();
      if (admin.isOpen(ctx)) admin.closeManage(ctx);
      else setHelp(ctx, false);
      return;
    }
    if (event.key === 'Tab') {
      trapFocus(dialog, event);
      return;
    }
    if (event.key === 'Enter' && target && target.getAttribute && target.getAttribute('data-enter')) {
      event.preventDefault();
      run(ctx, target.getAttribute('data-enter'), enterTarget(target));
      return;
    }
    if (dialog === admin.dialog(ctx) && target && target.getAttribute && target.getAttribute('role') === 'tab'
      && (event.key === 'ArrowRight' || event.key === 'ArrowLeft')) {
      const tabs = [...dialog.querySelectorAll('[role="tab"]')];
      const index = tabs.indexOf(target);
      const next = tabs[(index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length];
      if (next) admin.switchTab(ctx, next.getAttribute('data-tab'));
    }
    return;
  }

  if (isTyping(target)) {
    if (event.key === 'Escape' && target.getAttribute('data-field') === 'search') {
      event.preventDefault();
      setSearch(ctx, false);
      focusLater(ctx, '[data-action="toggle-search"]');
    } else if (event.key === 'Enter' && target.getAttribute('data-field') === 'search') {
      event.preventDefault();
      const first = ctx.root.querySelector('.rss-row');
      if (first) void reading.openArticle(ctx, Number(first.getAttribute('data-id')));
    }
    return;
  }
  if (event.metaKey || event.ctrlKey || event.altKey) return;

  const key = event.key;
  if (key === 'Escape') {
    if (ui.typeOpen) {
      ui.typeOpen = false;
      paintTypography(ctx.root, state, ui);
    } else if (ui.confirmMarkAll) {
      ui.confirmMarkAll = false;
      paintListHeader(ctx.root, state, ui);
    } else if (state.drawer) {
      state.drawer = false;
      view.paintLayout(ctx);
    } else if (state.focus && ui.layout !== 'single') {
      toggleFocus(ctx);
    } else if (state.view === 'article' && ui.layout === 'single') {
      goBack(ctx);
    }
    return;
  }
  if (key === '?') {
    event.preventDefault();
    setHelp(ctx, true);
    return;
  }
  if (key === '/') {
    event.preventDefault();
    if (ui.layout === 'single' && state.view === 'article') goBack(ctx);
    setSearch(ctx, true);
    return;
  }
  if (key === '[') {
    toggleNav(ctx);
    return;
  }
  if (key === 'A' && event.shiftKey) {
    if (!model.canMarkAll(state)) return;
    ui.confirmMarkAll = true;
    if (ui.layout === 'single') goBack(ctx);
    paintListHeader(ctx.root, state, ui);
    focusLater(ctx, '[data-action="mark-all"]');
    return;
  }
  if (key === ' ') {
    // 焦点在别的按钮上时空格是「按下按钮」，不抢；在文章行或空白处才翻页。
    const onButton = target && target.tagName === 'BUTTON' && !(target.classList && target.classList.contains('rss-row'));
    if (onButton) return;
    event.preventDefault();
    pageArticle(ctx, event.shiftKey ? -1 : 1);
    return;
  }
  const lower = key.length === 1 ? key.toLowerCase() : key;
  if (lower === 'j' || lower === 'n') {
    event.preventDefault();
    void reading.step(ctx, 1);
  } else if (lower === 'k' || lower === 'p') {
    event.preventDefault();
    void reading.step(ctx, -1);
  } else if (lower === 'o' || lower === 'v') {
    void reading.openOriginal(ctx);
  } else if (lower === 's') {
    reading.toggleStar(ctx);
  } else if (lower === 'm') {
    reading.toggleRead(ctx);
  } else if (lower === 'r') {
    void reading.reloadAll(ctx);
  } else if (lower === 'f') {
    toggleFocus(ctx);
  }
}

/** 输入框里按回车，按它声明的 data-enter 当作点了对应按钮；id 取自己的或所在源编辑区的。 */
function enterTarget(input) {
  const scope = input.closest ? input.closest('[id^="rss-source-"]') : null;
  const id = input.getAttribute('data-id') || (scope ? scope.getAttribute('id').slice('rss-source-'.length) : null);
  return { getAttribute: (name) => (name === 'data-id' ? id : null) };
}

module.exports = { start };
