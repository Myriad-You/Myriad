// 管理面板里的操作。只有授予 phantasi:manage 时才会走到这里；宿主还会再按端点鉴权一次。

const api = require('./api.js');
const { t, q, nextFrame } = require('./dom.js');
const { isFeed } = require('./format.js');
const { renderManage } = require('./manage.js');
const reading = require('./reading.js');
const view = require('./view.js');

function field(root, name, scope) {
  const selector = `[data-field="${name}"]`;
  const node = scope ? root.querySelector(`#${scope} ${selector}`) : root.querySelector(selector);
  if (!node) return '';
  if (node.type === 'checkbox') return node.checked;
  return String(node.value || '').trim();
}

function repaint(ctx) {
  renderManage(ctx.root, ctx.state, ctx.ui.manage);
}

function focusSoon(ctx, selector) {
  nextFrame(() => {
    const node = ctx.root.querySelector(selector);
    if (node && typeof node.focus === 'function') node.focus();
  });
}

function openManage(ctx, tab, options) {
  if (!ctx.state.canManage) return;
  const ui = ctx.ui.manage;
  const opts = options || {};
  ui.returnFocus = typeof document !== 'undefined' ? document.activeElement : null;
  ui.open = true;
  ui.tab = tab || 'sources';
  ui.pendingDelete = null;
  if (opts.sourceId != null) ui.expandedId = opts.sourceId;
  ctx.state.drawer = false;
  view.paintLayout(ctx);
  repaint(ctx);
  if (opts.intent === 'add') focusSoon(ctx, '[data-field="add-url"]');
  else if (opts.sourceId != null) focusSoon(ctx, `#rss-source-${opts.sourceId} [data-field="edit-name"]`);
  else focusSoon(ctx, `[data-tab="${ui.tab}"][role="tab"]`);
}

function closeManage(ctx) {
  const ui = ctx.ui.manage;
  if (!ui.open) return;
  ui.open = false;
  ui.pendingDelete = null;
  repaint(ctx);
  const back = ui.returnFocus;
  ui.returnFocus = null;
  if (back && typeof back.focus === 'function') nextFrame(() => back.focus());
}

function switchTab(ctx, tab) {
  const ui = ctx.ui.manage;
  ui.tab = tab;
  ui.pendingDelete = null;
  repaint(ctx);
  focusSoon(ctx, `[data-tab="${tab}"][role="tab"]`);
}

async function guarded(ctx, busy, work) {
  const ui = ctx.ui.manage;
  ui.busy = busy;
  repaint(ctx);
  try {
    await work();
  } catch (error) {
    view.toast(ctx, t('failed', { message: view.errorText(error) }), { error: true });
  } finally {
    ui.busy = '';
    repaint(ctx);
  }
}

async function discover(ctx) {
  const url = field(ctx.root, 'add-url');
  if (!url) {
    view.toast(ctx, t('needUrl'));
    focusSoon(ctx, '[data-field="add-url"]');
    return;
  }
  await guarded(ctx, 'discover', async () => {
    const found = await api.discover(url);
    ctx.ui.manage.discovered = {
      url: (found && found.url) || url,
      title: (found && found.title) || '',
      description: (found && found.description) || '',
      itemCount: Number(found && found.item_count) || 0,
    };
  });
  if (ctx.ui.manage.discovered) focusSoon(ctx, '[data-field="add-name"]');
}

function resetDiscover(ctx) {
  ctx.ui.manage.discovered = null;
  repaint(ctx);
  focusSoon(ctx, '[data-field="add-url"]');
}

async function addSource(ctx) {
  const url = field(ctx.root, 'add-url');
  if (!url) {
    view.toast(ctx, t('needUrl'));
    return;
  }
  let added = false;
  await guarded(ctx, 'add', async () => {
    await api.addSource({
      url,
      name: field(ctx.root, 'add-name') || undefined,
      category: field(ctx.root, 'add-category') || undefined,
    });
    added = true;
  });
  if (!added) return;
  ctx.ui.manage.discovered = null;
  view.toast(ctx, t('sourceAdded'));
  await reading.reloadAll(ctx);
  repaint(ctx);
  focusSoon(ctx, '[data-field="add-url"]');
}

function expandSource(ctx, id) {
  const ui = ctx.ui.manage;
  ui.expandedId = ui.expandedId === id ? null : id;
  ui.pendingDelete = null;
  repaint(ctx);
  if (ui.expandedId != null) focusSoon(ctx, `#rss-source-${id} [data-field="edit-name"]`);
}

async function saveSource(ctx, id) {
  const scope = `rss-source-${id}`;
  const interval = Number(field(ctx.root, 'edit-interval', scope));
  let saved = false;
  await guarded(ctx, `save:${id}`, async () => {
    await api.updateSource(id, {
      name: field(ctx.root, 'edit-name', scope),
      url: field(ctx.root, 'edit-url', scope) || undefined,
      category: field(ctx.root, 'edit-category', scope),
      update_interval: Number.isFinite(interval) && interval > 0 ? interval : undefined,
      enabled: field(ctx.root, 'edit-enabled', scope) === true,
    });
    saved = true;
  });
  if (!saved) return;
  view.toast(ctx, t('saved'));
  await reading.reloadAll(ctx);
  repaint(ctx);
}

