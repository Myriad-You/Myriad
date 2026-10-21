// 读的那一侧：载入、翻页、打开文章、标记。写操作先改界面再等宿主，失败就退回。

const api = require('./api.js');
const { t, q, nextFrame } = require('./dom.js');
const { isFeed, categoryTokens } = require('./format.js');
const model = require('./model.js');
const { renderNav } = require('./nav.js');
const { paintListHeader, updateRow, revealSelected, sourceLookup } = require('./list.js');
const { renderReader, paintReaderTools } = require('./reader.js');
const view = require('./view.js');

const PAGE_SIZE = 30;
let listToken = 0;
let articleToken = 0;
let prefsTimer = null;

function granted(name) {
  const list = Array.isArray(Tapp.permissions) ? Tapp.permissions : [];
  if (list.includes(name)) return true;
  try {
    const info = Tapp.lifecycle.getInfo();
    return Array.isArray(info.permissions) && info.permissions.includes(name);
  } catch {
    return false;
  }
}

async function readPrefs() {
  if (!granted('storage:read') || !Tapp.storage || typeof Tapp.storage.get !== 'function') return {};
  try {
    const value = await Tapp.storage.get(model.PREFS_KEY);
    return value && typeof value === 'object' ? value : {};
  } catch {
    return {};
  }
}

/** 偏好晚一点合并写，连点几下字号只落一次盘。没有存储权限就只在本次有效。 */
function savePrefs(ctx) {
  if (!granted('storage:write') || !Tapp.storage || typeof Tapp.storage.set !== 'function') return;
  if (prefsTimer) clearTimeout(prefsTimer);
  prefsTimer = setTimeout(() => {
    void Promise.resolve(Tapp.storage.set(model.PREFS_KEY, model.prefsOf(ctx.state))).catch(() => {});
  }, 300);
}

async function boot(ctx) {
  ctx.loadMore = () => void loadItems(ctx, true);
  model.restorePrefs(ctx.state, await readPrefs());
  view.paintAll(ctx);
  await reloadAll(ctx);
}

async function loadMeta(ctx) {
  const [sources, categories, stats] = await Promise.all([
    api.sources(),
    api.categories().catch(() => []),
    api.stats().catch(() => null),
  ]);
  const { state } = ctx;
  state.sources = Array.isArray(sources) ? sources : [];
  state.categories = Array.isArray(categories) ? categories : [];
  state.stats = stats;
  if (!model.scopeStillExists(state, isFeed, categoryTokens)) state.scope = { kind: 'all', value: null };
}

async function reloadAll(ctx) {
  const { state } = ctx;
  const keep = state.booted;
  state.listLoading = true;
  state.listError = '';
  view.paintListSide(ctx);
  try {
    await loadMeta(ctx);
  } catch (error) {
    state.booted = true;
    state.listLoading = false;
    state.listError = view.errorText(error);
    view.paintListSide(ctx);
    return;
  }
  state.booted = true;
  await loadItems(ctx, false, { keep });
}

async function refreshCounts(ctx) {
  try {
    await loadMeta(ctx);
  } catch {
    return;
  }
  renderNav(ctx.root, ctx.state);
  paintListHeader(ctx.root, ctx.state, ctx.ui);
}

/** keep：刷新时先留着旧列表，新的一页回来再换，免得闪一下骨架。 */
async function loadItems(ctx, append, options) {
  const { state } = ctx;
  const keep = Boolean(options && options.keep);
  if (append && (!state.nextCursor || state.listLoading)) return;
  const token = ++listToken;
  state.listLoading = true;
  state.listError = '';
  if (!append && !keep) {
    state.items = [];
    state.nextCursor = null;
  }
  view.paintListSide(ctx);
  try {
    const page = await api.list(model.listQuery(state, append ? state.nextCursor : null, PAGE_SIZE));
    if (token !== listToken) return;
    const incoming = page && Array.isArray(page.items) ? page.items : [];
    const known = new Set(append ? state.items.map((item) => item.id) : []);
    state.items = (append ? state.items : []).concat(incoming.filter((item) => !known.has(item.id)));
    state.nextCursor = page && page.next_cursor ? page.next_cursor : null;
  } catch (error) {
    if (token !== listToken) return;
    if (append) view.toast(ctx, t('failed', { message: view.errorText(error) }), { error: true });
    else state.listError = view.errorText(error);
  }
  state.listLoading = false;
  view.paintListSide(ctx);
}

