// 管理面板只在授予 phantasi:manage 时建出来。输入框只在切页或数据变了时重画，
// 打字过程中不碰 DOM，免得把正在输入的内容冲掉。

const { t, el, icon, iconButton, q } = require('./dom.js');
const { categoryTokens, formatWhen, hostOf, isFeed } = require('./format.js');
const { sourceAvatar } = require('./nav.js');

const INTERVALS = [15, 30, 60, 180, 360, 720, 1440];
const TABS = ['sources', 'categories', 'transfer'];

function createManageUi() {
  return {
    open: false,
    tab: 'sources',
    expandedId: null,
    pendingDelete: null,
    discovered: null,
    busy: '',
    refreshing: null,
    returnFocus: null,
  };
}

function buildManage() {
  const layer = el('div', { className: 'rss-layer', 'data-role': 'manage-layer', hidden: true });
  layer.appendChild(el('button', { type: 'button', className: 'rss-scrim', 'data-action': 'close-manage', tabindex: '-1', 'aria-hidden': 'true' }));
  const dialog = el('section', {
    className: 'rss-dialog',
    role: 'dialog',
    'aria-modal': 'true',
    'aria-labelledby': 'rss-manage-title',
    'data-role': 'manage',
  });
  const head = el('header', { className: 'rss-dialog__head' });
  head.append(
    el('h2', { id: 'rss-manage-title' }, t('manage')),
    iconButton('close-manage', 'close', t('close'), { 'data-role': 'manage-close' }),
  );
  const tabs = el('div', { className: 'rss-tabs', role: 'tablist', 'aria-label': t('manage') });
  for (const tab of TABS) {
    tabs.appendChild(el('button', {
      type: 'button',
      role: 'tab',
      id: `rss-tab-${tab}`,
      'aria-controls': 'rss-manage-panel',
      'data-action': 'manage-tab',
      'data-tab': tab,
    }, t(`tab_${tab}`)));
  }
  const panel = el('div', { className: 'rss-dialog__body', role: 'tabpanel', id: 'rss-manage-panel', 'data-role': 'manage-panel', tabindex: '-1' });
  dialog.append(head, tabs, panel);
  layer.appendChild(dialog);
  return layer;
}

function paintManageFrame(root, ui) {
  const layer = q(root, 'manage-layer');
  if (!layer) return;
  layer.hidden = !ui.open;
  for (const tab of layer.querySelectorAll('[role="tab"]')) {
    const on = tab.getAttribute('data-tab') === ui.tab;
    tab.setAttribute('aria-selected', on ? 'true' : 'false');
    tab.setAttribute('tabindex', on ? '0' : '-1');
  }
  const panel = q(root, 'manage-panel');
  if (panel) panel.setAttribute('aria-labelledby', `rss-tab-${ui.tab}`);
}

function renderManage(root, state, ui) {
  paintManageFrame(root, ui);
  const panel = q(root, 'manage-panel');
  if (!panel || !ui.open) return;
  if (ui.tab === 'categories') panel.replaceChildren(categoriesTab(state, ui));
  else if (ui.tab === 'transfer') panel.replaceChildren(transferTab(state, ui));
  else panel.replaceChildren(sourcesTab(state, ui));
}

function labelled(id, label, control, hint) {
  const wrap = el('div', { className: 'rss-field' });
  wrap.appendChild(el('label', { for: id }, label));
  control.setAttribute('id', id);
  wrap.appendChild(control);
  if (hint) wrap.appendChild(el('p', { className: 'rss-field__hint' }, hint));
  return wrap;
}

function categoryList(state) {
  const names = new Set();
  for (const category of state.categories) if (category && category.name) names.add(category.name);
  for (const source of state.sources) for (const token of categoryTokens(source.category)) names.add(token);
  const list = el('datalist', { id: 'rss-category-names' });
  for (const name of [...names].sort((a, b) => a.localeCompare(b))) list.appendChild(el('option', { value: name }));
  return list;
}

