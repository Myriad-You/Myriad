const { t, el, icon, iconButton, q, setPressed, toggleClass } = require('./dom.js');
const { categoryTokens, formatWhen, isFeed, localMedia, excerptOf, readingMinutes } = require('./format.js');
const { UNCATEGORIZED, canMarkAll, shownItems } = require('./model.js');
const { scopeTitle, sourceAvatar } = require('./nav.js');

function buildListHeader(state) {
  const head = el('header', { className: 'rss-pane-head rss-list-head' });
  const top = el('div', { className: 'rss-pane-head__row' });
  top.appendChild(iconButton('toggle-nav', 'sidebar', t('showFeeds'), { 'data-role': 'nav-toggle', className: 'rss-icon-btn rss-nav-toggle' }));
  const titles = el('div', { className: 'rss-pane-head__titles' });
  titles.append(
    el('h2', { className: 'rss-pane-title', 'data-role': 'list-title', id: 'rss-list-title' }),
    el('p', { className: 'rss-pane-sub', 'data-role': 'list-sub', 'aria-live': 'polite' }),
  );
  const tools = el('div', { className: 'rss-pane-head__tools' });
  tools.append(
    iconButton('toggle-search', 'search', t('search'), { 'data-role': 'search-toggle', 'aria-expanded': 'false', 'aria-controls': 'rss-search' }),
    iconButton('reload', 'refresh', t('reloadList'), { 'data-role': 'reload' }),
  );
  if (state.canWrite) {
    tools.appendChild(iconButton('ask-mark-all', 'checks', t('markAllRead'), { 'data-role': 'mark-all' }));
  }
  if (state.canManage) {
    tools.appendChild(iconButton('edit-scope-source', 'pencil', t('editSource'), { 'data-role': 'edit-source', hidden: true }));
  }
  top.append(titles, tools);

  const segment = el('div', { className: 'rss-segment', role: 'group', 'aria-label': t('show'), 'data-role': 'segment' });
  segment.append(
    el('button', { type: 'button', 'data-action': 'unread-only', 'data-value': 'true', 'aria-pressed': 'true' }, t('filterUnread')),
    el('button', { type: 'button', 'data-action': 'unread-only', 'data-value': 'false', 'aria-pressed': 'false' }, t('filterAll')),
  );

  const search = el('div', { className: 'rss-search', 'data-role': 'search-row', hidden: true });
  search.appendChild(icon('search'));
  search.appendChild(el('input', {
    type: 'search',
    id: 'rss-search',
    'data-field': 'search',
    placeholder: t('searchPlaceholder'),
    'aria-label': t('searchPlaceholder'),
    autocomplete: 'off',
    enterkeyhint: 'search',
  }));

  const confirm = el('div', { className: 'rss-confirm', 'data-role': 'mark-all-confirm', role: 'alertdialog', 'aria-live': 'assertive', hidden: true });
  const actions = el('div', { className: 'rss-confirm__actions' });
  confirm.append(el('p', { 'data-role': 'mark-all-text' }), actions);
  actions.append(
    el('button', { type: 'button', className: 'rss-btn is-small', 'data-action': 'cancel-mark-all' }, t('cancel')),
    el('button', { type: 'button', className: 'rss-btn is-primary is-small', 'data-action': 'mark-all' }, t('markAllReadConfirm')),
  );

  head.append(top, segment, search, confirm);
  return head;
}

function paintListHeader(root, state, ui) {
  const title = q(root, 'list-title');
  if (title) title.textContent = scopeTitle(state);
  const sub = q(root, 'list-sub');
  if (sub) sub.textContent = subtitle(state);
  const segment = q(root, 'segment');
  if (segment) {
    segment.hidden = state.scope.kind === 'starred';
    for (const option of segment.querySelectorAll('[data-action="unread-only"]')) {
      setPressed(option, String(state.unreadOnly) === option.getAttribute('data-value'));
    }
  }
  const markAll = q(root, 'mark-all');
  if (markAll) markAll.hidden = !canMarkAll(state);
  const edit = q(root, 'edit-source');
  if (edit) edit.hidden = state.scope.kind !== 'source';
  const searchRow = q(root, 'search-row');
  if (searchRow) searchRow.hidden = !ui.searchOpen;
  const searchToggle = q(root, 'search-toggle');
  if (searchToggle) {
    searchToggle.setAttribute('aria-expanded', ui.searchOpen ? 'true' : 'false');
    toggleClass(searchToggle, 'is-on', ui.searchOpen || Boolean(state.query));
  }
  const reload = q(root, 'reload');
  if (reload) toggleClass(reload, 'is-spinning', state.listLoading);
  const confirm = q(root, 'mark-all-confirm');
  if (confirm) confirm.hidden = !ui.confirmMarkAll;
  const text = q(root, 'mark-all-text');
  if (text) text.textContent = t('markAllReadAsk', { scope: scopeTitle(state) });
}

