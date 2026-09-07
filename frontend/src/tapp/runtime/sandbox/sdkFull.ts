/**
 * Page / headless Tapp SDK generator.
 *
 * Serializes grantedPermissions (授予), not declared or approved.
 */

import type { TappInstance } from '../../types'
import type { SandboxCapabilityProfile } from './capabilityProfiles'
import { ASSET_URL_HELPER_SOURCE } from './assetUrlRewriter'
import { serializeSandboxScriptValue } from './security'
import {
  DOM_HELPERS_CODE,
  FILE_DOWNLOAD_METHOD_CODE,
  generateStorageKeyValidator,
  sdkRequestTimeoutHelper,
} from './sdkShared'

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
  const gameTypeLiteral = serializeSandboxScriptValue(
    `game:${id}:${(manifest.game?.protocol || 'session').trim() || 'session'}`,
  )
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
  const _model3dUrlById = new Map();
  const _model3dUrls = new Set();
  ${ASSET_URL_HELPER_SOURCE}
  const snapshotAssetUrls = () => {
    const urls = {};
    _assetUrlByPath.forEach((entry, path) => {
      if (entry && entry.url) urls[path] = entry.url;
    });
    return urls;
  };
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
    _model3dUrls.forEach((url) => {
      try { URL.revokeObjectURL(url); } catch (e) {}
    });
    _model3dUrls.clear();
    _model3dUrlById.clear();
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

  // 事件缓冲区：缓存最新的有状态事件，新监听器注册时立即回放
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
  ${sdkRequestTimeoutHelper()}

  const sendRequest = (api, method, args = []) => {
    return new Promise((resolve, reject) => {
      const id = generateId();
      const timeout = setTimeout(() => {
        pendingRequests.delete(id);
        reject(new Error('Request timeout'));
      }, requestTimeoutMs(api, method));

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
          const err = new Error(message.payload?.error || 'Unknown error');
          if (message.payload?.code != null) err.code = message.payload.code;
          if (message.payload?.retryAfter != null) err.retryAfter = message.payload.retryAfter;
          pending.reject(err);
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
        // 强制触发重绘（WebKit 走保守路径）
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
          // 强制触发重绘（WebKit 走保守路径）
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
    // 回放缓冲区：如果已有该事件的最新值，立即调用回调
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
      invalidate: (reason, options) => sendRequest('widget', 'invalidateTarget', [reason, options]),
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

    analytics: {
      getSummary: (o) => sendRequest('analytics', 'getSummary', [o]),
      getVisitorCard: () => sendRequest('analytics', 'getVisitorCard', []),
    },

    model3d: {
      status: () => sendRequest('model3d', 'status', []),
      upload: (request) => sendRequest('model3d', 'upload', [request]),
      createTask: (request) => sendRequest('model3d', 'createTask', [request]),
      getTask: (taskId) => sendRequest('model3d', 'getTask', [taskId]),
      awaitTask: (taskId) => sendRequest('model3d', 'awaitTask', [taskId]),
      getUrl: async (assetId) => {
        if (typeof assetId !== 'string' || !assetId) throw new Error('Asset ID required');
        const cached = _model3dUrlById.get(assetId);
        if (cached) return cached;
        const asset = await sendRequest('model3d', 'getUrl', [assetId]);
        const bytes = decodeBase64ToBytes(asset.base64);
        const blob = new Blob([bytes], { type: asset.mimeType || 'model/gltf-binary' });
        const url = URL.createObjectURL(blob);
        const entry = { url: url, mimeType: asset.mimeType, size: asset.size, assetId: assetId };
        _model3dUrlById.set(assetId, entry);
        _model3dUrls.add(url);
        return entry;
      },
      getMetadata: (assetId) => sendRequest('model3d', 'getMetadata', [assetId]),
      revoke: (url) => {
        if (typeof url !== 'string') return;
        try { URL.revokeObjectURL(url); } catch (e) {}
        _model3dUrls.delete(url);
        _model3dUrlById.forEach((value, key) => {
          if (value && value.url === url) _model3dUrlById.delete(key);
        });
      },
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
      onChanged: (cb) => addEventListener('settingsChanged', cb),
    },

    shared: {
      get: (k) => { validateStorageKey(k); return sendRequest('shared', 'get', [k]); },
      set: (k, v) => { validateStorageKey(k); return sendRequest('shared', 'set', [k, v]); },
      remove: (k) => { validateStorageKey(k); return sendRequest('shared', 'remove', [k]); },
      keys: () => sendRequest('shared', 'keys', []),
      getAll: () => sendRequest('shared', 'getAll', []),
      clear: () => sendRequest('shared', 'clear', []),
      usage: () => sendRequest('shared', 'usage', []),
      onChanged: (cb) => addEventListener('sharedChanged', cb),
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
      // Declared allowlist only: { id, path?, query? } — never a free-form URL.
      openUrl: (req) => sendRequest('ui', 'openUrl', [req]),
      listOpenUrls: () => sendRequest('ui', 'listOpenUrls', []),
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

    persona: {
      get: () => sendRequest('persona', 'get', []),
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
      playTrack: (id, idx) =>
        sendRequest(
          'media',
          'playTrack',
          // Object form: full song snapshot (Aro share cards). Scalar form: playlist trackId/index.
          [
            id && typeof id === 'object'
              ? id
              : { trackId: id, trackIndex: idx },
          ],
        ),
      jumpToIndex: (idx) => sendRequest('media', 'jumpToIndex', [{ index: idx }]),
      loadNeteasePlaylist: (playlistId) => sendRequest('media', 'loadNeteasePlaylist', [{ playlistId }]),
      getSkipVip: () => sendRequest('media', 'getSkipVip', []),
      setSkipVip: (value) => sendRequest('media', 'setSkipVip', [{ value: value }]),
      onStateChange: (cb) => addEventListener('mediaStateChange', cb),
      onProgress: (cb) => addEventListener('mediaProgress', cb),
      // 频谱走宿主推送，别再每帧 getSpectrum：那样几秒就打满桥的入站限速，
      // 之后连 next/prev 都会被静默丢弃
      onSpectrum: (cb) => {
        const off = addEventListener('mediaSpectrum', cb);
        sendRequest('media', 'spectrumStream', [{ enabled: true }]).catch(() => {});
        return () => {
          off();
          sendRequest('media', 'spectrumStream', [{ enabled: false }]).catch(() => {});
        };
      },
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

    // Agent 交互 API - 允许 Tapp 与 Agent 进行数据交互
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
      ${FILE_DOWNLOAD_METHOD_CODE},
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
      getUrlMap: async () => {
        const paths = await sendRequest('assets', 'list', []);
        const map = {};
        if (!Array.isArray(paths)) return map;
        for (const path of paths) {
          const entry = await Tapp.assets.getUrl(path);
          map[path] = entry.url;
        }
        return map;
      },
      resolve: async (path) => {
        const direct = normalizeDeclaredAssetPath(path);
        if (direct) return Tapp.assets.getUrl(direct);
        await Tapp.assets.getUrlMap();
        const declared = resolveDeclaredAssetPath(path, snapshotAssetUrls());
        if (!declared) throw new Error('Asset path is required');
        return Tapp.assets.getUrl(declared);
      },
      rewriteUrl: (url) => rewriteAssetUrl(url, snapshotAssetUrls()) || url,
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
      // Explicit key rotation (confirm must be true)
      rotateKeys: (confirm) => sendRequest('federation', 'rotateKeys', [confirm]),
      // 时间线
      getFeed: () => sendRequest('federation', 'getFeed', []),
      /** Public posts from every instance sharing a group chat this one joined. */
      getRoomsFeed: () => sendRequest('federation', 'getRoomsFeed', []),
      getTimeline: () => sendRequest('federation', 'getTimeline', []),
      /** Resolve public object by id (quote click-through; no follow required). */
      getObject: (objectId) => sendRequest('federation', 'getObject', [objectId]),
      // 关注
      follow: (target) => sendRequest('federation', 'follow', [target]),
      unfollow: (target) => sendRequest('federation', 'unfollow', [target]),
      getFollowing: () => sendRequest('federation', 'getFollowing', []),
      getFollowers: () => sendRequest('federation', 'getFollowers', []),
      // 发布
      publish: (req) => sendRequest('federation', 'publish', [req]),
      createNote: (req) => sendRequest('federation', 'createNote', [req]),
      like: (objectId) => sendRequest('federation', 'like', [objectId]),
      unlike: (objectId) => sendRequest('federation', 'unlike', [objectId]),
      bookmark: (objectId) => sendRequest('federation', 'bookmark', [objectId]),
      unbookmark: (objectId) => sendRequest('federation', 'unbookmark', [objectId]),
      getBookmarks: () => sendRequest('federation', 'getBookmarks', []),
      announce: (objectId, content) => sendRequest('federation', 'announce', [objectId, content]),
      unannounce: (objectId) => sendRequest('federation', 'unannounce', [objectId]),
      /** X Web Intent compose only — returns intent_url; never posts server-side. */
      getExternalShareStatus: () =>
        sendRequest('federation', 'getExternalShareStatus', []),
      composeExternalShare: (req) =>
        sendRequest('federation', 'composeExternalShare', [req]),
      uploadMedia: (req) => sendRequest('federation', 'uploadMedia', [req]),
      unpublish: (req) => sendRequest('federation', 'unpublish', [req]),
      getPublished: () => sendRequest('federation', 'getPublished', []),
      // Channel
      getChannels: () => sendRequest('federation', 'getChannels', []),
      getChannel: (id) => sendRequest('federation', 'getChannel', [id]),
      createChannel: (req) => sendRequest('federation', 'createChannel', [req]),
      acceptChannel: (id) => sendRequest('federation', 'acceptChannel', [id]),
      closeChannel: (id) => sendRequest('federation', 'closeChannel', [id]),
      deleteChannel: (id) => sendRequest('federation', 'deleteChannel', [id]),
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
      acceptRoomInvite: (roomId) => sendRequest('federation', 'acceptRoomInvite', [roomId]),
      rejectRoomInvite: (roomId) => sendRequest('federation', 'rejectRoomInvite', [roomId]),
      removeMember: (roomId, actorUrl) => sendRequest('federation', 'removeMember', [roomId, actorUrl]),
      setMemberRole: (roomId, actorUrl, role) =>
        sendRequest('federation', 'setMemberRole', [roomId, actorUrl, role]),
      leaveRoom: (roomId) => sendRequest('federation', 'leaveRoom', [roomId]),
      transferRoomOwnership: (roomId, newOwner) =>
        sendRequest('federation', 'transferRoomOwnership', [roomId, newOwner]),
      initiateChannelE2e: (channelId) =>
        sendRequest('federation', 'initiateChannelE2e', [channelId]),
      initiateRoomE2e: (roomId) =>
        sendRequest('federation', 'initiateRoomE2e', [roomId]),
      addRoomSticker: (roomId, req) =>
        sendRequest('federation', 'addRoomSticker', [roomId, req]),
      removeRoomSticker: (roomId, stickerId) =>
        sendRequest('federation', 'removeRoomSticker', [roomId, stickerId]),
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
      updateTrustPolicy: (req) => sendRequest('federation', 'updateTrustPolicy', [req]),
      getInstances: () => sendRequest('federation', 'getInstances', []),
      getDeliveryStats: () => sendRequest('federation', 'getDeliveryStats', []),
      listDelivery: (limit) => sendRequest('federation', 'listDelivery', [limit]),
      retryDelivery: (id) => sendRequest('federation', 'retryDelivery', [id]),
      cancelDelivery: (id) => sendRequest('federation', 'cancelDelivery', [id]),
      retryAllDeadDelivery: (limit) => sendRequest('federation', 'retryAllDeadDelivery', [limit]),
      cancelAllPendingDelivery: (limit) =>
        sendRequest('federation', 'cancelAllPendingDelivery', [limit]),
      dismissDelivery: (id) => sendRequest('federation', 'dismissDelivery', [id]),
      purgeDeadDelivery: (opts) => sendRequest('federation', 'purgeDeadDelivery', [opts]),
      joinRoom: (roomId, opts) => sendRequest('federation', 'joinRoom', [roomId, opts]),
      updateInstanceTrust: (req) => sendRequest('federation', 'updateInstanceTrust', [req]),
      toggleInstanceBlock: (req) => sendRequest('federation', 'toggleInstanceBlock', [req]),
      // 文件传输
      initiateTransfer: (channelId, req) => sendRequest('federation', 'initiateTransfer', [channelId, req]),
      listTransfers: (channelId) => sendRequest('federation', 'listTransfers', [channelId]),
      initiateRoomTransfer: (roomId, req) => sendRequest('federation', 'initiateRoomTransfer', [roomId, req]),
      listRoomTransfers: (roomId) => sendRequest('federation', 'listRoomTransfers', [roomId]),
      listRoomFiles: (roomId, params) => sendRequest('federation', 'listRoomFiles', [roomId, params]),
      getTransfer: (transferId) => sendRequest('federation', 'getTransfer', [transferId]),
      downloadTransfer: (transferId) => sendRequest('federation', 'downloadTransfer', [transferId]),
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

    game: {
      create: (opts) => sendRequest('game', 'create', [opts || {}]),
      join: (shareId) => sendRequest('game', 'join', [shareId]),
      leave: (roomId) => sendRequest('game', 'leave', [roomId]),
      shareId: (room) => sendRequest('game', 'shareId', [room]),
      sendIntent: (roomId, body, seq) => sendRequest('game', 'sendIntent', [roomId, body, seq]),
      sendState: (roomId, body, seq) => sendRequest('game', 'sendState', [roomId, body, seq]),
      onMessage: (cb) => addEventListener('federation:message', (ev) => {
        const type = ev && ev.data && ev.data.message && ev.data.message.message_type;
        if (type === ${gameTypeLiteral}) cb(ev);
      }),
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
    Tapp.widget = {
      invalidate: Tapp.widget.invalidate,
    };
    delete Tapp.tappList;
    delete Tapp.component;
    delete Tapp.shortcut;
    delete Tapp.dynamicContent;
    delete Tapp.dom;
    delete Tapp.file;
    delete Tapp.model3d;
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
  if (Tapp.model3d) Object.freeze(Tapp.model3d);
  Object.freeze(Tapp.report);
  Object.freeze(Tapp.storage);
  Object.freeze(Tapp.dataExchange);
  Object.freeze(Tapp.settings);
  Object.freeze(Tapp.shared);
  Object.freeze(Tapp.ui);
  Object.freeze(Tapp.ui.fullscreen);
  Object.freeze(Tapp.data);
  Object.freeze(Tapp.api);
  Object.freeze(Tapp.context);
  Object.freeze(Tapp.persona);
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
  Object.freeze(Tapp.game);

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
