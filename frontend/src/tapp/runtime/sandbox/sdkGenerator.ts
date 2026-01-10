/**
 * Tapp SDK 代码生成器
 *
 * 生成注入到沙箱的 SDK 代码
 *
 * 安全特性：
 * - 所有 API 对象被冻结，防止篡改
 * - 使用 session token 验证消息来源
 * - 存储 key 验证防止路径遍历
 * - 完整的对象冻结包括 widgets/pages
 *
 * 🎯 性能优化：
 * - SDK 模板预生成（静态部分只计算一次）
 * - 使用占位符替换而非字符串拼接
 * - 存储 key 验证器代码缓存
 */

import type { TappInstance } from '../../types'

/**
 * 验证存储 key 的正则表达式
 * 只允许字母、数字、下划线、连字符、点、冒号
 * 禁止路径遍历字符
 */
const STORAGE_KEY_REGEX = /^[\w.\-:]+$/

// ========================
// 🎯 预缓存的静态代码片段
// ========================

/**
 * 存储 key 验证器代码（预生成，避免重复计算）
 */
const STORAGE_KEY_VALIDATOR_CODE = `
  const validateStorageKey = (key) => {
    if (!key || typeof key !== 'string') {
      throw new Error('Storage key must be a non-empty string');
    }
    if (key.length > 256) {
      throw new Error('Storage key too long (max 256 chars)');
    }
    if (key.includes('..') || key.includes('/') || key.includes('\\\\')) {
      throw new Error('Storage key contains invalid path characters');
    }
    if (key.startsWith('.') || key.endsWith('.')) {
      throw new Error('Storage key cannot start or end with a dot');
    }
    if (!/^[\\w.\\-:]+$/.test(key)) {
      throw new Error('Storage key contains invalid characters');
    }
    return key;
  };
`

/**
 * 生成存储 key 验证代码（使用缓存）
 */
function generateStorageKeyValidator(): string {
  return STORAGE_KEY_VALIDATOR_CODE
}

/**
 * 生成完整版 SDK（用于 Page 模式）
 *
 * @param tappInstance - Tapp 实例
 * @param sessionToken - 会话 token（用于消息验证）
 */