function subtitle(state) {
  if (state.listError) return t('listFailedShort');
  if (!state.booted || (state.listLoading && state.items.length === 0)) return t('loading');
  const count = shownItems(state, sourceLookup(state)).length;
  if (state.query.trim()) return t('searchHits', { count });
  if (state.scope.kind === 'starred') return t('countStarred', { count: countLabel(state, count) });
  if (state.unreadOnly) {
    const known = scopeUnread(state);
    return t('countUnread', { count: known == null ? countLabel(state, count) : String(known) });
  }
  return t('countAll', { count: countLabel(state, count) });
}

/** 未读数用站点给的计数，不用已加载的条数：分页时后者只是「30+」。 */
function scopeUnread(state) {
  const { kind, value } = state.scope;
  if (kind === 'all') return state.stats && typeof state.stats.total_unread === 'number' ? state.stats.total_unread : null;
  const feeds = state.sources.filter(isFeed);
  let picked = [];
  if (kind === 'source') picked = feeds.filter((source) => source.id === value);
  else if (kind === 'category' && value === UNCATEGORIZED) picked = feeds.filter((source) => categoryTokens(source.category).length === 0);
  else if (kind === 'category') picked = feeds.filter((source) => categoryTokens(source.category).includes(value));
  if (picked.length === 0) return null;
  return picked.reduce((total, source) => total + (Number(source.unread_count) || 0), 0);
}

function countLabel(state, count) {
  return state.nextCursor ? `${count}+` : String(count);
}

function sourceLookup(state) {
  return (id) => state.sources.find((source) => source.id === id);
}

function renderList(root, state) {
  const host = q(root, 'list-body');
  if (!host) return;
  const items = shownItems(state, sourceLookup(state));
  const list = el('div', { className: 'rss-rows' });

  if (state.listError && items.length === 0) {
    host.replaceChildren(errorState(state.listError));
    return;
  }
  if (items.length === 0 && (state.listLoading || !state.booted)) {
    host.replaceChildren(listSkeleton());
    return;
  }
  if (items.length === 0) {
    host.replaceChildren(emptyList(state));
    return;
  }
  for (const item of items) list.appendChild(row(state, item));
  if (state.nextCursor) {
    const more = el('div', { className: 'rss-more', 'data-role': 'sentinel' });
    more.appendChild(el('button', {
      type: 'button',
      className: 'rss-btn is-quiet',
      'data-action': 'more',
      disabled: state.listLoading ? true : null,
    }, state.listLoading ? t('loading') : t('loadMore')));
    list.appendChild(more);
  } else if (!state.query.trim()) {
    list.appendChild(el('p', { className: 'rss-end' }, t('endOfList')));
  }
  host.replaceChildren(list);
}

function row(state, item) {
  const selected = state.selectedId === item.id;
  const node = el('button', {
    type: 'button',
    className: `rss-row${item.is_read ? '' : ' is-unread'}${selected ? ' is-selected' : ''}`,
    'data-action': 'open',
    'data-id': item.id,
    'aria-current': selected ? 'true' : null,
  });
  fillRow(node, state, item);
  return node;
}

function fillRow(node, state, item) {
  const text = el('span', { className: 'rss-row__text' });
  const meta = el('span', { className: 'rss-row__meta' });
  meta.appendChild(el('span', { className: 'rss-row__dot', 'aria-hidden': 'true' }));
  const source = state.sources.find((entry) => entry.id === item.source_id);
  meta.appendChild(sourceAvatar(source || { name: item.source_name, icon: item.source_icon }));
  meta.appendChild(el('span', { className: 'rss-row__source' }, item.source_name || ''));
  const when = formatWhen(item.published_at);
  if (when) meta.appendChild(el('time', { className: 'rss-row__time' }, when));
  if (item.is_starred) {
    const star = icon('star', 'rss-row__star');
    meta.appendChild(star);
  }
  text.appendChild(meta);
  text.appendChild(el('span', { className: 'rss-row__title' }, item.title || t('untitled')));
  const excerpt = excerptOf(item);
  if (excerpt) text.appendChild(el('span', { className: 'rss-row__excerpt' }, excerpt));
  const minutes = readingMinutes(item);
  if (minutes) text.appendChild(el('span', { className: 'rss-row__foot' }, t('minutes', { count: minutes })));
  node.replaceChildren(text);
  const thumb = localMedia(item.image);
  if (thumb) node.appendChild(el('img', { className: 'rss-row__thumb', src: thumb, alt: '', loading: 'lazy', decoding: 'async' }));
  const label = [
    item.is_read ? null : t('unreadLabel'),
    item.is_starred ? t('starredLabel') : null,
    item.title || t('untitled'),
    item.source_name,
    when,
  ].filter(Boolean).join(', ');
  node.setAttribute('aria-label', label);
}