function setScope(ctx, scope) {
  const { state } = ctx;
  const changed = !model.sameScope(state.scope, scope);
  state.scope = scope;
  state.drawer = false;
  state.view = 'list';
  ctx.ui.confirmMarkAll = false;
  if (changed) {
    const body = q(ctx.root, 'list-body');
    if (body) body.scrollTop = 0;
  }
  savePrefs(ctx);
  view.paintLayout(ctx);
  void loadItems(ctx, false);
}

function setUnreadOnly(ctx, value) {
  if (ctx.state.unreadOnly === value) return;
  ctx.state.unreadOnly = value;
  ctx.ui.confirmMarkAll = false;
  savePrefs(ctx);
  void loadItems(ctx, false);
}

function adjustUnread(state, sourceId, delta) {
  const source = state.sources.find((item) => item.id === sourceId);
  if (source) source.unread_count = Math.max(0, (source.unread_count || 0) + delta);
  if (state.stats && typeof state.stats.total_unread === 'number') {
    state.stats.total_unread = Math.max(0, state.stats.total_unread + delta);
  }
}

function adjustStarred(state, delta) {
  if (state.stats && typeof state.stats.total_starred === 'number') {
    state.stats.total_starred = Math.max(0, state.stats.total_starred + delta);
  }
}

function afterItemChange(ctx, id) {
  updateRow(ctx.root, ctx.state, id);
  renderNav(ctx.root, ctx.state);
  paintListHeader(ctx.root, ctx.state, ctx.ui);
  paintReaderTools(ctx.root, ctx.state);
}

async function openArticle(ctx, id, options) {
  const { state, root } = ctx;
  const opts = options || {};
  if (id == null) return;
  const token = ++articleToken;
  const same = state.selectedId === id && state.article && state.article.id === id;
  state.selectedId = id;
  state.view = 'article';
  state.drawer = false;
  state.articleError = '';
  ctx.ui.typeOpen = false;
  view.paintLayout(ctx);
  view.paintSelection(ctx);
  if (opts.reveal) revealSelected(root, state);
  if (same) {
    focusArticle(ctx, opts);
    return;
  }
  state.article = null;
  renderReader(root, state);
  try {
    const article = await api.get(id);
    if (token !== articleToken) return;
    state.article = article;
  } catch (error) {
    if (token !== articleToken) return;
    state.articleError = view.errorText(error);
    renderReader(root, state);
    return;
  }
  renderReader(root, state);
  paintReaderTools(root, state);
  focusArticle(ctx, opts);
  if (state.canWrite && !state.article.is_read) await setRead(ctx, true, { quiet: true });
  prefetchIfNearEnd(ctx);
}

/** 单栏时把焦点交给标题，读屏从文章开头读；多栏时焦点留在原处，j/k 接着用。 */
function focusArticle(ctx, opts) {
  if (ctx.ui.layout !== 'single' && !opts.focus) return;
  nextFrame(() => {
    const title = q(ctx.root, 'article-title');
    if (title && typeof title.focus === 'function') title.focus({ preventScroll: true });
  });
}

function prefetchIfNearEnd(ctx) {
  const { state } = ctx;
  if (!state.nextCursor) return;
  const items = model.shownItems(state, sourceLookup(state));
  const index = items.findIndex((item) => item.id === state.selectedId);
  if (index >= 0 && items.length - index <= 3) void loadItems(ctx, true);
}

async function step(ctx, dir) {
  const { state } = ctx;
  let items = model.shownItems(state, sourceLookup(state));
  let id = model.neighbour(items, state.selectedId, dir);
  if (id == null && dir > 0 && state.nextCursor) {
    await loadItems(ctx, true);
    items = model.shownItems(state, sourceLookup(state));
    id = model.neighbour(items, state.selectedId, dir);
  }
  if (id == null) {
    view.toast(ctx, dir > 0 ? t('noNext') : t('noPrevious'));
    return;
  }
  await openArticle(ctx, id, { reveal: true });
}

