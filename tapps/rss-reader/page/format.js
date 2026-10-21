const { t } = require('./dom.js');

function categoryTokens(value) {
  return String(value || '')
    .split(',')
    .map((part) => part.trim())
    .filter(Boolean);
}

function isFeed(source) {
  return Boolean(source) && source.source_type !== 'note' && source.source_type !== 'link';
}

function toDate(value) {
  if (value == null || value === '') return null;
  const numeric = typeof value === 'number' ? value : Number(value);
  const date = Number.isFinite(numeric)
    ? new Date(numeric > 1e12 ? numeric : numeric * 1000)
    : new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

function locale() {
  try {
    return Tapp.i18n.getLocale() || undefined;
  } catch {
    return undefined;
  }
}

/** 列表里用相对时间，两周以外换成日期；正文页用完整时间。 */
function formatWhen(value, style) {
  const date = toDate(value);
  if (!date) return '';
  try {
    if (style === 'long') {
      return new Intl.DateTimeFormat(locale(), {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
        hour: 'numeric',
        minute: '2-digit',
      }).format(date);
    }
    const minutes = Math.round((Date.now() - date.getTime()) / 60000);
    if (Math.abs(minutes) < 1) return t('justNow');
    const rel = new Intl.RelativeTimeFormat(locale(), { numeric: 'auto', style: 'short' });
    if (Math.abs(minutes) < 60) return rel.format(-minutes, 'minute');
    const hours = Math.round(minutes / 60);
    if (Math.abs(hours) < 24) return rel.format(-hours, 'hour');
    const days = Math.round(hours / 24);
    if (Math.abs(days) < 14) return rel.format(-days, 'day');
    const sameYear = date.getFullYear() === new Date().getFullYear();
    return new Intl.DateTimeFormat(locale(), sameYear
      ? { month: 'short', day: 'numeric' }
      : { year: 'numeric', month: 'short', day: 'numeric' }).format(date);
  } catch {
    return '';
  }
}

/** 沙箱没有 media:remote，只认站内地址和小图 data URI。 */
function localMedia(url) {
  const value = String(url || '').trim();
  if (!value) return '';
  if (value.startsWith('/api/') || value.startsWith('/media/')) {
    if (value.includes('..') || value.includes('\\') || value.includes('//')) return '';
    return value.split('?')[0].split('#')[0];
  }
  if (/^data:image\/(png|jpeg|gif|webp);base64,/i.test(value)) return value;
  return '';
}

function monogram(name) {
  const text = String(name || '').trim();
  if (!text) return '·';
  return Array.from(text)[0].toUpperCase();
}

function plainText(value) {
  return String(value || '')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/\s+/g, ' ')
    .trim();
}

function excerptOf(item) {
  const text = plainText(item && item.summary);
  if (!text || text === (item && item.title)) return '';
  return text.slice(0, 220);
}

function readingMinutes(item) {
  const minutes = Number(item && item.reading_time);
  return Number.isFinite(minutes) && minutes > 0 ? Math.round(minutes) : 0;
}

function hostOf(url) {
  const match = /^https?:\/\/([^/?#]+)/i.exec(String(url || ''));
  return match ? match[1].replace(/^www\./, '') : '';
}

module.exports = {
  categoryTokens,
  isFeed,
  formatWhen,
  localMedia,
  monogram,
  plainText,
  excerptOf,
  readingMinutes,
  hostOf,
};