/** 只改一行，不重画整列：滚动位置和焦点都留在原处。 */
function updateRow(root, state, id) {
  const node = root.querySelector(`.rss-row[data-id="${id}"]`);
  const item = state.items.find((entry) => entry.id === id);
  if (!node || !item) return;
  toggleClass(node, 'is-unread', !item.is_read);
  fillRow(node, state, item);
}

function markSelected(root, state) {
  for (const node of root.querySelectorAll('.rss-row')) {
    const on = Number(node.getAttribute('data-id')) === state.selectedId;
    toggleClass(node, 'is-selected', on);
    if (on) node.setAttribute('aria-current', 'true');
    else node.removeAttribute && node.removeAttribute('aria-current');
  }
}

/** 只滚列表自己。scrollIntoView 会连带滚动外层容器，把整个界面推歪。 */
function revealSelected(root, state) {
  const node = root.querySelector(`.rss-row[data-id="${state.selectedId}"]`);
  const scroller = q(root, 'list-body');
  if (!node || !scroller || typeof node.getBoundingClientRect !== 'function') return;
  const box = node.getBoundingClientRect();
  const view = scroller.getBoundingClientRect();
  const margin = 8;
  if (box.top < view.top + margin) scroller.scrollTop -= view.top + margin - box.top;
  else if (box.bottom > view.bottom - margin) scroller.scrollTop += box.bottom - (view.bottom - margin);
}

function listSkeleton() {
  const box = el('div', { className: 'rss-rows', 'aria-hidden': 'true' });
  for (let index = 0; index < 7; index += 1) {
    const item = el('div', { className: 'rss-row is-skeleton' });
    const text = el('span', { className: 'rss-row__text' });
    text.append(
      el('span', { className: 'rss-line is-short rss-shimmer' }),
      el('span', { className: 'rss-line is-title rss-shimmer' }),
      el('span', { className: 'rss-line rss-shimmer' }),
    );
    item.appendChild(text);
    box.appendChild(item);
  }
  return box;
}

function emptyList(state) {
  const box = el('div', { className: 'rss-empty' });
  if (state.query.trim()) {
    box.append(el('strong', null, t('noHits')), el('p', null, t('noHitsHint')));
    box.appendChild(el('button', { type: 'button', className: 'rss-btn is-small', 'data-action': 'clear-search' }, t('clearSearch')));
    return box;
  }
  if (state.scope.kind === 'starred') {
    box.append(el('strong', null, t('emptyStarred')), el('p', null, t('emptyStarredHint')));
    return box;
  }
  if (state.unreadOnly) {
    box.appendChild(icon('check', 'rss-empty__mark'));
    box.append(el('strong', null, t('allCaughtUp')), el('p', null, t('allCaughtUpHint')));
    box.appendChild(el('button', { type: 'button', className: 'rss-btn is-small', 'data-action': 'unread-only', 'data-value': 'false' }, t('showAll')));
    return box;
  }
  box.append(el('strong', null, t('emptyItems')), el('p', null, state.canManage ? t('emptyItemsManage') : t('emptyItemsHint')));
  return box;
}

function errorState(message) {
  const box = el('div', { className: 'rss-empty is-error', role: 'alert' });
  box.appendChild(icon('alert', 'rss-empty__mark'));
  box.append(el('strong', null, t('listFailed')), el('p', null, message));
  box.appendChild(el('button', { type: 'button', className: 'rss-btn is-small', 'data-action': 'reload' }, t('retry')));
  return box;
}

module.exports = { buildListHeader, paintListHeader, renderList, updateRow, markSelected, revealSelected, sourceLookup };