function sourcesTab(state, ui) {
  const box = el('div', { className: 'rss-manage' });
  box.appendChild(categoryList(state));

  const add = el('section', { className: 'rss-card', 'aria-labelledby': 'rss-add-title' });
  add.appendChild(el('h3', { id: 'rss-add-title' }, t('addSource')));
  const urlRow = el('div', { className: 'rss-inline' });
  urlRow.append(
    el('input', {
      type: 'url',
      id: 'rss-add-url',
      'data-field': 'add-url',
      'data-enter': 'discover',
      placeholder: 'https://example.com/feed.xml',
      inputmode: 'url',
      autocomplete: 'off',
      spellcheck: 'false',
      value: ui.discovered ? ui.discovered.url : null,
    }),
    el('button', { type: 'button', className: 'rss-btn', 'data-action': 'discover', disabled: ui.busy === 'discover' ? true : null },
      ui.busy === 'discover' ? t('discovering') : t('discover')),
  );
  const urlField = el('div', { className: 'rss-field' });
  urlField.append(
    el('label', { for: 'rss-add-url' }, t('feedUrl')),
    urlRow,
    el('p', { className: 'rss-field__hint' }, t('feedUrlHint')),
  );
  add.appendChild(urlField);
  if (ui.discovered) {
    const found = el('div', { className: 'rss-found' });
    found.appendChild(el('p', { className: 'rss-found__title' }, t('discovered', { title: ui.discovered.title || hostOf(ui.discovered.url) })));
    const facts = [hostOf(ui.discovered.url), ui.discovered.itemCount ? t('itemsFound', { count: ui.discovered.itemCount }) : '']
      .filter(Boolean).join(' · ');
    if (facts) found.appendChild(el('p', { className: 'rss-muted' }, facts));
    if (ui.discovered.description) found.appendChild(el('p', { className: 'rss-found__desc' }, ui.discovered.description.slice(0, 240)));
    found.append(
      labelled('rss-add-name', t('sourceName'), el('input', { type: 'text', 'data-field': 'add-name', value: ui.discovered.title || '', 'data-enter': 'add-source' })),
      labelled('rss-add-category', t('category'), el('input', { type: 'text', 'data-field': 'add-category', list: 'rss-category-names', 'data-enter': 'add-source', placeholder: t('categoryPlaceholder') })),
    );
    const actions = el('div', { className: 'rss-actions' });
    actions.append(
      el('button', { type: 'button', className: 'rss-btn is-quiet', 'data-action': 'reset-discover' }, t('cancel')),
      el('button', { type: 'button', className: 'rss-btn is-primary', 'data-action': 'add-source', disabled: ui.busy === 'add' ? true : null }, ui.busy === 'add' ? t('adding') : t('add')),
    );
    found.appendChild(actions);
    add.appendChild(found);
  }
  box.appendChild(add);

  const feeds = state.sources.filter(isFeed).sort((a, b) => String(a.name || a.url).localeCompare(String(b.name || b.url)));
  const list = el('section', { className: 'rss-card is-list', 'aria-labelledby': 'rss-sources-title' });
  const head = el('div', { className: 'rss-card__head' });
  head.append(el('h3', { id: 'rss-sources-title' }, t('sourcesCount', { count: feeds.length })));
  list.appendChild(head);
  if (feeds.length === 0) list.appendChild(el('p', { className: 'rss-muted' }, t('emptySourcesManage')));
  for (const source of feeds) list.appendChild(sourceEntry(state, ui, source));
  box.appendChild(list);
  return box;
}

function sourceEntry(state, ui, source) {
  const open = ui.expandedId === source.id;
  const wrap = el('div', { className: `rss-entry${open ? ' is-open' : ''}` });
  const summary = el('button', {
    type: 'button',
    className: 'rss-entry__summary',
    'data-action': 'expand-source',
    'data-id': source.id,
    'aria-expanded': open ? 'true' : 'false',
    'aria-controls': `rss-source-${source.id}`,
  });
  const text = el('span', { className: 'rss-entry__text' });
  text.append(
    el('span', { className: 'rss-entry__name' }, source.name || source.url),
    el('span', { className: 'rss-entry__meta' }, [hostOf(source.url), source.category].filter(Boolean).join(' · ')),
  );
  summary.append(sourceAvatar(source), text);
  if (source.enabled === false) summary.appendChild(el('span', { className: 'rss-tag' }, t('paused')));
  if (source.last_error) summary.appendChild(el('span', { className: 'rss-tag is-danger' }, t('failing')));
  summary.appendChild(icon('chevron', 'rss-entry__chevron'));
  wrap.appendChild(summary);
  if (open) wrap.appendChild(sourceEditor(state, ui, source));
  return wrap;
}

