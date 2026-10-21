// 把各块的绘制串起来，并管布局状态（抽屉、单栏时看列表还是看文章）。
// ctx = { root, state, ui }，ui 是不落盘的界面瞬时状态。

const { t, el, icon, iconButton, q, toggleClass } = require('./dom.js');
const { renderNav } = require('./nav.js');
const { buildListHeader, paintListHeader, renderList, markSelected } = require('./list.js');
const { buildReader, renderReader, paintReaderTools, paintTypography } = require('./reader.js');
const { buildManage, renderManage } = require('./manage.js');

const WIDE = '(min-width: 1180px)';
const SPLIT = '(min-width: 860px)';

function matches(query) {
  try {
    return typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia(query).matches;
  } catch {
    return false;
  }
}

/** wide：三栏；split：列表 + 正文，侧栏是抽屉；single：一次只看一栏。 */
function layout() {
  if (matches(WIDE)) return 'wide';
  if (matches(SPLIT)) return 'split';
  return 'single';
}

function buildShell(ctx) {
  const { root, state } = ctx;
  root.replaceChildren();
  const nav = el('nav', { className: 'rss-nav', 'data-role': 'nav', 'aria-label': t('feeds'), id: 'rss-nav' });
  const navHead = el('header', { className: 'rss-pane-head rss-nav-head' });
  const brand = el('div', { className: 'rss-pane-head__row' });
  const titles = el('div', { className: 'rss-pane-head__titles' });
  titles.append(el('h1', { className: 'rss-app-title' }, t('title')), el('p', { className: 'rss-pane-sub', 'data-role': 'nav-stats' }));
  brand.append(titles, iconButton('toggle-nav', 'menu', t('hideFeeds'), { className: 'rss-icon-btn rss-nav-close', 'data-role': 'nav-close' }));
  navHead.appendChild(brand);
  const navFoot = el('footer', { className: 'rss-nav-foot' });
  if (state.canManage) {
    const manage = el('button', { type: 'button', className: 'rss-btn is-quiet', 'data-action': 'open-manage', 'data-tab': 'sources' });
    manage.append(icon('settings'), el('span', null, t('manage')));
    navFoot.appendChild(manage);
  } else {
    const exportBtn = el('button', { type: 'button', className: 'rss-btn is-quiet', 'data-action': 'export-opml' });
    exportBtn.append(icon('download'), el('span', null, t('exportOpml')));
    navFoot.appendChild(exportBtn);
  }
  navFoot.appendChild(iconButton('show-help', 'keyboard', t('shortcuts'), { 'aria-keyshortcuts': 'Shift+?' }));
  nav.append(navHead, el('div', { className: 'rss-nav__body', 'data-role': 'nav-body' }), navFoot);

  const list = el('section', { className: 'rss-list', 'data-role': 'list', 'aria-labelledby': 'rss-list-title' });
  list.append(buildListHeader(state), el('div', { className: 'rss-list__body', 'data-role': 'list-body' }));

  const shell = el('div', { className: 'rss-shell' });
  shell.append(nav, list, buildReader(state));
  root.append(
    shell,
    el('button', { type: 'button', className: 'rss-scrim rss-drawer-scrim', 'data-action': 'close-drawer', tabindex: '-1', 'aria-hidden': 'true' }),
    state.canManage ? buildManage() : el('div', { hidden: true }),
    buildHelp(),
    el('div', { className: 'rss-toast', 'data-role': 'toast', role: 'status', 'aria-live': 'polite', hidden: true }),
  );
  if (document.documentElement) {
    try {
      document.documentElement.lang = Tapp.i18n.getLocale() || '';
    } catch {
      // 拿不到语言就不改。
    }
  }
}

