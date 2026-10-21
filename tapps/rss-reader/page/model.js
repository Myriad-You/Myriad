// 阅读器状态。这里只放纯函数，DOM 和宿主调用不进来，方便单测。
//
// 「看哪里」和「看哪些」分开：scope 是侧栏选中的范围（全部、收藏、某个分类、
// 某个源），unreadOnly 是列表头的「未读 / 全部」。收藏本身就是一种范围，
// 宿主的 filter 只有一个槽位，所以收藏里不再叠加未读筛选。

const PREFS_KEY = 'rss.reader.prefs';
const PREFS_VERSION = 3;
const FONT_STEPS = [-2, -1, 0, 1, 2, 3];
const UNCATEGORIZED = '__none__';

function createState(permissions) {
  return {
    sources: [],
    categories: [],
    stats: null,
    scope: { kind: 'all', value: null },
    unreadOnly: true,
    query: '',
    items: [],
    nextCursor: null,
    listLoading: false,
    listError: '',
    booted: false,
    selectedId: null,
    article: null,
    articleLoading: false,
    articleError: '',
    view: 'list',
    drawer: false,
    navCollapsed: false,
    collapsed: [],
    fontStep: 0,
    fontFamily: 'serif',
    canWrite: Boolean(permissions && permissions.write),
    canManage: Boolean(permissions && permissions.manage),
  };
}

function sameScope(a, b) {
  return Boolean(a && b) && a.kind === b.kind && (a.value ?? null) === (b.value ?? null);
}

/** 宿主 list 的参数。未分类没有服务端筛选，按全部取回来再在本地过滤。 */
function listQuery(state, cursor, pageSize) {
  const query = { limit: pageSize, sort_order: 'desc' };
  const { kind, value } = state.scope;
  if (kind === 'starred') query.filter = 'starred';
  else query.filter = state.unreadOnly ? 'unread' : 'all';
  if (kind === 'source') query.source_id = value;
  else if (kind === 'category' && value !== UNCATEGORIZED) query.category = value;
  if (cursor) query.cursor = cursor;
  return query;
}

function markAllQuery(state) {
  const { kind, value } = state.scope;
  if (kind === 'source') return { source_id: value };
  if (kind === 'category' && value !== UNCATEGORIZED) return { category: value };
  return {};
}

/** 收藏页和「未分类」不能整批标已读：前者语义不对，后者宿主没有这个范围。 */
function canMarkAll(state) {
  if (!state.canWrite) return false;
  if (state.scope.kind === 'starred') return false;
  if (state.scope.kind === 'category' && state.scope.value === UNCATEGORIZED) return false;
  return true;
}

function shownItems(state, sourceOf) {
  const query = state.query.trim().toLowerCase();
  let items = state.items;
  if (state.scope.kind === 'category' && state.scope.value === UNCATEGORIZED && sourceOf) {
    items = items.filter((item) => {
      const source = sourceOf(item.source_id);
      return source && !String(source.category || '').trim();
    });
  }
  if (!query) return items;
  return items.filter((item) => {
    const hay = `${item.title || ''} ${item.source_name || ''} ${item.author || ''} ${item.summary || ''}`.toLowerCase();
    return hay.includes(query);
  });
}

/** 返回相邻文章的 id；还没选中时从第一篇开始；到头时返回 null，由调用方决定要不要再翻一页。 */
function neighbour(items, currentId, dir) {
  if (!items.length) return null;
  const index = items.findIndex((item) => item.id === currentId);
  if (index < 0) return items[0].id;
  const next = items[index + dir];
  return next ? next.id : null;
}

function clampStep(value) {
  const step = Number(value);
  if (!Number.isInteger(step)) return 0;
  return Math.max(FONT_STEPS[0], Math.min(FONT_STEPS[FONT_STEPS.length - 1], step));
}

/** 读回存过的偏好。v2 存的是 { filter, category, sourceId }，在这里换成新形状。 */
function restorePrefs(state, raw) {
  const prefs = raw && typeof raw === 'object' ? raw : {};
  if (prefs.version === PREFS_VERSION) {
    const scope = prefs.scope || {};
    if (scope.kind === 'all' || scope.kind === 'starred') state.scope = { kind: scope.kind, value: null };
    if (scope.kind === 'source' && Number.isInteger(scope.value)) state.scope = { kind: 'source', value: scope.value };
    if (scope.kind === 'category' && typeof scope.value === 'string' && scope.value) {
      state.scope = { kind: 'category', value: scope.value };
    }
    if (typeof prefs.unreadOnly === 'boolean') state.unreadOnly = prefs.unreadOnly;
    if (typeof prefs.navCollapsed === 'boolean') state.navCollapsed = prefs.navCollapsed;
    if (Array.isArray(prefs.collapsed)) state.collapsed = prefs.collapsed.filter((name) => typeof name === 'string').slice(0, 200);
    state.fontStep = clampStep(prefs.fontStep);
    if (prefs.fontFamily === 'sans' || prefs.fontFamily === 'serif') state.fontFamily = prefs.fontFamily;
    return state;
  }
  if (prefs.filter === 'starred') state.scope = { kind: 'starred', value: null };
  else if (Number.isInteger(prefs.sourceId)) state.scope = { kind: 'source', value: prefs.sourceId };
  else if (typeof prefs.category === 'string' && prefs.category) state.scope = { kind: 'category', value: prefs.category };
  if (prefs.filter === 'all') state.unreadOnly = false;
  if (prefs.filter === 'unread') state.unreadOnly = true;
  return state;
}

function prefsOf(state) {
  return {
    version: PREFS_VERSION,
    scope: state.scope,
    unreadOnly: state.unreadOnly,
    navCollapsed: state.navCollapsed,
    collapsed: state.collapsed,
    fontStep: state.fontStep,
    fontFamily: state.fontFamily,
  };
}

/** 源被删掉或分类改名后，存着的范围可能已经不存在了。 */
function scopeStillExists(state, isFeed, tokensOf) {
  const { kind, value } = state.scope;
  if (kind === 'source') return state.sources.some((source) => source.id === value && isFeed(source));
  if (kind === 'category') {
    if (value === UNCATEGORIZED) return true;
    return state.sources.some((source) => isFeed(source) && tokensOf(source.category).includes(value))
      || state.categories.some((category) => category && category.name === value);
  }
  return true;
}

module.exports = {
  PREFS_KEY,
  FONT_STEPS,
  UNCATEGORIZED,
  createState,
  sameScope,
  listQuery,
  markAllQuery,
  canMarkAll,
  shownItems,
  neighbour,
  clampStep,
  restorePrefs,
  prefsOf,
  scopeStillExists,
};
