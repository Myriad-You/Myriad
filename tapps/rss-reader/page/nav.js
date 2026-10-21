const { t, el, icon, q } = require('./dom.js');
const { categoryTokens, isFeed, localMedia, monogram } = require('./format.js');
const { UNCATEGORIZED, sameScope } = require('./model.js');

function feeds(state) {
  return state.sources.filter(isFeed);
}

/** 分类按名字排序；源可以同时属于几个分类，每个分类下都列一次。 */
function groups(state) {
  const names = new Set();
  const loose = [];
  for (const source of feeds(state)) {
    const tokens = categoryTokens(source.category);
    if (tokens.length === 0) loose.push(source);
    for (const token of tokens) names.add(token);
  }
  for (const category of state.categories) {
    if (category && category.name) names.add(category.name);
  }
  const sorted = [...names].sort((a, b) => a.localeCompare(b));
  return {
    folders: sorted.map((name) => ({
      name,
      sources: feeds(state)
        .filter((source) => categoryTokens(source.category).includes(name))
        .sort(bySourceName),
    })),
    loose: loose.sort(bySourceName),
  };
}

function bySourceName(a, b) {
  return String(a.name || a.url || '').localeCompare(String(b.name || b.url || ''));
}

function sum(sources) {
  return sources.reduce((total, source) => total + (Number(source.unread_count) || 0), 0);
}

function scopeButton(state, scope, label, options) {
  const opts = options || {};
  const on = sameScope(state.scope, scope);
  const node = el('button', {
    type: 'button',
    className: `rss-nav-item${opts.depth ? ' is-nested' : ''}${opts.muted ? ' is-muted' : ''}`,
    'data-action': 'scope',
    'data-kind': scope.kind,
    'data-value': scope.value == null ? '' : String(scope.value),
    'aria-current': on ? 'page' : null,
  });
  if (opts.leading) node.appendChild(opts.leading);
  node.appendChild(el('span', { className: 'rss-nav-item__label' }, label));
  if (opts.alert) {
    const warn = icon('alert', 'rss-nav-item__warn');
    node.appendChild(warn);
  }
  if (opts.count) node.appendChild(el('span', { className: 'rss-badge' }, formatCount(opts.count)));
  return node;
}

function formatCount(value) {
  const count = Number(value) || 0;
  return count > 999 ? '999+' : String(count);
}

// 取不到的图标记下来，侧栏重画时直接用首字母，不再请求一遍、闪一下。
const brokenIcons = new Set();

function markBrokenIcon(src) {
  if (src) brokenIcons.add(src);
}

function sourceAvatar(source) {
  const src = localMedia(source.icon);
  if (src && !brokenIcons.has(src)) {
    return el('img', {
      className: 'rss-avatar',
      src,
      alt: '',
      loading: 'lazy',
      decoding: 'async',
      'data-fallback': monogram(source.name || source.url),
    });
  }
  return el('span', { className: 'rss-avatar is-mono', 'aria-hidden': 'true' }, monogram(source.name || source.url));
}

function sourceRow(state, source, depth) {
  return scopeButton(state, { kind: 'source', value: source.id }, source.name || source.url || String(source.id), {
    depth,
    leading: sourceAvatar(source),
    count: source.unread_count,
    muted: source.enabled === false,
    alert: Boolean(source.last_error) && state.canManage,
  });
}

function renderNav(root, state) {
  const host = q(root, 'nav-body');
  if (!host) return;
  const body = el('div', { className: 'rss-nav__scroll' });

  const library = el('div', { className: 'rss-nav__group', role: 'group', 'aria-label': t('library') });
  library.append(
    scopeButton(state, { kind: 'all', value: null }, t('scopeAll'), {
      leading: icon('inbox'),
      count: state.stats && state.stats.total_unread,
    }),
    scopeButton(state, { kind: 'starred', value: null }, t('scopeStarred'), {
      leading: icon('star'),
      count: state.stats && state.stats.total_starred,
    }),
  );
  body.appendChild(library);

  const all = feeds(state);
  const heading = el('div', { className: 'rss-nav__heading' });
  heading.appendChild(el('span', null, t('feeds')));
  if (state.canManage) {
    heading.appendChild(el('button', {
      type: 'button',
      className: 'rss-icon-btn is-small',
      'data-action': 'open-manage',
      'data-tab': 'sources',
      'data-intent': 'add',
      'aria-label': t('addSource'),
      title: t('addSource'),
    })).appendChild(icon('plus'));
  }
  body.appendChild(heading);

  if (!state.booted) {
    body.appendChild(navSkeleton());
  } else if (all.length === 0) {
    const empty = el('div', { className: 'rss-nav__empty' });
    empty.appendChild(el('p', null, state.canManage ? t('emptySourcesManage') : t('emptySources')));
    if (state.canManage) {
      empty.appendChild(el('button', {
        type: 'button',
        className: 'rss-btn is-primary is-small',
        'data-action': 'open-manage',
        'data-tab': 'sources',
        'data-intent': 'add',
      }, t('addFirstSource')));
    }
    body.appendChild(empty);
  } else {
    const { folders, loose } = groups(state);
    const tree = el('div', { className: 'rss-nav__group' });
    for (const folder of folders) {
      const open = !state.collapsed.includes(folder.name);
      const row = el('div', { className: 'rss-folder' });
      const twisty = el('button', {
        type: 'button',
        className: 'rss-folder__twisty',
        'data-action': 'toggle-folder',
        'data-value': folder.name,
        'aria-expanded': open ? 'true' : 'false',
        'aria-label': open ? t('collapseFolder', { name: folder.name }) : t('expandFolder', { name: folder.name }),
      });
      twisty.appendChild(icon('chevron'));
      row.append(twisty, scopeButton(state, { kind: 'category', value: folder.name }, folder.name, {
        count: sum(folder.sources),
      }));
      tree.appendChild(row);
      if (open) {
        for (const source of folder.sources) tree.appendChild(sourceRow(state, source, 1));
      }
    }
    if (loose.length && folders.length) {
      tree.appendChild(scopeButton(state, { kind: 'category', value: UNCATEGORIZED }, t('uncategorized'), {
        leading: icon('folder'),
        count: sum(loose),
      }));
    }
    for (const source of loose) tree.appendChild(sourceRow(state, source, folders.length ? 1 : 0));
    body.appendChild(tree);
  }
  host.replaceChildren(body);

  const stats = q(root, 'nav-stats');
  if (stats) {
    stats.textContent = state.stats
      ? t('stats', { unread: state.stats.total_unread ?? 0, sources: all.length })
      : '';
  }
}

function navSkeleton() {
  const box = el('div', { className: 'rss-nav__group', 'aria-hidden': 'true' });
  for (let index = 0; index < 5; index += 1) {
    const row = el('div', { className: 'rss-nav-item is-skeleton' });
    row.append(el('span', { className: 'rss-avatar rss-shimmer' }), el('span', { className: 'rss-line rss-shimmer' }));
    box.appendChild(row);
  }
  return box;
}

function scopeTitle(state) {
  const { kind, value } = state.scope;
  if (kind === 'starred') return t('scopeStarred');
  if (kind === 'source') {
    const source = state.sources.find((item) => item.id === value);
    return (source && (source.name || source.url)) || t('articles');
  }
  if (kind === 'category') return value === UNCATEGORIZED ? t('uncategorized') : value;
  return t('scopeAll');
}

module.exports = { renderNav, scopeTitle, sourceAvatar, markBrokenIcon };
