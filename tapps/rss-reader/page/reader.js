const { t, el, icon, iconButton, q, setIcon, setPressed, toggleClass, nextFrame } = require('./dom.js');
const { formatWhen, readingMinutes, hostOf } = require('./format.js');
const { FONT_STEPS, shownItems, neighbour } = require('./model.js');
const { sanitizeContent } = require('./sanitize.js');
const { sourceAvatar } = require('./nav.js');
const { sourceLookup } = require('./list.js');

function buildReader(state) {
  const pane = el('article', { className: 'rss-reader', 'data-role': 'reader', 'aria-label': t('reading') });
  const head = el('header', { className: 'rss-pane-head rss-reader-head', 'data-role': 'reader-head' });
  const bar = el('div', { className: 'rss-pane-head__row', role: 'toolbar', 'aria-label': t('articleTools') });
  bar.append(
    iconButton('back', 'back', t('back'), { 'data-role': 'reader-back', className: 'rss-icon-btn rss-reader-back' }),
    iconButton('prev', 'up', t('previous'), { 'data-role': 'reader-prev', 'aria-keyshortcuts': 'K' }),
    iconButton('next', 'down', t('next'), { 'data-role': 'reader-next', 'aria-keyshortcuts': 'J' }),
    el('span', { className: 'rss-spacer' }),
    iconButton('toggle-type', 'type', t('typography'), { 'data-role': 'type-toggle', 'aria-expanded': 'false', 'aria-controls': 'rss-type' }),
    iconButton('toggle-focus', 'focus', t('focusMode'), { 'data-role': 'focus-toggle', 'aria-keyshortcuts': 'F', 'aria-pressed': 'false' }),
  );
  if (state.canWrite) {
    bar.append(
      iconButton('toggle-read', 'mailOpen', t('markUnread'), { 'data-role': 'read-toggle', 'aria-keyshortcuts': 'M' }),
      iconButton('toggle-star', 'star', t('star'), { 'data-role': 'star-toggle', 'aria-keyshortcuts': 'S' }),
    );
  }
  bar.append(iconButton('open-original', 'external', t('openOriginal'), { 'data-role': 'original', 'aria-keyshortcuts': 'O' }));
  head.append(bar, typePanel());
  // 进度条不放在栏头里：往下读时栏头会收起来，进度条要留在顶上。
  const progress = el('div', { className: 'rss-progress', 'aria-hidden': 'true' });
  progress.appendChild(el('span', { 'data-role': 'progress' }));

  const scroll = el('div', { className: 'rss-reader__scroll', 'data-role': 'reader-scroll' });
  scroll.appendChild(el('div', { 'data-role': 'reader-body' }));
  pane.append(head, progress, scroll);
  return pane;
}

function typePanel() {
  const panel = el('div', { className: 'rss-popover', id: 'rss-type', 'data-role': 'type-panel', role: 'group', 'aria-label': t('typography'), hidden: true });
  const size = el('div', { className: 'rss-popover__row' });
  const stepper = el('div', { className: 'rss-stepper' });
  size.append(el('span', { className: 'rss-popover__label' }, t('textSize')), stepper);
  stepper.append(
    el('button', { type: 'button', 'data-action': 'font-step', 'data-value': '-1', 'aria-label': t('smaller'), 'data-role': 'font-down' }, 'A−'),
    el('output', { 'data-role': 'font-value', 'aria-live': 'polite' }),
    el('button', { type: 'button', 'data-action': 'font-step', 'data-value': '1', 'aria-label': t('larger'), 'data-role': 'font-up' }, 'A+'),
  );
  const family = el('div', { className: 'rss-popover__row' });
  family.append(el('span', { className: 'rss-popover__label' }, t('typeface')));
  const segment = el('div', { className: 'rss-segment is-compact', role: 'group', 'aria-label': t('typeface') });
  segment.append(
    el('button', { type: 'button', className: 'is-serif', 'data-action': 'font-family', 'data-value': 'serif' }, t('serif')),
    el('button', { type: 'button', className: 'is-sans', 'data-action': 'font-family', 'data-value': 'sans' }, t('sans')),
  );
  family.appendChild(segment);
  const measure = el('div', { className: 'rss-popover__row' });
  measure.append(el('span', { className: 'rss-popover__label' }, t('lineWidth')));
  const widths = el('div', { className: 'rss-segment is-compact', role: 'group', 'aria-label': t('lineWidth') });
  for (const [value, label] of [['narrow', 'measureNarrow'], ['normal', 'measureNormal'], ['wide', 'measureWide']]) {
    widths.appendChild(el('button', { type: 'button', 'data-action': 'measure', 'data-value': value }, t(label)));
  }
  measure.appendChild(widths);
  panel.append(size, family, measure);
  return panel;
}