function buildHelp() {
  const layer = el('div', { className: 'rss-layer', 'data-role': 'help-layer', hidden: true });
  layer.appendChild(el('button', { type: 'button', className: 'rss-scrim', 'data-action': 'close-help', tabindex: '-1', 'aria-hidden': 'true' }));
  const dialog = el('section', { className: 'rss-dialog is-small', role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': 'rss-help-title', 'data-role': 'help' });
  const head = el('header', { className: 'rss-dialog__head' });
  head.append(el('h2', { id: 'rss-help-title' }, t('shortcuts')), iconButton('close-help', 'close', t('close')));
  const list = el('dl', { className: 'rss-shortcuts' });
  const rows = [
    [['J'], 'keyNext'],
    [['K'], 'keyPrev'],
    [['O'], 'keyOriginal'],
    [['S'], 'keyStar'],
    [['M'], 'keyRead'],
    [['R'], 'keyReload'],
    [['/'], 'keySearch'],
    [['Shift', 'A'], 'keyMarkAll'],
    [['['], 'keyFeeds'],
    [['Esc'], 'keyBack'],
  ];
  for (const [keys, label] of rows) {
    const term = el('dt');
    for (const key of keys) term.appendChild(el('kbd', null, key));
    list.append(term, el('dd', null, t(label)));
  }
  dialog.append(head, list);
  layer.appendChild(dialog);
  return layer;
}

function paintLayout(ctx) {
  const { root, state, ui } = ctx;
  const mode = layout();
  ui.layout = mode;
  root.setAttribute('data-layout', mode);
  toggleClass(root, 'is-article', state.view === 'article');
  toggleClass(root, 'is-drawer', state.drawer && mode !== 'wide');
  toggleClass(root, 'is-nav-collapsed', state.navCollapsed && mode === 'wide');
  const navOpen = mode === 'wide' ? !state.navCollapsed : state.drawer;
  const toggle = q(root, 'nav-toggle');
  if (toggle) {
    toggle.setAttribute('aria-expanded', navOpen ? 'true' : 'false');
    toggle.setAttribute('aria-controls', 'rss-nav');
    toggle.hidden = mode === 'wide' && !state.navCollapsed;
  }
  const close = q(root, 'nav-close');
  if (close) {
    close.setAttribute('aria-label', mode === 'wide' ? t('hideFeeds') : t('close'));
    close.setAttribute('title', `${mode === 'wide' ? t('hideFeeds') : t('close')} ([)`);
  }
  const back = q(root, 'reader-back');
  if (back) back.hidden = mode !== 'single';
  const help = q(root, 'help-layer');
  if (help) help.hidden = !ui.helpOpen;
}

function paintAll(ctx) {
  const { root, state, ui } = ctx;
  paintLayout(ctx);
  renderNav(root, state);
  paintListHeader(root, state, ui);
  renderList(root, state);
  renderReader(root, state);
  paintReaderTools(root, state);
  paintTypography(root, state, ui);
  if (state.canManage) renderManage(root, state, ui.manage);
  watchSentinel(ctx);
}

/** 列表变了但文章没变：别重画正文，否则滚动位置会丢。 */
function paintListSide(ctx) {
  const { root, state, ui } = ctx;
  renderNav(root, state);
  paintListHeader(root, state, ui);
  renderList(root, state);
  paintReaderTools(root, state);
  watchSentinel(ctx);
}

let observer = null;

/**
 * 列表末尾露出来就接着翻页。每次重画列表都要重新挂，旧节点已经不在树上了。
 * 搜索时不自动翻：只在已加载的里找，要更多由人点「加载更多」。
 */
function watchSentinel(ctx) {
  if (observer) observer.disconnect();
  observer = null;
  if (typeof IntersectionObserver !== 'function' || ctx.state.query.trim()) return;
  const sentinel = q(ctx.root, 'sentinel');
  if (!sentinel || typeof ctx.loadMore !== 'function') return;
  observer = new IntersectionObserver((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) ctx.loadMore();
  }, { root: q(ctx.root, 'list-body'), rootMargin: '0px 0px 480px 0px' });
  observer.observe(sentinel);
}

function paintSelection(ctx) {
  markSelected(ctx.root, ctx.state);
  paintReaderTools(ctx.root, ctx.state);
}

let toastTimer = null;

function toast(ctx, message, options) {
  const node = q(ctx.root, 'toast');
  if (!node) return;
  const opts = options || {};
  if (toastTimer) clearTimeout(toastTimer);
  node.replaceChildren(el('span', null, message));
  node.setAttribute('role', opts.error ? 'alert' : 'status');
  toggleClass(node, 'is-error', Boolean(opts.error));
  if (opts.action) {
    node.appendChild(el('button', { type: 'button', className: 'rss-toast__action', 'data-action': opts.action.name }, opts.action.label));
  }
  node.hidden = false;
  toastTimer = setTimeout(() => {
    node.hidden = true;
  }, opts.error ? 7000 : 3200);
}

function errorText(error) {
  const message = error && (error.message || error.error || error.code);
  return String(message || t('unknownError'));
}

module.exports = { layout, buildShell, paintLayout, paintAll, paintListSide, paintSelection, toast, errorText };
