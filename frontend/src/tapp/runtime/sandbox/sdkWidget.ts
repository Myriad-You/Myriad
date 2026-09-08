/**
 * Widget Tapp SDK generator.
 *
 * Optional namespaces stay present as denied stubs when 授予 is missing.
 * Session token is per-instance and is not part of the template cache key.
 */

import type { TappInstance } from '../../types'
import { ASSET_URL_HELPER_SOURCE } from './assetUrlRewriter'
import {
  DOM_HELPERS_CODE,
  FILE_DOWNLOAD_METHOD_CODE,
  generateStorageKeyValidator,
  sdkRequestTimeoutHelper,
} from './sdkShared'
import { serializeSandboxScriptValue } from './security'

/**
 * Widget SDK 模板缓存：同一 Tapp（id/name/version/permissions/caps）只拼装一次大字符串，
 * 每个 iframe 仅替换会话 token。session token 必须每实例唯一，绝不能跨沙箱复用。
 */
const WIDGET_SDK_TOKEN_PLACEHOLDER = '__TAPP_WIDGET_SESSION_TOKEN__'
let widgetSdkTemplateCache: { key: string; body: string } | null = null

/** Optional Widget SDK namespaces driven by grantedPermissions (smaller srcdoc). */
export interface WidgetSdkCaps {
  ai: boolean
  platform: boolean
  analytics: boolean
  report: boolean
  media: boolean
  speech: boolean
  event: boolean
  agent: boolean
  scheduler: boolean
}

export function resolveWidgetSdkCaps(
  permissions: string[] | undefined | null,
): WidgetSdkCaps {
  const p = new Set(permissions || [])
  const has = (name: string) => p.has(name)
  return {
    ai:
      has('ai:generate') ||
      has('ai:analyze') ||
      has('ai:chat') ||
      has('ai:image') ||
      has('ai:search'),
    platform: has('platform:read'),
    analytics: has('analytics:read'),
    report: has('report:read'),
    media:
      has('media:read') || has('media:control') || has('media:audio'),
    speech: has('speech:tts') || has('speech:asr'),
    event: has('event:publish') || has('event:subscribe'),
    agent: has('component:agent'),
    scheduler: has('scheduler:register'),
  }
}

function capsFingerprint(caps: WidgetSdkCaps): string {
  return [
    caps.ai,
    caps.platform,
    caps.analytics,
    caps.report,
    caps.media,
    caps.speech,
    caps.event,
    caps.agent,
    caps.scheduler,
  ]
    .map((v) => (v ? '1' : '0'))
    .join('')
}