async function refreshSource(ctx, id) {
  await guarded(ctx, `refresh:${id}`, async () => {
    const result = await api.refreshSource(id);
    view.toast(ctx, t('refreshDone', { count: (result && result.new_items) || 0 }));
  });
  await reading.reloadAll(ctx);
  repaint(ctx);
}

async function deleteSource(ctx, id) {
  const ui = ctx.ui.manage;
  const key = `source:${id}`;
  if (ui.pendingDelete !== key) {
    ui.pendingDelete = key;
    repaint(ctx);
    focusSoon(ctx, `[data-action="delete-source"][data-id="${id}"]`);
    return;
  }
  let gone = false;
  await guarded(ctx, `delete:${id}`, async () => {
    await api.deleteSource(id);
    gone = true;
  });
  ui.pendingDelete = null;
  if (!gone) return;
  ui.expandedId = null;
  if (ctx.state.scope.kind === 'source' && ctx.state.scope.value === id) {
    ctx.state.scope = { kind: 'all', value: null };
  }
  view.toast(ctx, t('deleted'));
  await reading.reloadAll(ctx);
  repaint(ctx);
}

async function addCategory(ctx) {
  const name = field(ctx.root, 'new-category');
  if (!name) {
    focusSoon(ctx, '[data-field="new-category"]');
    return;
  }
  await guarded(ctx, 'category', async () => {
    await api.createCategory(name);
    view.toast(ctx, t('saved'));
  });
  await reading.reloadAll(ctx);
  repaint(ctx);
  focusSoon(ctx, '[data-field="new-category"]');
}

async function renameCategory(ctx, id) {
  const name = field(ctx.root, `rename-category-${id}`);
  const current = ctx.state.categories.find((category) => category.id === id);
  if (!name || (current && current.name === name)) return;
  await guarded(ctx, 'category', async () => {
    await api.updateCategory(id, { name });
    if (current && ctx.state.scope.kind === 'category' && ctx.state.scope.value === current.name) {
      ctx.state.scope = { kind: 'category', value: name };
    }
    view.toast(ctx, t('saved'));
  });
  await reading.reloadAll(ctx);
  repaint(ctx);
}

async function deleteCategory(ctx, id) {
  const ui = ctx.ui.manage;
  const key = `category:${id}`;
  if (ui.pendingDelete !== key) {
    ui.pendingDelete = key;
    repaint(ctx);
    focusSoon(ctx, `[data-action="delete-category"][data-id="${id}"]`);
    return;
  }
  await guarded(ctx, 'category', async () => {
    await api.deleteCategory(id);
    view.toast(ctx, t('deleted'));
  });
  ui.pendingDelete = null;
  await reading.reloadAll(ctx);
  repaint(ctx);
}

function importOpml(ctx, input) {
  const file = input.files && input.files[0];
  input.value = '';
  if (!file) return;
  const reader = new FileReader();
  ctx.ui.manage.busy = 'import';
  repaint(ctx);
  reader.onerror = () => {
    ctx.ui.manage.busy = '';
    repaint(ctx);
    view.toast(ctx, t('failed', { message: t('unknownError') }), { error: true });
  };
  reader.onload = () => {
    void guarded(ctx, 'import', async () => {
      const result = await api.importOpml(String(reader.result || ''));
      view.toast(ctx, t('imported', {
        imported: (result && result.imported) || 0,
        skipped: (result && result.skipped) || 0,
      }));
      await reading.reloadAll(ctx);
    });
  };
  reader.readAsText(file);
}

/** 一个源一个源地抓，进度如实显示；某个源失败不打断其余的。 */
async function refreshAll(ctx) {
  const ui = ctx.ui.manage;
  if (ui.refreshing) return;
  const feeds = ctx.state.sources.filter((source) => isFeed(source) && source.enabled !== false);
  ui.refreshing = { done: 0, total: feeds.length };
  repaint(ctx);
  let count = 0;
  let failed = 0;
  for (const source of feeds) {
    try {
      const result = await api.refreshSource(source.id);
      count += (result && result.new_items) || 0;
    } catch {
      failed += 1;
    }
    ui.refreshing.done += 1;
    repaint(ctx);
  }
  ui.refreshing = null;
  view.toast(ctx, failed
    ? t('refreshDoneWithErrors', { count, failed })
    : t('refreshDone', { count }), { error: failed > 0 });
  await reading.reloadAll(ctx);
  repaint(ctx);
}

module.exports = {
  openManage,
  closeManage,
  switchTab,
  discover,
  resetDiscover,
  addSource,
  expandSource,
  saveSource,
  refreshSource,
  deleteSource,
  addCategory,
  renameCategory,
  deleteCategory,
  importOpml,
  refreshAll,
  isOpen: (ctx) => Boolean(ctx.ui.manage && ctx.ui.manage.open),
  dialog: (ctx) => q(ctx.root, 'manage'),
};
