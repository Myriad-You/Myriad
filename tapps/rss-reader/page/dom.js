// 只用沙箱里稳妥的那部分 DOM：createElement、属性、append、事件委托。

const SVG_NS = 'http://www.w3.org/2000/svg';

// 24×24 描线图标。只放阅读器真用到的几个。
const ICONS = {
  menu: 'M4 7h16M4 12h16M4 17h16',
  back: 'M15 6l-6 6 6 6',
  up: 'M6 15l6-6 6 6',
  down: 'M6 9l6 6 6-6',
  close: 'M6 6l12 12M18 6L6 18',
  search: 'M11 18a7 7 0 1 1 0-14 7 7 0 0 1 0 14zM20 20l-4-4',
  refresh: 'M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6',
  check: 'M5 12.5l4.5 4.5L19 7.5',
  checks: 'M2.5 12.5l4.5 4.5 9.5-9.5M11.5 16l1 1 9.5-9.5',
  circle: 'M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10z',
  star: 'M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z',
  external: 'M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5',
  type: 'M4 19l5-14 5 14M5.8 14h6.4M15 19l3-8 3 8M15.9 16.5h4.2',
  settings: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z',
  download: 'M12 4v11M7 10l5 5 5-5M5 20h14',
  upload: 'M12 20V9M7 14l5-5 5 5M5 4h14',
  plus: 'M12 5v14M5 12h14',
  inbox: 'M4 13l2.5-7h11l2.5 7v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1zM4 13h5l1 2h4l1-2h5',
  layers: 'M12 4l8 4-8 4-8-4zM4 12l8 4 8-4M4 16l8 4 8-4',
  folder: 'M4 7a1 1 0 0 1 1-1h4l2 2h8a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1z',
  chevron: 'M9 6l6 6-6 6',
  keyboard: 'M3 7a1 1 0 0 1 1-1h16a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1zM7 10h.01M11 10h.01M15 10h.01M7 14h10',
  alert: 'M12 9v4M12 17h.01M10.3 4.3L2.6 18a2 2 0 0 0 1.7 3h15.4a2 2 0 0 0 1.7-3L13.7 4.3a2 2 0 0 0-3.4 0z',
  clock: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7v5l3 2',
};

function t(key, variables) {
  return Tapp.i18n.t(key, variables || {});
}

function el(tag, attrs, text) {
  const node = document.createElement(tag);
  if (attrs) {
    for (const [key, value] of Object.entries(attrs)) {
      if (value == null || value === false) continue;
      if (key === 'className') node.className = value;
      else if (key === 'hidden') node.hidden = true;
      else node.setAttribute(key, value === true ? '' : String(value));
    }
  }
  if (text != null) node.textContent = text;
  return node;
}

function icon(name, extraClass) {
  const path = ICONS[name] || '';
  if (typeof document.createElementNS !== 'function') {
    return el('span', { className: 'rss-icon', 'aria-hidden': 'true' });
  }
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('class', extraClass ? `rss-icon ${extraClass}` : 'rss-icon');
  const drawn = document.createElementNS(SVG_NS, 'path');
  drawn.setAttribute('d', path);
  svg.appendChild(drawn);
  return svg;
}

/** 只有图标的按钮必须带名字：读屏读它，鼠标悬停也显示它。 */
function iconButton(action, iconName, label, attrs) {
  const button = el('button', Object.assign({
    type: 'button',
    className: 'rss-icon-btn',
    'data-action': action,
    'aria-label': label,
    title: label,
  }, attrs || {}));
  button.appendChild(icon(iconName));
  return button;
}

function button(action, label, attrs, iconName) {
  const node = el('button', Object.assign({
    type: 'button',
    className: 'rss-btn',
    'data-action': action,
  }, attrs || {}));
  if (iconName) node.appendChild(icon(iconName));
  node.appendChild(el('span', null, label));
  return node;
}

function toggleClass(node, name, on) {
  if (!node) return;
  if (node.classList && typeof node.classList.toggle === 'function' && node.classList.contains) {
    node.classList.toggle(name, Boolean(on));
    return;
  }
  const current = String(node.className || '').split(/\s+/).filter(Boolean);
  const next = current.filter((item) => item !== name);
  if (on) next.push(name);
  node.className = next.join(' ');
}

function q(root, role) {
  return root.querySelector(`[data-role="${role}"]`);
}

function setPressed(node, on) {
  if (node) node.setAttribute('aria-pressed', on ? 'true' : 'false');
}

function nextFrame(callback) {
  if (typeof requestAnimationFrame === 'function') requestAnimationFrame(callback);
  else setTimeout(callback, 0);
}

module.exports = { t, el, icon, iconButton, button, toggleClass, q, setPressed, nextFrame };