function sourceEditor(state, ui, source) {
  const form = el('div', { className: 'rss-entry__body', id: `rss-source-${source.id}` });
  const status = el('p', { className: source.last_error ? 'rss-note is-danger' : 'rss-note' });
  status.textContent = source.last_error
    ? t('lastError', { message: source.last_error })
    : source.last_success_at
      ? t('lastSuccess', { when: formatWhen(source.last_success_at), count: source.item_count || 0 })
      : t('neverFetched');
  form.appendChild(status);

  const interval = el('select', { 'data-field': 'edit-interval' });
  const current = Number(source.update_interval) || 60;
  const options = INTERVALS.includes(current) ? INTERVALS : INTERVALS.concat(current).sort((a, b) => a - b);
  for (const minutes of options) {
    const option = el('option', { value: String(minutes) }, intervalLabel(minutes));
    if (minutes === current) option.selected = true;
    interval.appendChild(option);
  }
  const enabled = el('input', { type: 'checkbox', role: 'switch', 'data-field': 'edit-enabled', id: `rss-enabled-${source.id}` });
  if (source.enabled !== false) enabled.checked = true;
  const switchRow = el('label', { className: 'rss-switch', for: `rss-enabled-${source.id}` });
  switchRow.append(enabled, el('span', { className: 'rss-switch__track', 'aria-hidden': 'true' }), el('span', null, t('enabled')));

  const grid = el('div', { className: 'rss-grid' });
  grid.append(
    labelled(`rss-edit-name-${source.id}`, t('sourceName'), el('input', { type: 'text', 'data-field': 'edit-name', value: source.name || '', 'data-enter': 'save-source' })),
    labelled(`rss-edit-url-${source.id}`, t('feedUrl'), el('input', { type: 'url', 'data-field': 'edit-url', value: source.url || '', spellcheck: 'false', 'data-enter': 'save-source' })),
    labelled(`rss-edit-category-${source.id}`, t('category'), el('input', { type: 'text', 'data-field': 'edit-category', list: 'rss-category-names', value: source.category || '', placeholder: t('categoryPlaceholder'), 'data-enter': 'save-source' }), t('categoryHint')),
    labelled(`rss-edit-interval-${source.id}`, t('interval'), interval),
  );
  form.append(grid, switchRow);

  const actions = el('div', { className: 'rss-actions' });
  const deleting = ui.pendingDelete === `source:${source.id}`;
  actions.append(
    el('button', { type: 'button', className: deleting ? 'rss-btn is-danger is-solid' : 'rss-btn is-danger', 'data-action': 'delete-source', 'data-id': source.id }, deleting ? t('confirmDeleteSource') : t('delete')),
    el('span', { className: 'rss-spacer' }),
    el('button', { type: 'button', className: 'rss-btn', 'data-action': 'refresh-source', 'data-id': source.id, disabled: ui.busy === `refresh:${source.id}` ? true : null }, ui.busy === `refresh:${source.id}` ? t('refreshing') : t('refresh')),
    el('button', { type: 'button', className: 'rss-btn is-primary', 'data-action': 'save-source', 'data-id': source.id, disabled: ui.busy === `save:${source.id}` ? true : null }, t('save')),
  );
  if (deleting) {
    form.appendChild(el('p', { className: 'rss-note is-danger', role: 'alert' }, t('deleteSourceWarning', { count: source.item_count || 0 })));
  }
  form.appendChild(actions);
  return form;
}

function intervalLabel(minutes) {
  if (minutes < 60) return t('everyMinutes', { count: minutes });
  if (minutes % 1440 === 0) return t('everyDays', { count: minutes / 1440 });
  if (minutes % 60 === 0) return t('everyHours', { count: minutes / 60 });
  return t('everyMinutes', { count: minutes });
}

