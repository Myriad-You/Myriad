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
import type { SandboxCapabilityProfile } from './capabilityProfiles'
import { serializeSandboxScriptValue } from './security'

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

/** Page 与 Widget 共用的安全 DOM helper，避免两套 SDK 能力漂移。 */
const DOM_HELPERS_CODE = `{
      escapeHtml: function(text) {
        if (text == null) return '';
        var htmlEscapes = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#x27;' };
        return String(text).replace(/[&<>"']/g, function(c) { return htmlEscapes[c]; });
      },
      setText: function(el, text) {
        if (el && el.textContent !== undefined) el.textContent = text;
      },
      setSafeHtml: function(el, text) {
        if (el && el.textContent !== undefined) el.textContent = text == null ? '' : String(text);
      },
      createTextNode: function(text) { return document.createTextNode(text); },
      setAttribute: function(el, name, value) {
        if (!el || !el.setAttribute) return;
        var normalizedName = String(name).toLowerCase();
        var dangerous = ['onclick', 'onerror', 'onload', 'onmouseover', 'onfocus', 'onblur', 'onchange', 'onsubmit', 'onkeydown', 'onkeyup'];
        if (dangerous.indexOf(normalizedName) >= 0) return;
        var normalizedValue = String(value).toLowerCase().trim();
        if (['href', 'src', 'action'].indexOf(normalizedName) >= 0 &&
            (normalizedValue.indexOf('javascript:') === 0 || normalizedValue.indexOf('data:text/html') === 0 || normalizedValue.indexOf('vbscript:') === 0)) return;
        el.setAttribute(name, value);
      },
      createElement: function(tag, options) {
        var el = document.createElement(tag);
        if (options) {
          if (Object.prototype.hasOwnProperty.call(options, 'text')) el.textContent = options.text;
          if (options.className) el.className = options.className;
          if (options.attributes) Object.keys(options.attributes).forEach(function(key) {
            Tapp.dom.setAttribute(el, key, options.attributes[key]);
          });
        }
        return el;
      },
      renderList: function(container, items, renderItem) {
        if (!container) return;
        container.textContent = '';
        (items || []).forEach(function(item, index) {
          var el = renderItem(item, index);
          if (el) container.appendChild(el);
        });
      }
    }`

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
export function generateFullSDK(
  tappInstance: TappInstance,
  sessionToken?: string,
  profile: Extract<SandboxCapabilityProfile, 'page' | 'headless'> = 'page',
): string {
  const { id, manifest, grantedPermissions } = tappInstance
  const token = sessionToken || ''
  const idLiteral = serializeSandboxScriptValue(id)
  const nameLiteral = serializeSandboxScriptValue(manifest.name)
  const versionLiteral = serializeSandboxScriptValue(manifest.version)
  const tokenLiteral = serializeSandboxScriptValue(token)
  const permissionsLiteral = serializeSandboxScriptValue(grantedPermissions)
  const headlessLiteral = profile === 'headless' ? 'true' : 'false'

  return `
(() => {
  'use strict';

  // 会话 token（用于消息验证）
  const _SESSION_TOKEN = ${tokenLiteral};

  let messageIdCounter = 0;
  const pendingRequests = new Map();
  const eventListeners = new Map();
  const dataExchangeProviders = new Map();
  const lifecycleCallbacks = { ready: [], destroy: [], pause: [], resume: [] };
  let lifecycleReady = false;
  let lifecycleDestroyed = false;
  const _assetUrlByPath = new Map();
  const _assetUrls = new Set();
  const decodeBase64ToBytes = (base64) => {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    return bytes;
  };
  const revokeAllAssetUrls = () => {
    _assetUrls.forEach((url) => {
      try { URL.revokeObjectURL(url); } catch (e) {}
    });
    _assetUrls.clear();
    _assetUrlByPath.clear();
  };
  const runLifecycleCallbacks = (name) => {
    lifecycleCallbacks[name].slice().forEach((callback) => {
      try {
        const result = callback();
        if (result && typeof result.then === 'function') {
          result.catch((error) => {
            console.error('[Tapp] Async lifecycle callback failed:', error);
            window.Tapp?.lifecycle?._notifyError(error).catch(() => {});
          });
        }
      } catch (error) {
        console.error('[Tapp] Lifecycle callback failed:', error);
        Promise.resolve().then(() => window.Tapp?.lifecycle?._notifyError(error).catch(() => {}));
      }
    });
  };
  const notifyLifecycleDestroy = () => {
    if (lifecycleDestroyed) return;
    lifecycleDestroyed = true;
    revokeAllAssetUrls();
    runLifecycleCallbacks('destroy');
  };
  window.addEventListener('pagehide', notifyLifecycleDestroy);
  window.addEventListener('beforeunload', notifyLifecycleDestroy);

  // security wrapper 会收窄 window.parent。这里接收包装前保存的真实
  // WindowProxy，用于发送消息和验证宿主响应来源，随后立即清除 handoff。
  const _HOST_WINDOW = (() => {
    const takeNativeParent = window.__TAPP_TAKE_NATIVE_PARENT__;
    const hostWindow = typeof takeNativeParent === 'function'
      ? takeNativeParent()
      : window.parent;
    try { delete window.__TAPP_TAKE_NATIVE_PARENT__; } catch (e) {}
    return hostWindow;
  })();

  // 🎯 事件缓冲区：缓存最新的有状态事件，新监听器注册时立即回放
  // 解决父窗口推送 mediaStateChange 早于 Tapp 代码注册 onStateChange 的竞态问题
  const _eventBuffer = new Map();
  const _BUFFERED_EVENTS = new Set(['mediaStateChange', 'mediaProgress', 'themeChange', 'primaryColorChange', 'localeChange']);
  const _ACTION_TO_EVENT = { 'theme:change': 'themeChange', 'locale:change': 'localeChange', 'primaryColor:change': 'primaryColorChange' };

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
      try {
        _HOST_WINDOW.postMessage({
          type: 'request',
          id,
          action: \`\${api}.\${method}\`,
          payload: { api, method, args },
          source: '${id}',
          timestamp: Date.now(),
          _sessionToken: _SESSION_TOKEN,
        }, '*');
      } catch (error) {
        clearTimeout(timeout);
        pendingRequests.delete(id);
        reject(error);
      }
    });
  };

  window.addEventListener('message', async (event) => {
    if (event.source !== _HOST_WINDOW) return;
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
      if (message.action === 'dataExchange:invoke') {
        const invocation = message.payload || {};
        const handler = dataExchangeProviders.get(invocation.exportId);
        if (!handler) {
          sendRequest('dataExchange', 'respond', [{
            requestId: invocation.requestId,
            ok: false,
            error: 'Data Exchange provider is not registered'
          }]).catch(() => {});
        } else {
          Promise.resolve()
            .then(() => handler(invocation.params, {
              purpose: invocation.purpose,
              requestId: invocation.requestId
            }))
            .then(
              (data) => sendRequest('dataExchange', 'respond', [{ requestId: invocation.requestId, ok: true, data }]),
              (error) => sendRequest('dataExchange', 'respond', [{
                requestId: invocation.requestId,
                ok: false,
                error: error?.message || String(error)
              }])
            )
            .catch(() => {});
        }
      }
      // 缓存有状态事件的最新值（统一映射为 camelCase key，与 addEventListener 回放一致）
      const _bufKey = _ACTION_TO_EVENT[message.action] || message.action;
      if (_BUFFERED_EVENTS.has(_bufKey)) {
        _eventBuffer.set(_bufKey, message.payload);
      }
      const listeners = eventListeners.get(message.action);
      listeners?.forEach((cb) => { try { cb(message.payload); } catch (e) {} });

      if (message.action === 'lifecycle:destroy') notifyLifecycleDestroy();
      else if (message.action === 'lifecycle:pause') runLifecycleCallbacks('pause');
      else if (message.action === 'lifecycle:resume') runLifecycleCallbacks('resume');
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
        // 语义色彩变量（供 Tapp CSS 使用）
        root.style.setProperty('--text-primary', isDark ? 'rgba(255,255,255,.92)' : '#1a1a1a');
        root.style.setProperty('--text-secondary', isDark ? 'rgba(255,255,255,.5)' : '#999');
        root.style.setProperty('--bg-primary', isDark ? '#0a0a0a' : '#fff');
        document.body.style.background = isDark ? '#0a0a0a' : '#fff';
        document.body.style.color = isDark ? 'rgba(255,255,255,.92)' : '#1a1a1a';
        // 🎯 强制触发重绘（WebKit 走保守路径）
        _forceRepaint();
      }
      else if (message.action === 'locale:change') {
        currentLocale = typeof message.payload === 'string' ? message.payload : currentLocale;
        eventListeners.get('localeChange')?.forEach((cb) => cb(message.payload));
      }
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
    // 🎯 回放缓冲区：如果已有该事件的最新值，立即调用回调
    const buffered = _eventBuffer.get(event);
    if (buffered !== undefined) {
      try { callback(buffered); } catch (e) {}
    }
    return () => listeners.delete(callback);
  };

  let currentLocale = typeof window._TAPP_LOCALE === 'string'
    ? window._TAPP_LOCALE
    : 'zh-CN';
  const translate = (key, variables = {}) => {
    const all = window._TAPP_I18N && typeof window._TAPP_I18N === 'object'
      ? window._TAPP_I18N
      : {};
    const language = currentLocale.split('-')[0];
    const table = all[currentLocale] || all[language] || all['en-US'] || all['zh-CN'] || {};
    const directValue = table && typeof table === 'object' ? table[String(key)] : undefined;
    const value = typeof directValue === 'string'
      ? directValue
      : String(key).split('.').reduce(
          (current, part) => current && typeof current === 'object' ? current[part] : undefined,
          table,
        );
    const text = typeof value === 'string' ? value : String(key);
    return text.replace(/\{([a-zA-Z0-9_]+)\}/g, (match, name) =>
      Object.prototype.hasOwnProperty.call(variables, name) ? String(variables[name]) : match
    );
  };

  const Tapp = {
    id: ${idLiteral},
    version: ${versionLiteral},
    name: ${nameLiteral},
    permissions: ${permissionsLiteral},

    lifecycle: {
      onReady: (cb) => {
        if (lifecycleReady) Promise.resolve().then(() => cb());
        else lifecycleCallbacks.ready.push(cb);
      },
      onDestroy: (cb) => lifecycleCallbacks.destroy.push(cb),
      onPause: (cb) => lifecycleCallbacks.pause.push(cb),
      onResume: (cb) => lifecycleCallbacks.resume.push(cb),
      getInfo: () => ({ id: ${idLiteral}, version: ${versionLiteral}, name: ${nameLiteral}, permissions: ${permissionsLiteral}, sandboxed: true }),
      _notifyError: (err) => sendRequest('lifecycle', 'error', [err.message || String(err)]),
      _notifyReady: () => {
        if (lifecycleReady) return;
        lifecycleReady = true;
        sendRequest('lifecycle', 'ready', []);
        runLifecycleCallbacks('ready');
      },
    },

    i18n: {
      t: translate,
      getLocale: () => currentLocale,
      getAll: () => {
        const all = window._TAPP_I18N;
        return all && typeof all === 'object' ? JSON.parse(JSON.stringify(all)) : {};
      },
    },

    widget: {
      register: (cfg) => sendRequest('widget', 'register', [cfg]),
      unregister: (id) => sendRequest('widget', 'unregister', [id]),
      listRegistered: () => sendRequest('widget', 'listRegistered', []),
      updateConfig: (id, cfg) => sendRequest('widget', 'updateConfig', [id, cfg]),
    },

    tappList: {
      list: () => sendRequest('tappList', 'list', []),
      get: (id) => sendRequest('tappList', 'get', [id]),
      getRecent: (limit) => sendRequest('tappList', 'getRecent', [limit]),
      /** Build a direct-install package from an installed Tapp (for chat share). */
      getInstallPackage: (id, opts) => sendRequest('tappList', 'getInstallPackage', [id, opts]),
      /** Resolve portable store catalog URL for a Tapp id (for peer store install). */
      resolveStoreSource: (id) => sendRequest('tappList', 'resolveStoreSource', [id]),
      install: (req) => sendRequest('tappList', 'install', [req]),
      uninstall: (id) => sendRequest('tappList', 'uninstall', [id]),
      start: (id) => sendRequest('tappList', 'start', [id]),
      stop: (id) => sendRequest('tappList', 'stop', [id]),
      export: (id) => sendRequest('tappList', 'export', [id]),
    },

    brewList: {
      // 读取
      list: (o) => sendRequest('brewList', 'list', [o]),
      get: (id) => sendRequest('brewList', 'get', [id]),
      sources: () => sendRequest('brewList', 'sources', []),
      categories: () => sendRequest('brewList', 'categories', []),
      stats: () => sendRequest('brewList', 'stats', []),
      discover: (url) => sendRequest('brewList', 'discover', [url]),
      exportOpml: () => sendRequest('brewList', 'exportOpml', []),
      // 写入
      markRead: (id) => sendRequest('brewList', 'markRead', [id]),
      markUnread: (id) => sendRequest('brewList', 'markUnread', [id]),
      star: (id) => sendRequest('brewList', 'star', [id]),
      unstar: (id) => sendRequest('brewList', 'unstar', [id]),
      markAllRead: (o) => sendRequest('brewList', 'markAllRead', [o]),
      // 评论
      getComments: (itemId) => sendRequest('brewList', 'getComments', [itemId]),
      createComment: (itemId, req) => sendRequest('brewList', 'createComment', [itemId, req]),
      updateComment: (commentId, req) => sendRequest('brewList', 'updateComment', [commentId, req]),
      deleteComment: (commentId) => sendRequest('brewList', 'deleteComment', [commentId]),
      getReplies: (commentId) => sendRequest('brewList', 'getReplies', [commentId]),
      createReply: (itemId, parentId, content) => sendRequest('brewList', 'createReply', [itemId, parentId, content]),
      // 管理
      addSource: (req) => sendRequest('brewList', 'addSource', [req]),
      updateSource: (id, req) => sendRequest('brewList', 'updateSource', [id, req]),
      deleteSource: (id) => sendRequest('brewList', 'deleteSource', [id]),
      refreshSource: (id) => sendRequest('brewList', 'refreshSource', [id]),
      importOpml: (opml) => sendRequest('brewList', 'importOpml', [opml]),
      createCategory: (req) => sendRequest('brewList', 'createCategory', [req]),
      deleteCategory: (id) => sendRequest('brewList', 'deleteCategory', [id]),
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
      tasks: {
        create: (request) => sendRequest('ai', 'tasks.create', [request]),
        get: (taskId) => sendRequest('ai', 'tasks.get', [taskId]),
        cancel: (taskId) => sendRequest('ai', 'tasks.cancel', [taskId]),
        usage: () => sendRequest('ai', 'tasks.usage', []),
        subscribe: (taskId, callback) => {
          if (typeof taskId !== 'string' || typeof callback !== 'function') {
            return Promise.reject(new Error('taskId and callback are required'));
          }
          const removeListener = addEventListener('aiTaskEvent', (event) => {
            if (event?.taskId === taskId) callback({ event: event.event, data: event.data });
          });
          return sendRequest('ai', 'tasks.subscribe', [taskId]).then(
            () => () => {
              removeListener();
              sendRequest('ai', 'tasks.unsubscribe', [taskId]).catch(() => {});
            },
            (error) => {
              removeListener();
              throw error;
            },
          );
        },
      },
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
      getAll: () => sendRequest('storage', 'getAll', []),
      clear: () => sendRequest('storage', 'clear', []),
      usage: () => sendRequest('storage', 'usage', []),
      onChanged: (cb) => addEventListener('storageChanged', cb),
    },

    dataExchange: {
      request: (request) => sendRequest('dataExchange', 'request', [request]),
      provide: async (exportId, handler) => {
        if (typeof exportId !== 'string' || typeof handler !== 'function') {
          throw new Error('exportId and provider handler are required');
        }
        if (dataExchangeProviders.has(exportId)) {
          throw new Error('Data Exchange provider is already registered: ' + exportId);
        }
        dataExchangeProviders.set(exportId, handler);
        try {
          await sendRequest('dataExchange', 'registerProvider', [exportId]);
        } catch (error) {
          if (dataExchangeProviders.get(exportId) === handler) dataExchangeProviders.delete(exportId);
          throw error;
        }
        return () => {
          if (dataExchangeProviders.get(exportId) !== handler) return;
          dataExchangeProviders.delete(exportId);
          sendRequest('dataExchange', 'unregisterProvider', [exportId]).catch(() => {});
        };
      },
    },

    settings: {
      get: (k) => { validateStorageKey(k); return sendRequest('settings', 'get', [k]); },
      set: (k, v) => { validateStorageKey(k); return sendRequest('settings', 'set', [k, v]); },
      getAll: () => sendRequest('settings', 'getAll', []),
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
    // access 只控制调用者范围：
    // - public: 所有用户（包括游客）可调用
    // - protected: 需登录（默认）
    // 所有 type: "http" 均需 network:fetch；builtin 按 ai:* 等能力校验
    api: Object.assign(
      (name, params) => sendRequest('api', 'execute', [name, params]),
      { list: () => sendRequest('api', 'list', []) },
    ),

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
      getLyrics: (opts) => sendRequest('media', 'getLyrics', [opts || {}]),
      getBeatGrid: () => sendRequest('media', 'getBeatGrid', []),
      playTrack: (id, idx) => sendRequest('media', 'playTrack', [{ trackId: id, trackIndex: idx }]),
      jumpToIndex: (idx) => sendRequest('media', 'jumpToIndex', [{ index: idx }]),
      loadNeteasePlaylist: (playlistId) => sendRequest('media', 'loadNeteasePlaylist', [{ playlistId }]),
      getSkipVip: () => sendRequest('media', 'getSkipVip', []),
      setSkipVip: (value) => sendRequest('media', 'setSkipVip', [{ value: value }]),
      onStateChange: (cb) => addEventListener('mediaStateChange', cb),
      onProgress: (cb) => addEventListener('mediaProgress', cb),
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

    // 🤖 Agent 交互 API - 允许 Tapp 与 Agent 进行数据交互
    agent: {
      onInteraction: (type, callback) => {
        if (typeof type !== 'string' || typeof callback !== 'function') {
          throw new Error('interaction type and callback are required');
        }
        return addEventListener('agentInteractionV2', (raw) => {
          if (raw?.type !== type) return;
          const interaction = {
            ...raw,
            accept: () => sendRequest('agent', 'v2.accept', [raw.interactionId]),
            submitResult: (result) => sendRequest('agent', 'v2.result', [raw.interactionId, {
              ...result,
              idempotencyKey: result?.idempotencyKey || \`result-\${raw.interactionId}\`,
            }]),
            reject: (reason) => sendRequest('agent', 'v2.reject', [raw.interactionId, reason]),
            requestIntent: (request) => sendRequest('agent', 'v2.intent', [raw.interactionId, request]),
          };
          callback(interaction);
        });
      },
    },

    event: {
      publish: (request) => sendRequest('event', 'publish', [request]),
      on: (topic, callback) => {
        if (typeof topic !== 'string' || typeof callback !== 'function') {
          throw new Error('topic and callback are required');
        }
        return addEventListener('tappEvent', (event) => {
          if (event?.topic === topic) callback(event);
        });
      },
    },

    dom: ${DOM_HELPERS_CODE},

    file: {
      download: (content, filename, mimeType) => sendRequest('file', 'download', [{ content, filename, mimeType }]),
    },

    // Package-static assets declared in manifest.assets.
    // Blob URLs are created inside the sandbox (opaque origin).
    assets: {
      list: () => sendRequest('assets', 'list', []),
      get: (path) => sendRequest('assets', 'get', [path]),
      getUrl: async (path) => {
        if (typeof path !== 'string' || !path) throw new Error('Asset path is required');
        const cached = _assetUrlByPath.get(path);
        if (cached) return cached;
        const asset = await sendRequest('assets', 'get', [path]);
        const bytes = decodeBase64ToBytes(asset.base64);
        const blob = new Blob([bytes], { type: asset.mimeType || 'application/octet-stream' });
        const url = URL.createObjectURL(blob);
        _assetUrlByPath.set(path, { url: url, mimeType: asset.mimeType, size: asset.size, path: path });
        _assetUrls.add(url);
        return _assetUrlByPath.get(path);
      },
      getArrayBuffer: async (path) => {
        const asset = await sendRequest('assets', 'get', [path]);
        const bytes = decodeBase64ToBytes(asset.base64);
        return { path: asset.path, mimeType: asset.mimeType, size: asset.size, buffer: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) };
      },
      revoke: (url) => {
        if (typeof url !== 'string') return;
        try { URL.revokeObjectURL(url); } catch (e) {}
        _assetUrls.delete(url);
        _assetUrlByPath.forEach((value, key) => {
          if (value && value.url === url) _assetUrlByPath.delete(key);
        });
      },
      revokeAll: () => revokeAllAssetUrls(),
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

    scheduler: {
      register: (options) => sendRequest('scheduler', 'register', [options]),
      unregister: (taskId) => sendRequest('scheduler', 'unregister', [taskId]),
      list: () => sendRequest('scheduler', 'list', []),
      get: (taskId) => sendRequest('scheduler', 'get', [taskId]),
      enable: (taskId) => sendRequest('scheduler', 'enable', [taskId]),
      disable: (taskId) => sendRequest('scheduler', 'disable', [taskId]),
      trigger: (taskId) => sendRequest('scheduler', 'trigger', [taskId]),
      onTask: (taskId, cb) => {
        if (!taskId || typeof cb !== 'function') throw new Error('taskId and callback required');
        sendRequest('scheduler', 'subscribe', [taskId]).catch(() => {});
        const removeListener = addEventListener('schedulerTask', (d) => {
          if (!d || d.taskId !== taskId) return;
          const event = d.event || d;
          Promise.resolve()
            .then(() => cb(d.payload, event))
            .then(
              () => sendRequest('scheduler', 'complete', [event.executionId, true]),
              (error) => sendRequest('scheduler', 'complete', [
                event.executionId,
                false,
                error && error.message ? error.message : String(error),
              ]),
            )
            .catch(() => {});
        });
        return () => {
          removeListener();
          sendRequest('scheduler', 'unsubscribe', [taskId]).catch(() => {});
        };
      },
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

    speech: {
      tts: (r) => sendRequest('speech', 'tts', [r]),
      getVoices: () => sendRequest('speech', 'getVoices', []),
      getStatus: () => sendRequest('speech', 'getStatus', []),
      asr: (r) => sendRequest('speech', 'asr', [r]),
    },

    federation: {
      // 身份
      getIdentity: () => sendRequest('federation', 'getIdentity', []),
      // 时间线
      getFeed: () => sendRequest('federation', 'getFeed', []),
      getTimeline: () => sendRequest('federation', 'getTimeline', []),
      // 关注
      follow: (target) => sendRequest('federation', 'follow', [target]),
      unfollow: (target) => sendRequest('federation', 'unfollow', [target]),
      getFollowing: () => sendRequest('federation', 'getFollowing', []),
      getFollowers: () => sendRequest('federation', 'getFollowers', []),
      // 发布
      publish: (req) => sendRequest('federation', 'publish', [req]),
      createNote: (req) => sendRequest('federation', 'createNote', [req]),
      uploadMedia: (req) => sendRequest('federation', 'uploadMedia', [req]),
      unpublish: (req) => sendRequest('federation', 'unpublish', [req]),
      getPublished: () => sendRequest('federation', 'getPublished', []),
      // Channel
      getChannels: () => sendRequest('federation', 'getChannels', []),
      getChannel: (id) => sendRequest('federation', 'getChannel', [id]),
      createChannel: (req) => sendRequest('federation', 'createChannel', [req]),
      acceptChannel: (id) => sendRequest('federation', 'acceptChannel', [id]),
      closeChannel: (id) => sendRequest('federation', 'closeChannel', [id]),
      getMessages: (channelId, before, limit) => sendRequest('federation', 'getMessages', [channelId, before, limit]),
      sendMessage: (channelId, req) => sendRequest('federation', 'sendMessage', [channelId, req]),
      // Room
      getRooms: () => sendRequest('federation', 'getRooms', []),
      getRoom: (id) => sendRequest('federation', 'getRoom', [id]),
      createRoom: (req) => sendRequest('federation', 'createRoom', [req]),
      updateRoom: (id, req) => sendRequest('federation', 'updateRoom', [id, req]),
      getRoomMembers: (roomId) => sendRequest('federation', 'getRoomMembers', [roomId]),
      getRoomMessages: (roomId, before, limit) => sendRequest('federation', 'getRoomMessages', [roomId, before, limit]),
      sendRoomMessage: (roomId, req) => sendRequest('federation', 'sendRoomMessage', [roomId, req]),
      pinRoomMessage: (roomId, messageId, pinned) => sendRequest('federation', 'pinRoomMessage', [roomId, messageId, pinned]),
      inviteMember: (roomId, req) => sendRequest('federation', 'inviteMember', [roomId, req]),
      removeMember: (roomId, actorUrl) => sendRequest('federation', 'removeMember', [roomId, actorUrl]),
      leaveRoom: (roomId) => sendRequest('federation', 'leaveRoom', [roomId]),
      deleteRoom: (roomId) => sendRequest('federation', 'deleteRoom', [roomId]),
      // Ring
      getRings: () => sendRequest('federation', 'getRings', []),
      getRing: (id) => sendRequest('federation', 'getRing', [id]),
      getRingPeers: (id) => sendRequest('federation', 'getRingPeers', [id]),
      createRing: (req) => sendRequest('federation', 'createRing', [req]),
      leaveRing: (ringId) => sendRequest('federation', 'leaveRing', [ringId]),
      addPeer: (ringId, req) => sendRequest('federation', 'addPeer', [ringId, req]),
      removePeer: (ringId, peerUrl) => sendRequest('federation', 'removePeer', [ringId, peerUrl]),
      triggerSync: (ringId) => sendRequest('federation', 'triggerSync', [ringId]),
      // Trust 策略
      getTrustPolicy: () => sendRequest('federation', 'getTrustPolicy', []),
      getInstances: () => sendRequest('federation', 'getInstances', []),
      updateInstanceTrust: (req) => sendRequest('federation', 'updateInstanceTrust', [req]),
      toggleInstanceBlock: (req) => sendRequest('federation', 'toggleInstanceBlock', [req]),
      // 文件传输
      initiateTransfer: (channelId, req) => sendRequest('federation', 'initiateTransfer', [channelId, req]),
      listTransfers: (channelId) => sendRequest('federation', 'listTransfers', [channelId]),
      getTransfer: (transferId) => sendRequest('federation', 'getTransfer', [transferId]),
      uploadChunk: (transferId, req) => sendRequest('federation', 'uploadChunk', [transferId, req]),
      cancelTransfer: (transferId) => sendRequest('federation', 'cancelTransfer', [transferId]),
      // 实时订阅
      subscribeChannel: (channelId) => sendRequest('federation', 'subscribeChannel', [channelId]),
      unsubscribeChannel: (channelId) => sendRequest('federation', 'unsubscribeChannel', [channelId]),
      subscribeRoom: (roomId) => sendRequest('federation', 'subscribeRoom', [roomId]),
      unsubscribeRoom: (roomId) => sendRequest('federation', 'unsubscribeRoom', [roomId]),
      // 事件
      onMessage: (cb) => addEventListener('federation:message', cb),
      onChannelUpdate: (cb) => addEventListener('federation:channelUpdate', cb),
      onRoomUpdate: (cb) => addEventListener('federation:roomUpdate', cb),
    },

    on: addEventListener,
    widgets: {},
    pages: {},
  };

  // Headless core is a background capability profile, not an invisible Page.
  // Keep data/scheduler/event/media/federation APIs, but remove visible UI and
  // host control-plane namespaces before the public object is frozen.
  if (${headlessLiteral}) {
    Tapp.ui = {
      getTheme: Tapp.ui.getTheme,
      onThemeChange: Tapp.ui.onThemeChange,
      getPrimaryColor: Tapp.ui.getPrimaryColor,
      onPrimaryColorChange: Tapp.ui.onPrimaryColorChange,
      getLocale: Tapp.ui.getLocale,
      onLocaleChange: Tapp.ui.onLocaleChange,
      showNotification: Tapp.ui.showNotification,
    };
    delete Tapp.widget;
    delete Tapp.tappList;
    delete Tapp.component;
    delete Tapp.shortcut;
    delete Tapp.dynamicContent;
    delete Tapp.dom;
    delete Tapp.file;
    delete Tapp.widgets;
    delete Tapp.pages;
  }

  // 冻结所有 API 对象（防止篡改）
  Object.freeze(Tapp);
  Object.freeze(Tapp.lifecycle);
  Object.freeze(Tapp.i18n);
  Object.freeze(Tapp.widget);
  Object.freeze(Tapp.tappList);
  Object.freeze(Tapp.brewList);
  Object.freeze(Tapp.platform);
  Object.freeze(Tapp.ai.tasks);
  Object.freeze(Tapp.ai);
  Object.freeze(Tapp.report);
  Object.freeze(Tapp.storage);
  Object.freeze(Tapp.dataExchange);
  Object.freeze(Tapp.settings);
  Object.freeze(Tapp.ui);
  Object.freeze(Tapp.ui.fullscreen);
  Object.freeze(Tapp.data);
  Object.freeze(Tapp.api);
  Object.freeze(Tapp.context);
  Object.freeze(Tapp.media);
  Object.freeze(Tapp.component);
  Object.freeze(Tapp.shortcut);
  Object.freeze(Tapp.event);
  Object.freeze(Tapp.dom);
  Object.freeze(Tapp.file);
  Object.freeze(Tapp.assets);
  Object.freeze(Tapp.user);
  Object.freeze(Tapp.background);
  Object.freeze(Tapp.scheduler);
  Object.freeze(Tapp.dynamicContent);
  Object.freeze(Tapp.animation);
  Object.freeze(Tapp.speech);
  Object.freeze(Tapp.federation);

  // widgets/pages 容器保持可扩展：Tapp 代码需要向其注册定义
  // （Object.seal 会禁止新增属性，strict 模式下注册直接抛 TypeError）。
  // 整个容器不可被替换——Tapp 已被 freeze，属性绑定是只读的。

  // 防止通过原型链篡改
  Object.freeze(Object.getPrototypeOf(Tapp));

  window.Tapp = Tapp;

  // 防止重新定义 Tapp
  Object.defineProperty(window, 'Tapp', {
    value: Tapp,
    writable: false,
    configurable: false
  });

  window.addEventListener('error', (event) => {
    const error = event.error || new Error(event.message || 'Unknown window error');
    Tapp.lifecycle._notifyError(error).catch(() => {});
  });
  window.addEventListener('unhandledrejection', (event) => {
    const reason = event.reason;
    const error = reason instanceof Error ? reason : new Error(String(reason));
    Tapp.lifecycle._notifyError(error).catch(() => {});
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
export function generateWidgetSDK(
  tappInstance: TappInstance,
  sessionToken?: string,
): string {
  const { id, manifest, grantedPermissions } = tappInstance
  const token = sessionToken || ''
  const idLiteral = serializeSandboxScriptValue(id)
  const nameLiteral = serializeSandboxScriptValue(manifest.name)
  const versionLiteral = serializeSandboxScriptValue(manifest.version)
  const tokenLiteral = serializeSandboxScriptValue(token)
  const permissionsLiteral = serializeSandboxScriptValue(
    grantedPermissions || [],
  )

  return `
(function() {
  'use strict';

  // 会话 token（用于消息验证）
  var _SESSION_TOKEN = ${tokenLiteral};

  var messageIdCounter = 0;
  var pendingRequests = new Map();
  var eventListeners = new Map();
  var dataExchangeProviders = new Map();
  // 🎯 添加生命周期回调支持
  var lifecycleCallbacks = { destroy: [], pause: [], resume: [] };
  var lifecycleDestroyed = false;
  var _assetUrlByPath = new Map();
  var _assetUrls = new Set();
  var decodeBase64ToBytes = function(base64) {
    var binary = atob(base64);
    var bytes = new Uint8Array(binary.length);
    for (var i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    return bytes;
  };
  var revokeAllAssetUrls = function() {
    _assetUrls.forEach(function(url) {
      try { URL.revokeObjectURL(url); } catch (e) {}
    });
    _assetUrls.clear();
    _assetUrlByPath.clear();
  };
  var notifyLifecycleDestroy = function() {
    if (lifecycleDestroyed) return;
    lifecycleDestroyed = true;
    revokeAllAssetUrls();
    lifecycleCallbacks.destroy.slice().forEach(function(cb) {
      try { cb(); } catch (e) { console.error('[Tapp Widget] Destroy callback failed:', e); }
    });
  };
  window.addEventListener('pagehide', notifyLifecycleDestroy);
  window.addEventListener('beforeunload', notifyLifecycleDestroy);

  // 发送与接收都绑定到创建当前沙箱的真实父窗口。
  var _HOST_WINDOW = window.parent;

  // 🎯 事件缓冲区：缓存最新的有状态事件，新监听器注册时立即回放
  var _eventBuffer = new Map();
  var _BUFFERED_EVENTS = { mediaStateChange: 1, mediaProgress: 1, themeChange: 1, primaryColorChange: 1, localeChange: 1 };
  var _ACTION_TO_EVENT = { 'theme:change': 'themeChange', 'locale:change': 'localeChange', 'primaryColor:change': 'primaryColorChange' };

  var generateId = function() { return 'widget-' + (++messageIdCounter) + '-' + Date.now(); };

  ${generateStorageKeyValidator()}

  var sendRequest = function(api, method, args) {
    args = args || [];
    return new Promise(function(resolve, reject) {
      var id = generateId();
      var timeout = setTimeout(function() { pendingRequests.delete(id); reject(new Error('Request timeout')); }, 30000);
      pendingRequests.set(id, { resolve: resolve, reject: reject, timeout: timeout });
      try {
        _HOST_WINDOW.postMessage({
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
    // 🎯 回放缓冲区：如果已有该事件的最新值，立即调用回调
    var buffered = _eventBuffer.get(event);
    if (buffered !== undefined) {
      try { callback(buffered); } catch(e) {}
    }
    return function() { listeners.delete(callback); };
  };

  window.addEventListener('message', function(event) {
    if (event.source !== _HOST_WINDOW) return;
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
      if (msg.action === 'dataExchange:invoke') {
        var invocation = msg.payload || {};
        var provider = dataExchangeProviders.get(invocation.exportId);
        if (!provider) {
          sendRequest('dataExchange', 'respond', [{
            requestId: invocation.requestId,
            ok: false,
            error: 'Data Exchange provider is not registered'
          }]).catch(function() {});
        } else {
          Promise.resolve()
            .then(function() {
              return provider(invocation.params, {
                purpose: invocation.purpose,
                requestId: invocation.requestId
              });
            })
            .then(function(data) {
              return sendRequest('dataExchange', 'respond', [{ requestId: invocation.requestId, ok: true, data: data }]);
            }, function(error) {
              return sendRequest('dataExchange', 'respond', [{
                requestId: invocation.requestId,
                ok: false,
                error: error && error.message ? error.message : String(error)
              }]);
            })
            .catch(function() {});
        }
      }
      // 🎯 缓存有状态事件的最新值（供 addEventListener 回放，统一 camelCase key）
      var _bufKey = _ACTION_TO_EVENT[msg.action] || msg.action;
      if (_BUFFERED_EVENTS[_bufKey]) {
        _eventBuffer.set(_bufKey, msg.payload);
      }
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
        // 语义色彩变量（供 Tapp CSS 使用）
        root.style.setProperty('--text-primary', isDark ? 'rgba(255,255,255,.92)' : '#1a1a1a');
        root.style.setProperty('--text-secondary', isDark ? 'rgba(255,255,255,.5)' : '#999');
        root.style.setProperty('--bg-primary', isDark ? '#0a0a0a' : '#fff');
        document.body.style.background = isDark ? '#0a0a0a' : '#fff';
        document.body.style.color = isDark ? 'rgba(255,255,255,.92)' : '#1a1a1a';
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
        currentLocale = typeof msg.payload === 'string' ? msg.payload : currentLocale;
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
      // 🎵 媒体状态变化事件
      else if (msg.action === 'mediaStateChange') {
        eventListeners.get('mediaStateChange')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
      }
      // 🎵 媒体进度实时推送
      else if (msg.action === 'mediaProgress') {
        eventListeners.get('mediaProgress')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
      }
      else if (msg.action === 'storageChanged') {
        eventListeners.get('storageChanged')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
      }
    }
  });

  var currentLocale = typeof window._TAPP_LOCALE === 'string'
    ? window._TAPP_LOCALE
    : 'zh-CN';
  function translate(key, variables) {
    variables = variables || {};
    var all = window._TAPP_I18N && typeof window._TAPP_I18N === 'object'
      ? window._TAPP_I18N
      : {};
    var language = currentLocale.split('-')[0];
    var table = all[currentLocale] || all[language] || all['en-US'] || all['zh-CN'] || {};
    var directValue = table && typeof table === 'object' ? table[String(key)] : undefined;
    var value = typeof directValue === 'string'
      ? directValue
      : String(key).split('.').reduce(function(current, part) {
          return current && typeof current === 'object' ? current[part] : undefined;
        }, table);
    var text = typeof value === 'string' ? value : String(key);
    return text.replace(/\{([a-zA-Z0-9_]+)\}/g, function(match, name) {
      return Object.prototype.hasOwnProperty.call(variables, name) ? String(variables[name]) : match;
    });
  }

  window.Tapp = {
    id: ${idLiteral},
    name: ${nameLiteral},
    version: ${versionLiteral},
    permissions: ${permissionsLiteral},
    widgets: {},
    pages: {},

    // 生命周期 API
    lifecycle: {
      onReady: function(cb) { if (document.readyState === 'complete') setTimeout(cb, 0); else window.addEventListener('load', cb); },
      onDestroy: function(cb) { lifecycleCallbacks.destroy.push(cb); },
      onPause: function(cb) { lifecycleCallbacks.pause.push(cb); },
      onResume: function(cb) { lifecycleCallbacks.resume.push(cb); }
    },

    i18n: {
      t: translate,
      getLocale: function() { return currentLocale; },
      getAll: function() {
        var all = window._TAPP_I18N;
        return all && typeof all === 'object' ? JSON.parse(JSON.stringify(all)) : {};
      }
    },

    widget: {
      getInstanceSettings: function() {
        return Object.assign({}, (window._TAPP_WIDGET_PROPS && window._TAPP_WIDGET_PROPS.config) || {});
      },
      updateInstanceSettings: function(patch) {
        return sendRequest('widget', 'instanceSettings.update', [patch]);
      },
      invalidate: function(reason) {
        return sendRequest('widget', 'invalidate', [reason]);
      }
    },

    storage: {
      get: function(k) { validateStorageKey(k); return sendRequest('storage', 'get', [k]); },
      set: function(k, v) { validateStorageKey(k); return sendRequest('storage', 'set', [k, v]); },
      remove: function(k) { validateStorageKey(k); return sendRequest('storage', 'remove', [k]); },
      keys: function() { return sendRequest('storage', 'keys', []); },
      getAll: function() { return sendRequest('storage', 'getAll', []); },
      clear: function() { return sendRequest('storage', 'clear', []); },
      usage: function() { return sendRequest('storage', 'usage', []); },
      onChanged: function(cb) { return addEventListener('storageChanged', cb); }
    },

    dataExchange: {
      request: function(request) { return sendRequest('dataExchange', 'request', [request]); },
      provide: function(exportId, handler) {
        if (typeof exportId !== 'string' || typeof handler !== 'function') {
          return Promise.reject(new Error('exportId and provider handler are required'));
        }
        if (dataExchangeProviders.has(exportId)) {
          return Promise.reject(new Error('Data Exchange provider is already registered: ' + exportId));
        }
        dataExchangeProviders.set(exportId, handler);
        return sendRequest('dataExchange', 'registerProvider', [exportId]).then(function() {
          return function() {
            if (dataExchangeProviders.get(exportId) !== handler) return;
            dataExchangeProviders.delete(exportId);
            sendRequest('dataExchange', 'unregisterProvider', [exportId]).catch(function() {});
          };
        }, function(error) {
          if (dataExchangeProviders.get(exportId) === handler) dataExchangeProviders.delete(exportId);
          throw error;
        });
      }
    },

    settings: {
      get: function(k) { validateStorageKey(k); return sendRequest('settings', 'get', [k]); },
      set: function(k, v) { validateStorageKey(k); return sendRequest('settings', 'set', [k, v]); },
      getAll: function() { return sendRequest('settings', 'getAll', []); }
    },

    ai: {
      tasks: {
        create: function(request) { return sendRequest('ai', 'tasks.create', [request]); },
        get: function(taskId) { return sendRequest('ai', 'tasks.get', [taskId]); },
        cancel: function(taskId) { return sendRequest('ai', 'tasks.cancel', [taskId]); },
        usage: function() { return sendRequest('ai', 'tasks.usage', []); },
        subscribe: function(taskId, callback) {
          if (typeof taskId !== 'string' || typeof callback !== 'function') {
            return Promise.reject(new Error('taskId and callback are required'));
          }
          var removeListener = addEventListener('aiTaskEvent', function(event) {
            if (event && event.taskId === taskId) callback({ event: event.event, data: event.data });
          });
          return sendRequest('ai', 'tasks.subscribe', [taskId]).then(function() {
            return function() {
              removeListener();
              sendRequest('ai', 'tasks.unsubscribe', [taskId]).catch(function() {});
            };
          }, function(error) {
            removeListener();
            throw error;
          });
        }
      }
    },

    event: {
      publish: function(request) { return sendRequest('event', 'publish', [request]); },
      on: function(topic, callback) {
        if (typeof topic !== 'string' || typeof callback !== 'function') {
          throw new Error('topic and callback are required');
        }
        return addEventListener('tappEvent', function(event) {
          if (event && event.topic === topic) callback(event);
        });
      }
    },

    agent: {
      onInteraction: function(type, callback) {
        if (typeof type !== 'string' || typeof callback !== 'function') {
          throw new Error('interaction type and callback are required');
        }
        return addEventListener('agentInteractionV2', function(raw) {
          if (!raw || raw.type !== type) return;
          callback(Object.assign({}, raw, {
            accept: function() { return sendRequest('agent', 'v2.accept', [raw.interactionId]); },
            submitResult: function(result) {
              result = result || {};
              return sendRequest('agent', 'v2.result', [raw.interactionId, Object.assign({}, result, {
                idempotencyKey: result.idempotencyKey || ('result-' + raw.interactionId)
              })]);
            },
            reject: function(reason) { return sendRequest('agent', 'v2.reject', [raw.interactionId, reason]); },
            requestIntent: function(request) { return sendRequest('agent', 'v2.intent', [raw.interactionId, request]); }
          }));
        });
      }
    },

    media: {
      play: function() { return sendRequest('media', 'control', [{ action: 'play' }]); },
      pause: function() { return sendRequest('media', 'control', [{ action: 'pause' }]); },
      next: function() { return sendRequest('media', 'control', [{ action: 'next' }]); },
      prev: function() { return sendRequest('media', 'control', [{ action: 'prev' }]); },
      seek: function(p) { return sendRequest('media', 'control', [{ action: 'seek', value: p }]); },
      setVolume: function(v) { return sendRequest('media', 'control', [{ action: 'volume', value: v }]); },
      setMode: function(m) { return sendRequest('media', 'control', [{ action: 'mode', value: m }]); },
      mute: function() { return sendRequest('media', 'control', [{ action: 'mute' }]); },
      unmute: function() { return sendRequest('media', 'control', [{ action: 'unmute' }]); },
      getStatus: function() { return sendRequest('media', 'getStatus', []); },
      getPlaylist: function() { return sendRequest('media', 'getPlaylist', []); },
      getSpectrum: function() { return sendRequest('media', 'getSpectrum', []); },
      getLyrics: function(opts) { return sendRequest('media', 'getLyrics', [opts || {}]); },
      getBeatGrid: function() { return sendRequest('media', 'getBeatGrid', []); },
      playTrack: function(id, idx) { return sendRequest('media', 'playTrack', [{ trackId: id, trackIndex: idx }]); },
      jumpToIndex: function(idx) { return sendRequest('media', 'jumpToIndex', [{ index: idx }]); },
      loadNeteasePlaylist: function(playlistId) { return sendRequest('media', 'loadNeteasePlaylist', [{ playlistId: playlistId }]); },
      getSkipVip: function() { return sendRequest('media', 'getSkipVip', []); },
      setSkipVip: function(value) { return sendRequest('media', 'setSkipVip', [{ value: value }]); },
      onStateChange: function(cb) { return addEventListener('mediaStateChange', cb); },
      onProgress: function(cb) { return addEventListener('mediaProgress', cb); }
    },

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

    scheduler: {
      register: function(options) { return sendRequest('scheduler', 'register', [options]); },
      unregister: function(taskId) { return sendRequest('scheduler', 'unregister', [taskId]); },
      list: function() { return sendRequest('scheduler', 'list', []); },
      get: function(taskId) { return sendRequest('scheduler', 'get', [taskId]); },
      enable: function(taskId) { return sendRequest('scheduler', 'enable', [taskId]); },
      disable: function(taskId) { return sendRequest('scheduler', 'disable', [taskId]); },
      trigger: function(taskId) { return sendRequest('scheduler', 'trigger', [taskId]); },
      onTask: function(taskId, cb) {
        if (!taskId || typeof cb !== 'function') throw new Error('taskId and callback required');
        var subscribeRequest = sendRequest('scheduler', 'subscribe', [taskId]);
        if (subscribeRequest && subscribeRequest.catch) subscribeRequest.catch(function() {});
        var removeListener = addEventListener('schedulerTask', function(d) {
          if (!d || d.taskId !== taskId) return;
          var event = d.event || d;
          Promise.resolve().then(function() {
            return cb(d.payload, event);
          }).then(function() {
            return sendRequest('scheduler', 'complete', [event.executionId, true]);
          }, function(error) {
            return sendRequest('scheduler', 'complete', [
              event.executionId,
              false,
              error && error.message ? error.message : String(error)
            ]);
          }).catch(function() {});
        });
        return function() {
          removeListener();
          var unsubscribeRequest = sendRequest('scheduler', 'unsubscribe', [taskId]);
          if (unsubscribeRequest && unsubscribeRequest.catch) unsubscribeRequest.catch(function() {});
        };
      }
    },

    animation: {
      getLevel: function() { return sendRequest('animation', 'getLevel', []); },
      shouldAnimate: function() { return sendRequest('animation', 'shouldAnimate', []); },
      getConfig: function() { return sendRequest('animation', 'getConfig', []); },
      getStaggerDelay: function(i, d) { return sendRequest('animation', 'getStaggerDelay', [i, d]); },
      onLevelChange: function(cb) { return addEventListener('animationLevelChange', cb); }
    },

    speech: {
      tts: function(r) { return sendRequest('speech', 'tts', [r]); },
      getVoices: function() { return sendRequest('speech', 'getVoices', []); },
      getStatus: function() { return sendRequest('speech', 'getStatus', []); },
      asr: function(r) { return sendRequest('speech', 'asr', [r]); }
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
    // access 只控制调用者范围：
    // - public: 所有用户（包括游客）可调用
    // - protected: 需登录（默认）
    // 所有 type: "http" 均需 network:fetch；builtin 按 ai:* 等能力校验
    api: Object.assign(
      function(name, params) { return sendRequest('api', 'execute', [name, params]); },
      { list: function() { return sendRequest('api', 'list', []); } }
    ),

    // 获取上下文信息
    context: {
      getApp: function() { return sendRequest('context', 'getApp', []); },
      getUser: function() { return sendRequest('context', 'getUser', []); },
      getPlayer: function() { return sendRequest('context', 'getPlayer', []); },
      getNavigation: function() { return sendRequest('context', 'getNavigation', []); },
      getSystem: function() { return sendRequest('context', 'getSystem', []); },
      getGeo: function() { return sendRequest('context', 'getGeo', []); }
    },

    user: {
      getRole: function() { return sendRequest('user', 'getRole', []); },
      isAdmin: function() { return sendRequest('user', 'isAdmin', []); },
      isGuest: function() { return sendRequest('user', 'isGuest', []); },
      isLoggedIn: function() { return sendRequest('user', 'isLoggedIn', []); },
      getAllowedPermissionLevels: function() { return sendRequest('user', 'getAllowedPermissionLevels', []); },
      canUsePermissionLevel: function(level) { return sendRequest('user', 'canUsePermissionLevel', [level]); }
    },

    dom: ${DOM_HELPERS_CODE},

    file: {
      download: function(content, filename, mimeType) { return sendRequest('file', 'download', [{ content: content, filename: filename, mimeType: mimeType }]); }
    },

    assets: {
      list: function() { return sendRequest('assets', 'list', []); },
      get: function(path) { return sendRequest('assets', 'get', [path]); },
      getUrl: function(path) {
        if (typeof path !== 'string' || !path) return Promise.reject(new Error('Asset path is required'));
        var cached = _assetUrlByPath.get(path);
        if (cached) return Promise.resolve(cached);
        return sendRequest('assets', 'get', [path]).then(function(asset) {
          var bytes = decodeBase64ToBytes(asset.base64);
          var blob = new Blob([bytes], { type: asset.mimeType || 'application/octet-stream' });
          var url = URL.createObjectURL(blob);
          var entry = { url: url, mimeType: asset.mimeType, size: asset.size, path: path };
          _assetUrlByPath.set(path, entry);
          _assetUrls.add(url);
          return entry;
        });
      },
      getArrayBuffer: function(path) {
        return sendRequest('assets', 'get', [path]).then(function(asset) {
          var bytes = decodeBase64ToBytes(asset.base64);
          return {
            path: asset.path,
            mimeType: asset.mimeType,
            size: asset.size,
            buffer: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength)
          };
        });
      },
      revoke: function(url) {
        if (typeof url !== 'string') return;
        try { URL.revokeObjectURL(url); } catch (e) {}
        _assetUrls.delete(url);
        _assetUrlByPath.forEach(function(value, key) {
          if (value && value.url === url) _assetUrlByPath.delete(key);
        });
      },
      revokeAll: function() { revokeAllAssetUrls(); }
    }
  };

  // 冻结所有 API 对象（防止篡改）
  Object.freeze(Tapp);
  Object.freeze(Tapp.lifecycle);
  Object.freeze(Tapp.i18n);
  Object.freeze(Tapp.storage);
  Object.freeze(Tapp.dataExchange);
  Object.freeze(Tapp.settings);
  Object.freeze(Tapp.ai.tasks);
  Object.freeze(Tapp.ai);
  Object.freeze(Tapp.platform);
  Object.freeze(Tapp.report);
  Object.freeze(Tapp.background);
  Object.freeze(Tapp.scheduler);
  Object.freeze(Tapp.animation);
  Object.freeze(Tapp.speech);
  Object.freeze(Tapp.ui);
  Object.freeze(Tapp.api);
  Object.freeze(Tapp.media);
  Object.freeze(Tapp.context);
  Object.freeze(Tapp.user);
  Object.freeze(Tapp.dom);
  Object.freeze(Tapp.file);
  Object.freeze(Tapp.assets);

  // widgets/pages 容器保持可扩展：Widget 代码需要向其注册 render 定义
  // （Object.seal 会禁止新增属性，strict 模式下注册直接抛 TypeError）。

  // 防止重新定义 Tapp
  Object.defineProperty(window, 'Tapp', {
    value: Tapp,
    writable: false,
    configurable: false
  });

  console.log('[TappWidgetSDK] Initialized:', ${idLiteral});
})();
`
}