function listedItem(state, id) {
  return state.items.find((item) => item.id === id) || null;
}

function targets(state) {
  const id = state.selectedId;
  const article = state.article && state.article.id === id ? state.article : null;
  return { id, article, listed: listedItem(state, id) };
}

async function setRead(ctx, read, options) {
  const { state } = ctx;
  const { id, article, listed } = targets(state);
  if (id == null) return;
  if (!state.canWrite) {
    view.toast(ctx, t('noWrite'));
    return;
  }
  const record = article || listed;
  if (!record || Boolean(record.is_read) === read) return;
  const apply = (value) => {
    if (article) article.is_read = value;
    if (listed) listed.is_read = value;
  };
  apply(read);
  adjustUnread(state, record.source_id, read ? -1 : 1);
  afterItemChange(ctx, id);
  try {
    if (read) await api.markRead(id);
    else await api.markUnread(id);
    if (!(options && options.quiet)) view.toast(ctx, read ? t('markedRead') : t('markedUnread'));
  } catch (error) {
    apply(!read);
    adjustUnread(state, record.source_id, read ? 1 : -1);
    afterItemChange(ctx, id);
    view.toast(ctx, t('failed', { message: view.errorText(error) }), { error: true });
  }
}

async function setStar(ctx, starred) {
  const { state } = ctx;
  const { id, article, listed } = targets(state);
  if (id == null) return;
  if (!state.canWrite) {
    view.toast(ctx, t('noWrite'));
    return;
  }
  const record = article || listed;
  if (!record || Boolean(record.is_starred) === starred) return;
  const apply = (value) => {
    if (article) article.is_starred = value;
    if (listed) listed.is_starred = value;
  };
  apply(starred);
  adjustStarred(state, starred ? 1 : -1);
  afterItemChange(ctx, id);
  try {
    if (starred) await api.star(id);
    else await api.unstar(id);
    view.toast(ctx, starred ? t('starred') : t('unstarred'));
  } catch (error) {
    apply(!starred);
    adjustStarred(state, starred ? -1 : 1);
    afterItemChange(ctx, id);
    view.toast(ctx, t('failed', { message: view.errorText(error) }), { error: true });
  }
}

function toggleRead(ctx) {
  const { article, listed } = targets(ctx.state);
  const record = article || listed;
  if (record) void setRead(ctx, !record.is_read);
}

function toggleStar(ctx) {
  const { article, listed } = targets(ctx.state);
  const record = article || listed;
  if (record) void setStar(ctx, !record.is_starred);
}

async function openOriginal(ctx) {
  const { id, article, listed } = targets(ctx.state);
  const record = article || listed;
  if (id == null || !record || !record.link) return;
  try {
    await api.openOriginal(id);
  } catch (error) {
    view.toast(ctx, t('openFailed', { message: view.errorText(error) }), { error: true });
  }
}

async function markAll(ctx) {
  const { state } = ctx;
  ctx.ui.confirmMarkAll = false;
  paintListHeader(ctx.root, state, ctx.ui);
  if (!model.canMarkAll(state)) return;
  try {
    await api.markAllRead(model.markAllQuery(state));
  } catch (error) {
    view.toast(ctx, t('failed', { message: view.errorText(error) }), { error: true });
    return;
  }
  for (const item of state.items) item.is_read = true;
  if (state.article) state.article.is_read = true;
  view.paintListSide(ctx);
  view.toast(ctx, t('markedAllRead'));
  await refreshCounts(ctx);
}

async function exportOpml(ctx) {
  try {
    const opml = await api.exportOpml();
    await Tapp.file.download(String(opml || ''), 'subscriptions.opml', 'text/x-opml;charset=utf-8');
  } catch (error) {
    view.toast(ctx, t('failed', { message: view.errorText(error) }), { error: true });
  }
}

module.exports = {
  granted,
  boot,
  savePrefs,
  reloadAll,
  refreshCounts,
  loadItems,
  setScope,
  setUnreadOnly,
  openArticle,
  step,
  toggleRead,
  toggleStar,
  openOriginal,
  markAll,
  exportOpml,
};