function categoriesTab(state, ui) {
  const box = el('div', { className: 'rss-manage' });
  const add = el('section', { className: 'rss-card' });
  add.appendChild(el('h3', null, t('newCategory')));
  const row = el('div', { className: 'rss-inline' });
  row.append(
    el('input', { type: 'text', 'data-field': 'new-category', 'aria-label': t('newCategory'), placeholder: t('newCategoryPlaceholder'), 'data-enter': 'add-category' }),
    el('button', { type: 'button', className: 'rss-btn is-primary', 'data-action': 'add-category' }, t('add')),
  );
  add.appendChild(row);
  add.appendChild(el('p', { className: 'rss-field__hint' }, t('categoryExplain')));
  box.appendChild(add);

  const list = el('section', { className: 'rss-card is-list' });
  list.appendChild(el('h3', null, t('categories')));
  if (state.categories.length === 0) list.appendChild(el('p', { className: 'rss-muted' }, t('noCategories')));
  for (const category of state.categories) {
    const used = state.sources.filter((source) => categoryTokens(source.category).includes(category.name)).length;
    const item = el('div', { className: 'rss-entry is-flat' });
    const line = el('div', { className: 'rss-inline' });
    const input = el('input', {
      type: 'text',
      'data-field': `rename-category-${category.id}`,
      'aria-label': t('renameCategory', { name: category.name }),
      value: category.name || '',
      'data-enter': 'rename-category',
      'data-id': category.id,
    });
    const deleting = ui.pendingDelete === `category:${category.id}`;
    line.append(
      input,
      el('span', { className: 'rss-entry__meta' }, t('sourcesCount', { count: used })),
      el('button', { type: 'button', className: 'rss-btn is-small', 'data-action': 'rename-category', 'data-id': category.id }, t('rename')),
      el('button', { type: 'button', className: deleting ? 'rss-btn is-small is-danger is-solid' : 'rss-btn is-small is-danger', 'data-action': 'delete-category', 'data-id': category.id }, deleting ? t('confirmDelete') : t('delete')),
    );
    item.appendChild(line);
    list.appendChild(item);
  }
  box.appendChild(list);
  return box;
}

function transferTab(state, ui) {
  const box = el('div', { className: 'rss-manage' });
  const opml = el('section', { className: 'rss-card' });
  opml.append(el('h3', null, t('opml')), el('p', { className: 'rss-muted' }, t('opmlExplain')));
  const actions = el('div', { className: 'rss-actions is-start' });
  const picker = el('label', { className: 'rss-btn rss-file' });
  picker.append(icon('upload'), el('span', null, ui.busy === 'import' ? t('importing') : t('importOpml')));
  picker.appendChild(el('input', { type: 'file', accept: '.opml,.xml,text/xml,text/x-opml', 'data-action': 'import-opml', disabled: ui.busy === 'import' ? true : null }));
  const exportBtn = el('button', { type: 'button', className: 'rss-btn', 'data-action': 'export-opml' });
  exportBtn.append(icon('download'), el('span', null, t('exportOpml')));
  actions.append(picker, exportBtn);
  opml.appendChild(actions);
  box.appendChild(opml);

  const fetchAll = el('section', { className: 'rss-card' });
  fetchAll.append(el('h3', null, t('refreshAll')), el('p', { className: 'rss-muted' }, t('refreshAllExplain')));
  const run = el('div', { className: 'rss-actions is-start' });
  const refreshing = ui.refreshing;
  const runBtn = el('button', { type: 'button', className: 'rss-btn', 'data-action': 'refresh-all', disabled: refreshing ? true : null });
  runBtn.append(icon('refresh'), el('span', null, refreshing
    ? t('refreshProgress', { done: refreshing.done, total: refreshing.total })
    : t('refreshAllRun', { count: state.sources.filter(isFeed).length })));
  run.appendChild(runBtn);
  fetchAll.appendChild(run);
  if (refreshing) {
    const meter = el('div', { className: 'rss-meter', role: 'progressbar', 'aria-valuemin': '0', 'aria-valuemax': String(refreshing.total), 'aria-valuenow': String(refreshing.done) });
    const fill = el('span');
    if (fill.style) fill.style.width = `${refreshing.total ? (refreshing.done / refreshing.total) * 100 : 0}%`;
    meter.appendChild(fill);
    fetchAll.appendChild(meter);
  }
  box.appendChild(fetchAll);
  return box;
}

module.exports = { TABS, createManageUi, buildManage, renderManage, paintManageFrame };