function buildWidgetSdkBody(
  idLiteral: string,
  nameLiteral: string,
  versionLiteral: string,
  tokenLiteral: string,
  permissionsLiteral: string,
  caps: WidgetSdkCaps,
): string {
  // Always keep optional namespaces present so existing Tapps that call
  // Tapp.ai / Tapp.media without a prior capability check get a clear
  // Permission denied Promise rejection (previous contract), not TypeError.
  // Full implementations only when granted — large API bodies omitted otherwise.
  const bufferedEvents = caps.media
    ? `{ mediaStateChange: 1, mediaProgress: 1, themeChange: 1, primaryColorChange: 1, localeChange: 1 }`
    : `{ themeChange: 1, primaryColorChange: 1, localeChange: 1 }`

  const deniedHelper = `
  var _denied = function(perm) {
    return function() {
      return Promise.reject(new Error('Permission denied: Missing permission: ' + perm));
    };
  };
`

  const aiNs = caps.ai
    ? `
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
    },`
    : `
    ai: {
      tasks: {
        create: _denied('ai:generate, ai:analyze, ai:chat, ai:image, ai:search'),
        get: _denied('ai:generate, ai:analyze, ai:chat, ai:image, ai:search'),
        cancel: _denied('ai:generate, ai:analyze, ai:chat, ai:image, ai:search'),
        usage: _denied('ai:generate, ai:analyze, ai:chat, ai:image, ai:search'),
        subscribe: _denied('ai:generate, ai:analyze, ai:chat, ai:image, ai:search')
      }
    },`

  const model3dNs = `
    model3d: {
      status: _denied('3d:generate'),
      upload: _denied('3d:generate'),
      createTask: _denied('3d:generate'),
      getTask: _denied('3d:generate'),
      awaitTask: _denied('3d:generate'),
      getUrl: _denied('3d:generate'),
      getMetadata: _denied('3d:generate'),
      revoke: function() {}
    },`

  const eventNs = caps.event
    ? `
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
    },`
    : `
    event: {
      publish: _denied('event:publish'),
      on: function() { throw new Error('Permission denied: Missing permission: event:subscribe'); }
    },`

  const agentNs = caps.agent
    ? `
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
    },`
    : `
    agent: {
      onInteraction: function() { throw new Error('Permission denied: Missing permission: component:agent'); }
    },`

  const mediaNs = caps.media
    ? `
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
      playTrack: function(id, idx) {
        return sendRequest('media', 'playTrack', [
          id && typeof id === 'object' ? id : { trackId: id, trackIndex: idx },
        ]);
      },
      jumpToIndex: function(idx) { return sendRequest('media', 'jumpToIndex', [{ index: idx }]); },
      loadNeteasePlaylist: function(playlistId) { return sendRequest('media', 'loadNeteasePlaylist', [{ playlistId: playlistId }]); },
      getSkipVip: function() { return sendRequest('media', 'getSkipVip', []); },
      setSkipVip: function(value) { return sendRequest('media', 'setSkipVip', [{ value: value }]); },
      onStateChange: function(cb) { return addEventListener('mediaStateChange', cb); },
      onProgress: function(cb) { return addEventListener('mediaProgress', cb); },
      onSpectrum: function(cb) {
        var off = addEventListener('mediaSpectrum', cb);
        var on = sendRequest('media', 'spectrumStream', [{ enabled: true }]);
        if (on && on.catch) on.catch(function() {});
        return function() {
          off();
          var offReq = sendRequest('media', 'spectrumStream', [{ enabled: false }]);
          if (offReq && offReq.catch) offReq.catch(function() {});
        };
      }
    },`
    : `
    media: {
      play: _denied('media:control'), pause: _denied('media:control'), next: _denied('media:control'),
      prev: _denied('media:control'), seek: _denied('media:control'), setVolume: _denied('media:control'),
      setMode: _denied('media:control'), mute: _denied('media:control'), unmute: _denied('media:control'),
      getStatus: _denied('media:read'), getPlaylist: _denied('media:read'), getSpectrum: _denied('media:read'),
      getLyrics: _denied('media:read'), getBeatGrid: _denied('media:read'), playTrack: _denied('media:control'),
      jumpToIndex: _denied('media:control'), loadNeteasePlaylist: _denied('media:control'),
      getSkipVip: _denied('media:read'), setSkipVip: _denied('media:control'),
      onStateChange: function() { return function() {}; },
      onProgress: function() { return function() {}; },
      onSpectrum: function() { return function() {}; }
    },`

  const platformNs = caps.platform
    ? `
    platform: {
      listEnabled: function() { return sendRequest('platform', 'listEnabled', []); },
      getData: function(p, o) { return sendRequest('platform', 'getData', [p, o]); },
      getStats: function(p) { return sendRequest('platform', 'getStats', [p]); },
      getDistribution: function(p, d) { return sendRequest('platform', 'getDistribution', [p, d]); }
    },`
    : `
    platform: {
      listEnabled: _denied('platform:read'), getData: _denied('platform:read'),
      getStats: _denied('platform:read'), getDistribution: _denied('platform:read')
    },`

  const analyticsNs = caps.analytics
    ? `
    analytics: {
      getSummary: function(o) { return sendRequest('analytics', 'getSummary', [o]); },
      getVisitorCard: function() { return sendRequest('analytics', 'getVisitorCard', []); }
    },`
    : `
    analytics: {
      getSummary: _denied('analytics:read'),
      getVisitorCard: _denied('analytics:read')
    },`

  const reportNs = caps.report
    ? `
    report: {
      listReports: function() { return sendRequest('report', 'listReports', []); },
      getReport: function(id) { return sendRequest('report', 'getReport', [id]); },
      getPlatformReport: function(p) { return sendRequest('report', 'getPlatformReport', [p]); },
      list: function() { return sendRequest('report', 'list', []); },
      get: function(id) { return sendRequest('report', 'get', [{ reportId: id }]); }
    },`
    : `
    report: {
      listReports: _denied('report:read'), getReport: _denied('report:read'),
      getPlatformReport: _denied('report:read'), list: _denied('report:read'), get: _denied('report:read')
    },`

  const schedulerNs = caps.scheduler
    ? `
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
    },`
    : `
    scheduler: {
      register: _denied('scheduler:register'), unregister: _denied('scheduler:register'),
      list: _denied('scheduler:register'), get: _denied('scheduler:register'),
      enable: _denied('scheduler:register'), disable: _denied('scheduler:register'),
      trigger: _denied('scheduler:register'),
      onTask: function() { throw new Error('Permission denied: Missing permission: scheduler:register'); }
    },`

  const speechNs = caps.speech
    ? `
    speech: {
      tts: function(r) { return sendRequest('speech', 'tts', [r]); },
      getVoices: function() { return sendRequest('speech', 'getVoices', []); },
      getStatus: function() { return sendRequest('speech', 'getStatus', []); },
      asr: function(r) { return sendRequest('speech', 'asr', [r]); }
    },`
    : `
    speech: {
      tts: _denied('speech:tts'), getVoices: _denied('speech:tts'),
      getStatus: _denied('speech:tts'), asr: _denied('speech:asr')
    },`

  // Always freeze optional namespaces (full or stub) so shape stays stable.
  const freezeOptional = [
    'Object.freeze(Tapp.ai.tasks); Object.freeze(Tapp.ai);',
    'Object.freeze(Tapp.model3d);',
    'Object.freeze(Tapp.platform);',
    'Object.freeze(Tapp.analytics);',
    'Object.freeze(Tapp.report);',
    'Object.freeze(Tapp.scheduler);',
    'Object.freeze(Tapp.speech);',
    'Object.freeze(Tapp.media);',
    'Object.freeze(Tapp.event);',
    'Object.freeze(Tapp.agent);',
  ].join('\n  ')

  return `
(function() {
  'use strict';

  // 会话 token（用于消息验证）
  var _SESSION_TOKEN = ${tokenLiteral};

  var messageIdCounter = 0;
  var pendingRequests = new Map();
  var eventListeners = new Map();
  var dataExchangeProviders = new Map();
  // 添加生命周期回调支持
  var lifecycleCallbacks = { destroy: [], pause: [], resume: [] };
  var lifecycleDestroyed = false;
  var _assetUrlByPath = new Map();
  var _assetUrls = new Set();
  ${ASSET_URL_HELPER_SOURCE}
  var snapshotAssetUrls = function() {
    var urls = {};
    _assetUrlByPath.forEach(function(entry, path) {
      if (entry && entry.url) urls[path] = entry.url;
    });
    return urls;
  };
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

  // security wrapper 会收窄 window.parent。接收包装前的真实 WindowProxy，
  // 用于发送消息与校验宿主响应来源（与 Page SDK 一致）。
  var _HOST_WINDOW = (function() {
    var takeNativeParent = window.__TAPP_TAKE_NATIVE_PARENT__;
    var hostWindow = typeof takeNativeParent === 'function'
      ? takeNativeParent()
      : window.parent;
    try { delete window.__TAPP_TAKE_NATIVE_PARENT__; } catch (e) {}
    return hostWindow;
  })();

  // 事件缓冲区：缓存最新的有状态事件，新监听器注册时立即回放
  var _eventBuffer = new Map();
  var _BUFFERED_EVENTS = ${bufferedEvents};
  var _ACTION_TO_EVENT = { 'theme:change': 'themeChange', 'locale:change': 'localeChange', 'primaryColor:change': 'primaryColorChange' };

  var generateId = function() { return 'widget-' + (++messageIdCounter) + '-' + Date.now(); };

  ${generateStorageKeyValidator()}
  ${sdkRequestTimeoutHelper()}

  var sendRequest = function(api, method, args) {
    args = args || [];
    return new Promise(function(resolve, reject) {
      var id = generateId();
      var timeout = setTimeout(function() { pendingRequests.delete(id); reject(new Error('Request timeout')); }, requestTimeoutMs(api, method));
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
    // 回放缓冲区：如果已有该事件的最新值，立即调用回调
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
      if (payload.success) {
        pending.resolve(payload.data);
      } else {
        var err = new Error(payload.error || 'Request failed');
        if (payload.code != null) err.code = payload.code;
        if (payload.retryAfter != null) err.retryAfter = payload.retryAfter;
        pending.reject(err);
      }
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
      // 缓存有状态事件的最新值（供 addEventListener 回放，统一 camelCase key）
      var _bufKey = _ACTION_TO_EVENT[msg.action] || msg.action;
      if (_BUFFERED_EVENTS[_bufKey]) {
        _eventBuffer.set(_bufKey, msg.payload);
      }
      // 强制重绘辅助函数：WebKit 专用沙箱会设置 window._TAPP_DISABLE_TRANSFORM_REPAINT
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
        // 强制触发重绘
        forceRepaint();
      }
      // 主色调变化事件
      else if (msg.action === 'primaryColor:change') {
        eventListeners.get('primaryColorChange')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
        // 更新 CSS 变量
        if (msg.payload) {
          document.documentElement.style.setProperty('--tapp-primary', msg.payload);
          // 强制触发重绘
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
      // 生命周期暂停事件（页面不可见时触发）
      else if (msg.action === 'lifecycle:pause') {
        lifecycleCallbacks.pause.forEach(function(cb) { try { cb(); } catch(e) {} });
        eventListeners.get('pause')?.forEach(function(cb) { try { cb(); } catch(e) {} });
      }
      // 生命周期恢复事件（页面重新可见时触发）
      else if (msg.action === 'lifecycle:resume') {
        lifecycleCallbacks.resume.forEach(function(cb) { try { cb(); } catch(e) {} });
        eventListeners.get('resume')?.forEach(function(cb) { try { cb(); } catch(e) {} });
      }
      // 媒体状态变化事件
      else if (msg.action === 'mediaStateChange') {
        eventListeners.get('mediaStateChange')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
      }
      // 媒体进度实时推送
      else if (msg.action === 'mediaProgress') {
        eventListeners.get('mediaProgress')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
      }
      else if (msg.action === 'storageChanged') {
        eventListeners.get('storageChanged')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
      }
      else if (msg.action === 'settingsChanged') {
        eventListeners.get('settingsChanged')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
      }
      else if (msg.action === 'sharedChanged') {
        eventListeners.get('sharedChanged')?.forEach(function(cb) { try { cb(msg.payload); } catch(e) {} });
      }
    }
  });

  ${deniedHelper}
  var currentLocale = typeof window._TAPP_LOCALE === 'string'
    ? window._TAPP_LOCALE
    : (typeof document !== 'undefined' && document.documentElement.lang)
      || (typeof navigator !== 'undefined' && navigator.language)
      || 'en-US';
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
      invalidate: function(reason, options) {
        if (options == null) {
          return sendRequest('widget', 'invalidate', [reason]);
        }
        return sendRequest('widget', 'invalidateTarget', [reason, options]);
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
      getAll: function() { return sendRequest('settings', 'getAll', []); },
      onChanged: function(cb) { return addEventListener('settingsChanged', cb); }
    },

    shared: {
      get: function(k) { validateStorageKey(k); return sendRequest('shared', 'get', [k]); },
      set: function(k, v) { validateStorageKey(k); return sendRequest('shared', 'set', [k, v]); },
      remove: function(k) { validateStorageKey(k); return sendRequest('shared', 'remove', [k]); },
      keys: function() { return sendRequest('shared', 'keys', []); },
      getAll: function() { return sendRequest('shared', 'getAll', []); },
      clear: function() { return sendRequest('shared', 'clear', []); },
      usage: function() { return sendRequest('shared', 'usage', []); },
      onChanged: function(cb) { return addEventListener('sharedChanged', cb); }
    },
${aiNs}${model3dNs}${eventNs}${agentNs}${mediaNs}${platformNs}${analyticsNs}${reportNs}
    background: {
      require: function(r, reason) { return sendRequest('background', 'require', [r, reason]); },
      release: function(r) { return sendRequest('background', 'release', [r]); },
      list: function() { return sendRequest('background', 'list', []); },
      has: function(r) { return sendRequest('background', 'has', [r]); }
    },
${schedulerNs}
    animation: {
      getLevel: function() { return sendRequest('animation', 'getLevel', []); },
      shouldAnimate: function() { return sendRequest('animation', 'shouldAnimate', []); },
      getConfig: function() { return sendRequest('animation', 'getConfig', []); },
      getStaggerDelay: function(i, d) { return sendRequest('animation', 'getStaggerDelay', [i, d]); },
      onLevelChange: function(cb) { return addEventListener('animationLevelChange', cb); }
    },
${speechNs}
    ui: {
      getTheme: function() { return sendRequest('ui', 'getTheme', []); },
      getPrimaryColor: function() { return sendRequest('ui', 'getPrimaryColor', []); },
      getLocale: function() { return sendRequest('ui', 'getLocale', []); },
      showNotification: function(o) { return sendRequest('ui', 'showNotification', [o]); },
      openUrl: function(req) { return sendRequest('ui', 'openUrl', [req]); },
      listOpenUrls: function() { return sendRequest('ui', 'listOpenUrls', []); },
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

    persona: {
      get: function() { return sendRequest('persona', 'get', []); }
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
      ${FILE_DOWNLOAD_METHOD_CODE}
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
      getUrlMap: function() {
        return sendRequest('assets', 'list', []).then(function(paths) {
          var map = {};
          if (!Array.isArray(paths)) return map;
          var chain = Promise.resolve();
          paths.forEach(function(path) {
            chain = chain.then(function() {
              return Tapp.assets.getUrl(path).then(function(entry) {
                map[path] = entry.url;
              });
            });
          });
          return chain.then(function() { return map; });
        });
      },
      resolve: function(path) {
        var direct = normalizeDeclaredAssetPath(path);
        if (direct) return Tapp.assets.getUrl(direct);
        return Tapp.assets.getUrlMap().then(function() {
          var declared = resolveDeclaredAssetPath(path, snapshotAssetUrls());
          if (!declared) return Promise.reject(new Error('Asset path is required'));
          return Tapp.assets.getUrl(declared);
        });
      },
      rewriteUrl: function(url) {
        return rewriteAssetUrl(url, snapshotAssetUrls()) || url;
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

  // Freeze exposed API objects (prevent tampering). Optional namespaces always
  // exist as full impl or denied stubs — freezeOptional covers both so shape
  // stays stable regardless of grantedPermissions.
  Object.freeze(Tapp);
  Object.freeze(Tapp.lifecycle);
  Object.freeze(Tapp.i18n);
  Object.freeze(Tapp.storage);
  Object.freeze(Tapp.dataExchange);
  Object.freeze(Tapp.settings);
  Object.freeze(Tapp.shared);
  Object.freeze(Tapp.background);
  Object.freeze(Tapp.animation);
  Object.freeze(Tapp.ui);
  Object.freeze(Tapp.api);
  Object.freeze(Tapp.context);
  Object.freeze(Tapp.persona);
  Object.freeze(Tapp.user);
  Object.freeze(Tapp.dom);
  Object.freeze(Tapp.file);
  Object.freeze(Tapp.assets);
  ${freezeOptional}

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
  const permissionsLiteral = serializeSandboxScriptValue(
    grantedPermissions || [],
  )
  const caps = resolveWidgetSdkCaps(grantedPermissions)
  const cacheKey = `${id}\0${manifest.name}\0${manifest.version}\0${permissionsLiteral}\0${capsFingerprint(caps)}`
  const placeholderLiteral = serializeSandboxScriptValue(
    WIDGET_SDK_TOKEN_PLACEHOLDER,
  )

  if (!widgetSdkTemplateCache || widgetSdkTemplateCache.key !== cacheKey) {
    widgetSdkTemplateCache = {
      key: cacheKey,
      body: buildWidgetSdkBody(
        idLiteral,
        nameLiteral,
        versionLiteral,
        placeholderLiteral,
        permissionsLiteral,
        caps,
      ),
    }
  }

  // 每实例替换会话 token（安全：token 不进缓存，不跨沙箱共享）
  return widgetSdkTemplateCache.body.replace(
    placeholderLiteral,
    serializeSandboxScriptValue(token),
  )
}