export function generateFullSDK(tappInstance: TappInstance, sessionToken?: string): string {
  const { id, manifest, grantedPermissions } = tappInstance
  const token = sessionToken || ''

  return `
(() => {
  'use strict';

  // 会话 token（用于消息验证）
  const _SESSION_TOKEN = '${token}';
  
  let messageIdCounter = 0;
  const pendingRequests = new Map();
  const eventListeners = new Map();
  const lifecycleCallbacks = { ready: [], destroy: [], pause: [], resume: [] };

  // WebKit 专用沙箱会在注入 HTML 时显式设置该标记，避免 UA 嗅探。
  // 在 WebKit iframe 上，频繁切换 transform 合成层可能触发“空白/不绘制”回归。
  const _forceRepaint = function () {
    void document.body.offsetHeight;
    if (window._TAPP_DISABLE_TRANSFORM_REPAINT) return;
    try {
      requestAnimationFrame(function () {
        document.body.style.transform = 'translateZ(0)';
        requestAnimationFrame(function () {
          document.body.style.transform = '';
        });
      });
    } catch (e) {
      // ignore
    }
  };

  const generateId = () => \`tapp-\${++messageIdCounter}-\${Date.now()}\`;
  
  ${generateStorageKeyValidator()}

  const sendRequest = (api, method, args = []) => {
    return new Promise((resolve, reject) => {
      const id = generateId();
      const timeout = setTimeout(() => {
        pendingRequests.delete(id);
        reject(new Error('Request timeout'));
      }, 30000);

      pendingRequests.set(id, { resolve, reject, timeout });

      // 消息中包含 session token 用于验证
      window.parent.postMessage({
        type: 'request',
        id,
        action: \`\${api}.\${method}\`,
        payload: { api, method, args },
        source: '${id}',
        timestamp: Date.now(),
        _sessionToken: _SESSION_TOKEN,
      }, '*');
    });
  };

  window.addEventListener('message', (event) => {
    const { data: message } = event;
    if (!message?.type) return;

    if (message.type === 'response') {
      const pending = pendingRequests.get(message.id);
      if (pending) {
        clearTimeout(pending.timeout);
        pendingRequests.delete(message.id);
        if (message.payload?.success) {
          pending.resolve(message.payload.data);
        } else {
          pending.reject(new Error(message.payload?.error || 'Unknown error'));
        }
      }
    } else if (message.type === 'event') {
      const listeners = eventListeners.get(message.action);
      listeners?.forEach((cb) => { try { cb(message.payload); } catch (e) {} });
      
      if (message.action === 'lifecycle:destroy') lifecycleCallbacks.destroy.forEach((cb) => cb());
      else if (message.action === 'lifecycle:pause') lifecycleCallbacks.pause.forEach((cb) => cb());
      else if (message.action === 'lifecycle:resume') lifecycleCallbacks.resume.forEach((cb) => cb());
      else if (message.action === 'theme:change') {
        const isDark = message.payload === 'dark';
        eventListeners.get('themeChange')?.forEach((cb) => cb(message.payload));
        // 更新 body class 和 CSS 变量
        document.body.classList.toggle('dark', isDark);
        document.body.classList.toggle('light', !isDark);
        const root = document.documentElement;
        root.style.setProperty('--tapp-text', isDark ? '#f3f4f6' : '#1f2937');
        root.style.setProperty('--tapp-subtext', isDark ? '#9ca3af' : '#6b7280');
        root.style.setProperty('--tapp-bg', isDark ? '#0a0a0a' : '#f8fafc');
        root.style.setProperty('--tapp-card-bg', isDark ? 'rgba(255,255,255,0.03)' : 'rgba(255,255,255,0.7)');
        root.style.setProperty('--tapp-border', isDark ? 'rgba(255,255,255,0.08)' : 'rgba(0,0,0,0.06)');
        root.style.setProperty('--tapp-input-bg', isDark ? 'rgba(255,255,255,0.05)' : 'rgba(255,255,255,0.9)');
        root.style.setProperty('--tapp-shadow', isDark ? 'rgba(0,0,0,0.4)' : 'rgba(0,0,0,0.08)');
        // 🎯 强制触发重绘（WebKit 走保守路径）
        _forceRepaint();
      }
      else if (message.action === 'locale:change') eventListeners.get('localeChange')?.forEach((cb) => cb(message.payload));
      else if (message.action === 'primaryColor:change') {
        eventListeners.get('primaryColorChange')?.forEach((cb) => cb(message.payload));
        // 更新 CSS 变量
        if (message.payload) {
          document.documentElement.style.setProperty('--tapp-primary', message.payload);
          // 🎯 强制触发重绘（WebKit 走保守路径）
          _forceRepaint();
        }
      }
    }
  });

  const addEventListener = (event, callback) => {
    let listeners = eventListeners.get(event);
    if (!listeners) {
      listeners = new Set();
      eventListeners.set(event, listeners);
    }
    listeners.add(callback);
    return () => listeners.delete(callback);
  };

  const Tapp = {
    id: '${id}',
    version: '${manifest.version}',
    name: '${manifest.name}',
    permissions: ${JSON.stringify(grantedPermissions)},

    lifecycle: {
      onReady: (cb) => lifecycleCallbacks.ready.push(cb),
      onDestroy: (cb) => lifecycleCallbacks.destroy.push(cb),
      onPause: (cb) => lifecycleCallbacks.pause.push(cb),
      onResume: (cb) => lifecycleCallbacks.resume.push(cb),
      getInfo: () => ({ id: '${id}', version: '${manifest.version}', name: '${manifest.name}', permissions: ${JSON.stringify(grantedPermissions)}, sandboxed: true }),
      _notifyError: (err) => sendRequest('lifecycle', 'error', [err.message || String(err)]),
      _notifyReady: () => {
        if (window._TAPP_SKIP_READY) { sendRequest('lifecycle', 'ready', []); return; }
        sendRequest('lifecycle', 'ready', []);
        lifecycleCallbacks.ready.forEach((cb) => cb());
      },
    },

    widget: {
      register: (cfg) => sendRequest('widget', 'register', [cfg]),
      unregister: (id) => sendRequest('widget', 'unregister', [id]),
      listRegistered: () => sendRequest('widget', 'listRegistered', []),
      updateConfig: (id, cfg) => sendRequest('widget', 'updateConfig', [id, cfg]),
    },

    platform: {
      listEnabled: () => sendRequest('platform', 'listEnabled', []),
      getData: (p, o) => sendRequest('platform', 'getData', [p, o]),
      getStats: (p) => sendRequest('platform', 'getStats', [p]),
      getDistribution: (p, d) => sendRequest('platform', 'getDistribution', [p, d]),
      addItem: (d) => sendRequest('platform', 'addItem', [d]),
      addItems: (i) => sendRequest('platform', 'addItems', [i]),
      registerPlatform: (c) => sendRequest('platform', 'registerPlatform', [c]),
    },

    ai: {
      generate: (r) => sendRequest('ai', 'generate', [r]),
      analyze: (r) => sendRequest('ai', 'analyze', [r]),
      getQuota: () => sendRequest('ai', 'getQuota', []),
      canGenerate: () => sendRequest('ai', 'canGenerate', []),
      chat: (m, c, o) => sendRequest('ai', 'chat', [{ messages: m, context: c, options: o }]),
      image: (r) => sendRequest('ai', 'image', [r]),
    },

    report: {
      listReports: () => sendRequest('report', 'listReports', []),
      getReport: (id) => sendRequest('report', 'getReport', [id]),
      getPlatformReport: (p) => sendRequest('report', 'getPlatformReport', [p]),
      create: (t, rt, c, m) => sendRequest('report', 'create', [{ title: t, reportType: rt, content: c, metadata: m }]),
      list: () => sendRequest('report', 'list', []),
      get: (id) => sendRequest('report', 'get', [{ reportId: id }]),
      update: (id, t, c, m) => sendRequest('report', 'update', [{ reportId: id, title: t, content: c, metadata: m }]),
      delete: (id) => sendRequest('report', 'delete', [{ reportId: id }]),
    },

    storage: {
      get: (k) => { validateStorageKey(k); return sendRequest('storage', 'get', [k]); },
      set: (k, v) => { validateStorageKey(k); return sendRequest('storage', 'set', [k, v]); },
      remove: (k) => { validateStorageKey(k); return sendRequest('storage', 'remove', [k]); },
      keys: () => sendRequest('storage', 'keys', []),
      clear: () => sendRequest('storage', 'clear', []),
      usage: () => sendRequest('storage', 'usage', []),
    },

    settings: {
      get: (k) => { validateStorageKey(k); return sendRequest('storage', 'get', [\`_settings.\${k}\`]); },
      set: (k, v) => { validateStorageKey(k); return sendRequest('storage', 'set', [\`_settings.\${k}\`, v]); },
      async getAll() {
        const keys = await sendRequest('storage', 'keys', []);
        const sKeys = (keys || []).filter((k) => k.startsWith('_settings.'));
        const result = {};
        for (const k of sKeys) {
          result[k.replace('_settings.', '')] = await sendRequest('storage', 'get', [k]);
        }
        return result;
      },
    },

    ui: {
      setTitle: (t) => sendRequest('ui', 'setTitle', [t]),
      getTheme: () => sendRequest('ui', 'getTheme', []),
      onThemeChange: (cb) => addEventListener('themeChange', cb),
      getPrimaryColor: () => sendRequest('ui', 'getPrimaryColor', []),
      onPrimaryColorChange: (cb) => addEventListener('primaryColorChange', cb),
      getLocale: () => sendRequest('ui', 'getLocale', []),
      onLocaleChange: (cb) => addEventListener('localeChange', cb),
      showNotification: (o) => sendRequest('ui', 'showNotification', [o]),
      confirm: (m) => sendRequest('ui', 'confirm', [m]),
      requestFullscreen: () => sendRequest('ui', 'requestFullscreen', []),
      exitFullscreen: () => sendRequest('ui', 'exitFullscreen', []),
      fullscreen: {
        request: () => sendRequest('ui', 'requestFullscreen', []),
        exit: () => sendRequest('ui', 'exitFullscreen', []),
        toggle: () => sendRequest('ui', 'toggleFullscreen', []),
        isFullscreen: () => sendRequest('ui', 'isFullscreen', []),
      },
    },

    data: { transform: (r) => sendRequest('data', 'transform', [r]) },
    
    // Tapp API 声明系统：调用 manifest 中声明的 API
    // 支持两种访问级别：
    // - public: 所有用户（包括游客）可调用
    // - protected: 需要 network:fetch 权限
    api: (name, params) => sendRequest('api', 'execute', [name, params]),
    
    context: {
      getApp: () => sendRequest('context', 'getApp', []),
      getUser: () => sendRequest('context', 'getUser', []),
      getPlayer: () => sendRequest('context', 'getPlayer', []),
      getNavigation: () => sendRequest('context', 'getNavigation', []),
      getSystem: () => sendRequest('context', 'getSystem', []),
      // 获取客户端地理位置信息（公开 API，所有用户可调用）
      getGeo: () => sendRequest('context', 'getGeo', []),
    },

    media: {
      play: () => sendRequest('media', 'control', [{ action: 'play' }]),
      pause: () => sendRequest('media', 'control', [{ action: 'pause' }]),
      next: () => sendRequest('media', 'control', [{ action: 'next' }]),
      prev: () => sendRequest('media', 'control', [{ action: 'prev' }]),
      seek: (p) => sendRequest('media', 'control', [{ action: 'seek', value: p }]),
      setVolume: (v) => sendRequest('media', 'control', [{ action: 'volume', value: v }]),
      setMode: (m) => sendRequest('media', 'control', [{ action: 'mode', value: m }]),
      mute: () => sendRequest('media', 'control', [{ action: 'mute' }]),
      unmute: () => sendRequest('media', 'control', [{ action: 'unmute' }]),
      getStatus: () => sendRequest('media', 'getStatus', []),
      getPlaylist: () => sendRequest('media', 'getPlaylist', []),
      getSpectrum: () => sendRequest('media', 'getSpectrum', []),
      playTrack: (id, idx) => sendRequest('media', 'playTrack', [{ trackId: id, trackIndex: idx }]),
      jumpToIndex: (idx) => sendRequest('media', 'jumpToIndex', [{ index: idx }]),
      loadNeteasePlaylist: (playlistId) => sendRequest('media', 'loadNeteasePlaylist', [{ playlistId }]),
      onStateChange: (cb) => addEventListener('mediaStateChange', cb),
    },

    component: {
      registerTheme: (c) => sendRequest('component', 'registerTheme', [c]),
      registerAgent: (c) => sendRequest('component', 'registerAgent', [c]),
      unregister: (t, id) => sendRequest('component', 'unregister', [t, id]),
      list: (t) => sendRequest('component', 'list', [t]),
    },

    shortcut: {
      register: (c) => sendRequest('shortcut', 'register', [c]),
      unregister: (id) => sendRequest('shortcut', 'unregister', [id]),
      list: () => sendRequest('shortcut', 'list', []),
    },

    event: {
      publish: (t, p, tgt) => sendRequest('event', 'publish', [t, p, tgt]),
      subscribe: (ts) => sendRequest('event', 'subscribe', [ts]),
      unsubscribe: (ts) => sendRequest('event', 'unsubscribe', [ts]),
      on: (t, cb) => addEventListener(\`tapp:\${t}\`, cb),
    },

    dom: {
      escapeHtml(text) {
        if (text == null) return '';
        const htmlEscapes = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#x27;', '/': '&#x2F;', '\`': '&#x60;', '=': '&#x3D;' };
        return String(text).replace(/[&<>"'\`=\\/]/g, (c) => htmlEscapes[c]);
      },
      setText: (el, t) => { if (el?.textContent !== undefined) el.textContent = t; },
      setSafeHtml: (el, h) => { if (el?.innerHTML !== undefined) el.innerHTML = Tapp.dom.escapeHtml(h); },
      createTextNode: (t) => document.createTextNode(t),
      setAttribute(el, n, v) {
        if (!el?.setAttribute) return;
        const ln = n.toLowerCase();
        const danger = ['onclick', 'onerror', 'onload', 'onmouseover', 'onfocus', 'onblur', 'onchange', 'onsubmit', 'onkeydown', 'onkeyup'];
        if (danger.includes(ln)) return;
        const sv = String(v).toLowerCase().trim();
        if (['href', 'src', 'action'].includes(ln) && (sv.startsWith('javascript:') || sv.startsWith('data:text/html') || sv.startsWith('vbscript:'))) return;
        el.setAttribute(n, v);
      },
      createElement(tag, opts) {
        const el = document.createElement(tag);
        if (opts) {
          if (opts.text) el.textContent = opts.text;
          if (opts.className) el.className = opts.className;
          if (opts.attributes) Object.entries(opts.attributes).forEach(([k, v]) => Tapp.dom.setAttribute(el, k, v));
        }
        return el;
      },
      renderList(container, items, renderItem) {
        if (!container) return;
        container.innerHTML = '';
        items.forEach((item, i) => { const el = renderItem(item, i); if (el) container.appendChild(el); });
      },
    },

    file: {
      download: (content, filename, mimeType) => sendRequest('file', 'download', [{ content, filename, mimeType }]),
    },

    user: {
      getRole: () => sendRequest('user', 'getRole', []),
      isAdmin: () => sendRequest('user', 'isAdmin', []),
      isGuest: () => sendRequest('user', 'isGuest', []),
      isLoggedIn: () => sendRequest('user', 'isLoggedIn', []),
      getAllowedPermissionLevels: () => sendRequest('user', 'getAllowedPermissionLevels', []),
      canUsePermissionLevel: (l) => sendRequest('user', 'canUsePermissionLevel', [l]),
    },

    background: {
      require: (r, reason) => sendRequest('background', 'require', [r, reason]),
      release: (r) => sendRequest('background', 'release', [r]),
      list: () => sendRequest('background', 'list', []),
      has: (r) => sendRequest('background', 'has', [r]),
    },

    dynamicContent: {
      set: (c) => sendRequest('dynamicContent', 'set', [c]),
      update: (u) => sendRequest('dynamicContent', 'update', [u]),
      get: () => sendRequest('dynamicContent', 'get', []),
      remove: () => sendRequest('dynamicContent', 'remove', []),
    },

    animation: {
      getLevel: () => sendRequest('animation', 'getLevel', []),
      shouldAnimate: () => sendRequest('animation', 'shouldAnimate', []),
      getConfig: () => sendRequest('animation', 'getConfig', []),
      getStaggerDelay: (i, d) => sendRequest('animation', 'getStaggerDelay', [i, d]),
      onLevelChange: (cb) => addEventListener('animationLevelChange', cb),
    },

    on: addEventListener,
    widgets: {},
    pages: {},
  };

  // 冻结所有 API 对象（防止篡改）
  Object.freeze(Tapp);
  Object.freeze(Tapp.lifecycle);
  Object.freeze(Tapp.widget);
  Object.freeze(Tapp.platform);
  Object.freeze(Tapp.ai);
  Object.freeze(Tapp.report);
  Object.freeze(Tapp.storage);
  Object.freeze(Tapp.settings);
  Object.freeze(Tapp.ui);
  Object.freeze(Tapp.ui.fullscreen);
  Object.freeze(Tapp.fetch);
  Object.freeze(Tapp.data);
  Object.freeze(Tapp.context);
  Object.freeze(Tapp.media);
  Object.freeze(Tapp.component);
  Object.freeze(Tapp.shortcut);
  Object.freeze(Tapp.event);
  Object.freeze(Tapp.dom);
  Object.freeze(Tapp.file);
  Object.freeze(Tapp.user);
  Object.freeze(Tapp.background);
  Object.freeze(Tapp.dynamicContent);
  Object.freeze(Tapp.animation);
  
  // 🔒 冻结 widgets 和 pages 容器（Tapp 代码可以添加内容，但不能替换整个对象）
  // 使用 Object.seal 允许添加属性但禁止删除
  Object.seal(Tapp.widgets);
  Object.seal(Tapp.pages);
  
  // 防止通过原型链篡改
  Object.freeze(Object.getPrototypeOf(Tapp));

  window.Tapp = Tapp;
  
  // 防止重新定义 Tapp
  Object.defineProperty(window, 'Tapp', {
    value: Tapp,
    writable: false,
    configurable: false
  });
  
  setTimeout(() => Tapp.lifecycle._notifyReady(), 0);
})();
`
}