function paintTypography(root, state, ui) {
  const reader = q(root, 'reader');
  if (reader) {
    reader.setAttribute('data-font', state.fontFamily);
    reader.setAttribute('data-step', String(state.fontStep));
    reader.setAttribute('data-measure', state.measure);
  }
  const panel = q(root, 'type-panel');
  if (panel) panel.hidden = !ui.typeOpen;
  const toggle = q(root, 'type-toggle');
  if (toggle) {
    toggle.setAttribute('aria-expanded', ui.typeOpen ? 'true' : 'false');
    toggleClass(toggle, 'is-on', ui.typeOpen);
  }
  const value = q(root, 'font-value');
  if (value) value.textContent = `${Math.round(100 + state.fontStep * 10)}%`;
  const down = q(root, 'font-down');
  if (down) down.disabled = state.fontStep <= FONT_STEPS[0];
  const up = q(root, 'font-up');
  if (up) up.disabled = state.fontStep >= FONT_STEPS[FONT_STEPS.length - 1];
  for (const option of root.querySelectorAll('[data-action="font-family"]')) {
    setPressed(option, option.getAttribute('data-value') === state.fontFamily);
  }
  for (const option of root.querySelectorAll('[data-action="measure"]')) {
    setPressed(option, option.getAttribute('data-value') === state.measure);
  }
}

function currentItem(state) {
  if (state.article && state.article.id === state.selectedId) return state.article;
  return state.items.find((item) => item.id === state.selectedId) || null;
}

function paintReaderTools(root, state) {
  const article = currentItem(state);
  const items = shownItems(state, sourceLookup(state));
  const prev = q(root, 'reader-prev');
  if (prev) prev.disabled = !article || neighbour(items, state.selectedId, -1) == null;
  const next = q(root, 'reader-next');
  if (next) next.disabled = !article || (neighbour(items, state.selectedId, 1) == null && !state.nextCursor);
  const read = q(root, 'read-toggle');
  if (read) {
    read.disabled = !article;
    const isRead = Boolean(article && article.is_read);
    read.setAttribute('aria-label', isRead ? t('markUnread') : t('markRead'));
    read.setAttribute('title', `${isRead ? t('markUnread') : t('markRead')} (M)`);
    const unread = Boolean(article) && !isRead;
    setPressed(read, unread);
    toggleClass(read, 'is-on', unread);
    setIcon(read, isRead ? 'mailOpen' : 'mail');
  }
  const star = q(root, 'star-toggle');
  if (star) {
    star.disabled = !article;
    const on = Boolean(article && article.is_starred);
    star.setAttribute('aria-label', on ? t('unstar') : t('star'));
    star.setAttribute('title', `${on ? t('unstar') : t('star')} (S)`);
    setPressed(star, on);
    toggleClass(star, 'is-starred', on);
  }
  const original = q(root, 'original');
  if (original) {
    original.disabled = !(article && article.link);
    original.setAttribute('title', `${t('openOriginal')} (O)`);
  }
  for (const [role, key] of [['reader-prev', 'previous'], ['reader-next', 'next']]) {
    const node = q(root, role);
    if (node) node.setAttribute('title', `${t(key)} (${role === 'reader-prev' ? 'K' : 'J'})`);
  }
}