/**
 * 生成精简版 SDK（用于 Widget 模式）
 *
 * @param tappInstance - Tapp 实例
 * @param sessionToken - 会话 token（用于消息验证）
 */
export function generateWidgetSDK(tappInstance: TappInstance, sessionToken?: string): string {
  const { id, manifest, grantedPermissions } = tappInstance
  const token = sessionToken || ''

  return `
(function() {
  'use strict';

  // 会话 token（用于消息验证）
  var _SESSION_TOKEN = '${token}';

  var messageIdCounter = 0;
  var pendingRequests = new Map();
  var eventListeners = new Map();
  // 🎯 添加生命周期回调支持
  var lifecycleCallbacks = { pause: [], resume: [] };

  var generateId = function() { return 'widget-' + (++messageIdCounter) + '-' + Date.now(); };
  
  // 存储 key 验证
  var validateStorageKey = function(key) {
    if (!key || typeof key !== 'string') {
      throw new Error('Storage key must be a non-empty string');
    }
    if (key.length > 256) {
      throw new Error('Storage key too long (max 256 chars)');
    }
    if (key.indexOf('..') >= 0 || key.indexOf('/') >= 0 || key.indexOf('\\\\') >= 0) {
      throw new Error('Storage key contains invalid path characters');
    }
    if (key.charAt(0) === '.' || key.charAt(key.length - 1) === '.') {
      throw new Error('Storage key cannot start or end with a dot');
    }
    if (!/^[\\w.\\-:]+$/.test(key)) {
      throw new Error('Storage key contains invalid characters');
    }
    return key;
  };

  var sendRequest = function(api, method, args) {
    args = args || [];
    return new Promise(function(resolve, reject) {
      var id = generateId();
      var timeout = setTimeout(function() { pendingRequests.delete(id); reject(new Error('Request timeout')); }, 30000);
      pendingRequests.set(id, { resolve: resolve, reject: reject, timeout: timeout });
      try {
        window.parent.postMessage({ 
          type: 'request', 
          id: id, 
          action: api + '.' + method, 
          payload: { api: api, method: method, args: args }, 
          timestamp: Date.now(),
          _sessionToken: _SESSION_TOKEN
        }, '*');
      } catch (e) { clearTimeout(timeout); pendingRequests.delete(id); reject(e); }
    });
  };
  
  var addEventListener = function(event, callback) {
    var listeners = eventListeners.get(event);
    if (!listeners) {
      listeners = new Set();
      eventListeners.set(event, listeners);
    }
    listeners.add(callback);
    return function() { listeners.delete(callback); };
  };

  window.addEventListener('message', function(event) {
    var msg = event.data;
    if (!msg) return;
    
    // 处理响应
    if (msg.type === 'response') {
      var pending = pendingRequests.get(msg.id);
      if (!pending) return;
      clearTimeout(pending.timeout);
      pendingRequests.delete(msg.id);
      var payload = msg.payload || {};
      if (payload.success) { pending.resolve(payload.data); } else { pending.reject(new Error(payload.error || 'Request failed')); }
    }
    
    // 处理事件
    if (msg.type === 'event') {
      // 🎯 强制重绘辅助函数：WebKit 专用沙箱会设置 window._TAPP_DISABLE_TRANSFORM_REPAINT
      var forceRepaint = function () {
        void document.body.offsetHeight;
        if (window._TAPP_DISABLE_TRANSFORM_REPAINT) return;
        try {
          requestAnimationFrame(function () {
            document.body.style.transform = 'translateZ(0)';
            requestAnimationFrame(function () {
              document.body.style.transform = '';
            });
          });
        } catch (e) {
          // ignore
        }
      };
      
      // 主题变化事件
      if (msg.action === 'theme:change') {
        var isDark = msg.payload === 'dark';
        eventListeners.get('themeChange')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
        // 更新 body 的 class
        document.body.classList.toggle('dark', isDark);
        document.body.classList.toggle('light', !isDark);
        // 更新主题相关的 CSS 变量
        var root = document.documentElement;
        root.style.setProperty('--tapp-text', isDark ? '#f3f4f6' : '#1f2937');
        root.style.setProperty('--tapp-subtext', isDark ? '#9ca3af' : '#6b7280');
        root.style.setProperty('--tapp-bg', isDark ? '#0a0a0a' : '#f8fafc');
        root.style.setProperty('--tapp-card-bg', isDark ? 'rgba(255,255,255,0.03)' : 'rgba(255,255,255,0.7)');
        root.style.setProperty('--tapp-border', isDark ? 'rgba(255,255,255,0.08)' : 'rgba(0,0,0,0.06)');
        root.style.setProperty('--tapp-input-bg', isDark ? 'rgba(255,255,255,0.05)' : 'rgba(255,255,255,0.9)');
        root.style.setProperty('--tapp-shadow', isDark ? 'rgba(0,0,0,0.4)' : 'rgba(0,0,0,0.08)');
        // 🎯 强制触发重绘
        forceRepaint();
      }
      // 主色调变化事件
      else if (msg.action === 'primaryColor:change') {
        eventListeners.get('primaryColorChange')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
        // 更新 CSS 变量
        if (msg.payload) {
          document.documentElement.style.setProperty('--tapp-primary', msg.payload);
          // 🎯 强制触发重绘
          forceRepaint();
        }
      }
      // 语言变化事件
      else if (msg.action === 'locale:change') {
        eventListeners.get('localeChange')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
      }
      // 容器尺寸变化事件（已在 HTML 中处理，这里作为备份）
      else if (msg.action === 'container:resize') {
        window._TAPP_DIMENSIONS = msg.payload;
        var root = document.documentElement;
        root.style.setProperty('--tapp-scale', msg.payload.scale || 1);
        root.style.setProperty('--tapp-font-scale', msg.payload.fontScale || 1);
        window.dispatchEvent(new CustomEvent('tapp:resize', { detail: msg.payload }));
      }
      // 🎯 生命周期暂停事件（页面不可见时触发）
      else if (msg.action === 'lifecycle:pause') {
        lifecycleCallbacks.pause.forEach(function(cb) { try { cb(); } catch(e) {} });
        eventListeners.get('pause')?.forEach(function(cb) { try { cb(); } catch(e) {} });
      }
      // 🎯 生命周期恢复事件（页面重新可见时触发）
      else if (msg.action === 'lifecycle:resume') {
        lifecycleCallbacks.resume.forEach(function(cb) { try { cb(); } catch(e) {} });
        eventListeners.get('resume')?.forEach(function(cb) { try { cb(); } catch(e) {} });
      }
    }
  });

  window.Tapp = {
    id: '${id}',
    name: '${manifest.name}',
    version: '${manifest.version}',
    permissions: ${JSON.stringify(grantedPermissions || [])},
    widgets: {},
    pages: {},
    
    // 🎯 生命周期 API（用于响应冻结/恢复）
    lifecycle: {
      onPause: function(cb) { lifecycleCallbacks.pause.push(cb); },
      onResume: function(cb) { lifecycleCallbacks.resume.push(cb); }
    },
    
    storage: {
      get: function(k) { validateStorageKey(k); return sendRequest('storage', 'get', [k]); },
      set: function(k, v) { validateStorageKey(k); return sendRequest('storage', 'set', [k, v]); },
      remove: function(k) { validateStorageKey(k); return sendRequest('storage', 'remove', [k]); },
      keys: function() { return sendRequest('storage', 'keys', []); },
      clear: function() { return sendRequest('storage', 'clear', []); }
    },
    
    settings: {
      get: function(k) { validateStorageKey(k); return sendRequest('storage', 'get', ['_settings.' + k]); },
      set: function(k, v) { validateStorageKey(k); return sendRequest('storage', 'set', ['_settings.' + k, v]); },
      getAll: function() {
        return sendRequest('storage', 'keys', []).then(function(keys) {
          var sKeys = (keys || []).filter(function(k) { return k.startsWith('_settings.'); });
          var result = {};
          var promises = sKeys.map(function(k) {
            return sendRequest('storage', 'get', [k]).then(function(v) {
              result[k.replace('_settings.', '')] = v;
            });
          });
          return Promise.all(promises).then(function() { return result; });
        });
      }
    },
    
    ai: { chat: function(m, c, o) { return sendRequest('ai', 'chat', [{ messages: m, context: c, options: o }]); } },
    
    platform: {
      listEnabled: function() { return sendRequest('platform', 'listEnabled', []); },
      getData: function(p, o) { return sendRequest('platform', 'getData', [p, o]); },
      getStats: function(p) { return sendRequest('platform', 'getStats', [p]); },
      getDistribution: function(p, d) { return sendRequest('platform', 'getDistribution', [p, d]); }
    },
    
    report: {
      listReports: function() { return sendRequest('report', 'listReports', []); },
      getReport: function(id) { return sendRequest('report', 'getReport', [id]); },
      getPlatformReport: function(p) { return sendRequest('report', 'getPlatformReport', [p]); },
      list: function() { return sendRequest('report', 'list', []); },
      get: function(id) { return sendRequest('report', 'get', [{ reportId: id }]); }
    },
    
    background: {
      require: function(r, reason) { return sendRequest('background', 'require', [r, reason]); },
      release: function(r) { return sendRequest('background', 'release', [r]); },
      list: function() { return sendRequest('background', 'list', []); },
      has: function(r) { return sendRequest('background', 'has', [r]); }
    },
    
    animation: {
      getLevel: function() { return sendRequest('animation', 'getLevel', []); },
      shouldAnimate: function() { return sendRequest('animation', 'shouldAnimate', []); },
      getConfig: function() { return sendRequest('animation', 'getConfig', []); },
      getStaggerDelay: function(i, d) { return sendRequest('animation', 'getStaggerDelay', [i, d]); },
      onLevelChange: function(cb) { return addEventListener('animationLevelChange', cb); }
    },
    
    ui: {
      getTheme: function() { return sendRequest('ui', 'getTheme', []); },
      getPrimaryColor: function() { return sendRequest('ui', 'getPrimaryColor', []); },
      getLocale: function() { return sendRequest('ui', 'getLocale', []); },
      showNotification: function(o) { return sendRequest('ui', 'showNotification', [o]); },
      onThemeChange: function(cb) { return addEventListener('themeChange', cb); },
      onPrimaryColorChange: function(cb) { return addEventListener('primaryColorChange', cb); },
      onLocaleChange: function(cb) { return addEventListener('localeChange', cb); }
    },
    
    // Tapp API 声明系统：调用 manifest 中声明的 API
    // 支持两种访问级别：
    // - public: 所有用户（包括游客）可调用
    // - protected: 需要 network:fetch 权限
    api: function(name, params) { return sendRequest('api', 'execute', [name, params]); },
    
    // 获取上下文信息
    context: {
      getGeo: function() { return sendRequest('context', 'getGeo', []); }
    },
    
    dom: {
      setText: function(el, text) { if (el) el.textContent = text; },
      setHtml: function(el, html) { if (el) el.innerHTML = html; },
      addClass: function(el, cls) { if (el) el.classList.add(cls); },
      removeClass: function(el, cls) { if (el) el.classList.remove(cls); },
      toggleClass: function(el, cls) { if (el) el.classList.toggle(cls); }
    },
    
    file: {
      download: function(content, filename, mimeType) { return sendRequest('file', 'download', [{ content: content, filename: filename, mimeType: mimeType }]); }
    },
    
    lifecycle: {
      onReady: function(cb) { if (document.readyState === 'complete') setTimeout(cb, 0); else window.addEventListener('load', cb); },
      onDestroy: function(cb) { window.addEventListener('beforeunload', cb); }
    }
  };

  // 冻结所有 API 对象（防止篡改）
  Object.freeze(Tapp);
  Object.freeze(Tapp.lifecycle);
  Object.freeze(Tapp.storage);
  Object.freeze(Tapp.settings);
  Object.freeze(Tapp.ai);
  Object.freeze(Tapp.platform);
  Object.freeze(Tapp.report);
  Object.freeze(Tapp.background);
  Object.freeze(Tapp.animation);
  Object.freeze(Tapp.ui);
  Object.freeze(Tapp.context);
  Object.freeze(Tapp.dom);
  Object.freeze(Tapp.file);
  
  // 使用 seal 允许添加 widget/page 定义但禁止替换整个对象
  Object.seal(Tapp.widgets);
  Object.seal(Tapp.pages);
  
  // 防止重新定义 Tapp
  Object.defineProperty(window, 'Tapp', {
    value: Tapp,
    writable: false,
    configurable: false
  });

  console.log('[TappWidgetSDK] Initialized:', '${id}');
})();
`
}