function renderReader(root, state) {
  const host = q(root, 'reader-body');
  if (!host) return;
  const article = currentItem(state);
  if (!article) {
    host.replaceChildren(readerEmpty(state));
    setProgress(root, 0);
    return;
  }
  const page = el('div', { className: 'rss-article' });
  const kicker = el('div', { className: 'rss-article__kicker' });
  const source = state.sources.find((entry) => entry.id === article.source_id);
  kicker.append(
    sourceAvatar(source || { name: article.source_name, icon: article.source_icon }),
    el('span', null, article.source_name || hostOf(article.link) || ''),
  );
  page.appendChild(kicker);
  page.appendChild(el('h1', { className: 'rss-article__title', tabindex: '-1', 'data-role': 'article-title' }, article.title || t('untitled')));
  const by = el('p', { className: 'rss-article__byline' });
  const parts = [
    article.author,
    formatWhen(article.published_at, 'long'),
    readingMinutes(article) ? t('minutesRead', { count: readingMinutes(article) }) : '',
  ].filter(Boolean);
  by.textContent = parts.join(' · ');
  if (parts.length) page.appendChild(by);

  const full = state.article && state.article.id === article.id;
  if (state.articleError && !full) {
    const box = el('div', { className: 'rss-empty is-error is-inline', role: 'alert' });
    box.append(el('strong', null, t('articleFailed')), el('p', null, state.articleError));
    box.appendChild(el('button', { type: 'button', className: 'rss-btn is-small', 'data-action': 'retry-article' }, t('retry')));
    page.appendChild(box);
  } else if (!full) {
    const skeleton = el('div', { className: 'rss-article__body', 'aria-busy': 'true' });
    for (const width of ['is-full', 'is-full', 'is-mid', 'is-full', 'is-short']) {
      skeleton.appendChild(el('span', { className: `rss-line rss-shimmer ${width}` }));
    }
    page.appendChild(skeleton);
  } else {
    const content = el('div', { className: 'rss-article__body' });
    content.appendChild(sanitizeContent(article.content || article.summary || ''));
    if (!content.childNodes || content.childNodes.length === 0) {
      content.appendChild(el('p', { className: 'rss-article__none' }, t('noContent')));
    }
    page.appendChild(content);
    page.appendChild(articleFoot(state, article));
  }
  host.replaceChildren(page);
  const scroll = q(root, 'reader-scroll');
  if (scroll && full) scroll.scrollTop = 0;
  showHead(root, true);
  setProgress(root, 0);
  nextFrame(() => trackProgress(root));
}

function articleFoot(state, article) {
  const foot = el('footer', { className: 'rss-article__foot' });
  if (article.link) {
    const original = el('div', { className: 'rss-article__source' });
    original.append(
      el('span', { className: 'rss-article__host' }, hostOf(article.link)),
      el('button', { type: 'button', className: 'rss-btn is-small', 'data-action': 'open-original' }, t('readOriginal')),
    );
    foot.appendChild(original);
  }
  const items = shownItems(state, sourceLookup(state));
  const nextId = neighbour(items, article.id, 1);
  const next = nextId != null ? items.find((item) => item.id === nextId) : null;
  if (next) {
    const card = el('button', { type: 'button', className: 'rss-next', 'data-action': 'next' });
    card.append(
      el('span', { className: 'rss-next__label' }, t('upNext')),
      el('span', { className: 'rss-next__title' }, next.title || t('untitled')),
      el('span', { className: 'rss-next__meta' }, [next.source_name, formatWhen(next.published_at)].filter(Boolean).join(' · ')),
    );
    foot.appendChild(card);
  } else if (!state.nextCursor) {
    foot.appendChild(el('p', { className: 'rss-end' }, t('endOfList')));
  }
  return foot;
}

function readerEmpty(state) {
  const box = el('div', { className: 'rss-empty is-reader' });
  const unread = state.stats && state.stats.total_unread;
  box.appendChild(icon('book', 'rss-empty__mark'));
  box.append(
    el('strong', null, unread === 0 ? t('allCaughtUp') : t('emptyArticle')),
    el('p', null, t('emptyArticleHint')),
  );
  const keys = el('p', { className: 'rss-keys' });
  keys.append(
    el('kbd', null, 'J'), el('kbd', null, 'K'), el('span', null, t('shortcutMove')),
    el('kbd', null, '?'), el('span', null, t('shortcutHelp')),
  );
  box.appendChild(keys);
  return box;
}

/**
 * 往下读时把栏头收起来，往回滚或回到顶部再放出来。状态只记在节点的类名上，
 * 换文章时直接放出来也不会和别处记的状态对不上。
 */
function showHead(root, visible) {
  toggleClass(q(root, 'reader'), 'is-head-hidden', !visible);
}

let lastTop = 0;

function followScroll(root, holdHead) {
  const scroll = q(root, 'reader-scroll');
  if (!scroll) return;
  const top = scroll.scrollTop || 0;
  const delta = top - lastTop;
  lastTop = top;
  if (holdHead || top < 48) showHead(root, true);
  else if (delta > 4) showHead(root, false);
  else if (delta < -4) showHead(root, true);
}

function setProgress(root, ratio) {
  const bar = q(root, 'progress');
  if (!bar) return;
  const value = Math.max(0, Math.min(1, ratio));
  if (bar.style) bar.style.transform = `scaleX(${value})`;
}

function trackProgress(root) {
  const scroll = q(root, 'reader-scroll');
  if (!scroll) return;
  const max = (scroll.scrollHeight || 0) - (scroll.clientHeight || 0);
  setProgress(root, max > 8 ? (scroll.scrollTop || 0) / max : 0);
}

module.exports = { buildReader, renderReader, paintReaderTools, paintTypography, trackProgress, followScroll, showHead, currentItem };
