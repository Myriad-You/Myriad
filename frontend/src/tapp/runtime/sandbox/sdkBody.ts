/** Page / Widget / Headless 共用一份消息环；各面只拼自己的命名空间。 */

import type { SandboxCapabilityProfile } from './capabilityProfiles'
import { ASSET_URL_HELPER_SOURCE } from './assetUrlRewriter'
import {
  DOM_HELPERS_CODE,
  FILE_DOWNLOAD_METHOD_CODE,
  generateKvNamespaceCode,
  generateSettingsNamespaceCode,
  generateStorageKeyValidator,
  SDK_FREEZE_TAPP_CODE,
  SDK_HOST_CHROME_CODE,
  sdkRequestTimeoutHelper,
} from './sdkShared'

export type SdkSurface = SandboxCapabilityProfile

/** 按授予权限裁剪 Widget 可选命名空间。 */
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

const ALL_WIDGET_CAPS: WidgetSdkCaps = {
  ai: true,
  platform: true,
  analytics: true,
  report: true,
  media: true,
  speech: true,
  event: true,
  agent: true,
  scheduler: true,
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
    media: has('media:read') || has('media:control') || has('media:audio'),
    speech: has('speech:tts') || has('speech:asr'),
    event: has('event:publish') || has('event:subscribe'),
    agent: has('component:agent'),
    scheduler: has('scheduler:register'),
  }
}

export interface GenerateSdkBodyInput {
  surface: SdkSurface
  idLiteral: string
  nameLiteral: string
  versionLiteral: string
  tokenLiteral: string
  permissionsLiteral: string
  gameTypeLiteral: string
  caps?: WidgetSdkCaps
}

function when(enabled: boolean, code: string): string {
  return enabled ? code : ''
}

function cap(
  surface: SdkSurface,
  enabled: boolean,
  live: string,
  denied: string,
): string {
  if (surface !== 'widget') return live
  return enabled ? live : denied
}

const DENIED_HELPER = `
  const _denied = (perm) => () =>
    Promise.reject(new Error('Permission denied: Missing permission: ' + perm));
  const _unavailable = (name) => () =>
    Promise.reject(new Error(name + ' is not available in the widget sandbox'));
  const _deniedThrow = (perm) => () => {
    throw new Error('Permission denied: Missing permission: ' + perm);
  };
`

function aiNamespace(surface: SdkSurface, enabled: boolean): string {
  return cap(
    surface,
    enabled,
    `
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
    },`,
    `
    ai: {
      tasks: {
        create: _denied('ai:generate, ai:analyze, ai:chat, ai:image, ai:search'),
        get: _denied('ai:generate, ai:analyze, ai:chat, ai:image, ai:search'),
        cancel: _denied('ai:generate, ai:analyze, ai:chat, ai:image, ai:search'),
        usage: _denied('ai:generate, ai:analyze, ai:chat, ai:image, ai:search'),
        subscribe: _denied('ai:generate, ai:analyze, ai:chat, ai:image, ai:search'),
      },
    },`,
  )
}

function eventNamespace(surface: SdkSurface, enabled: boolean): string {
  return cap(
    surface,
    enabled,
    `
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
    },`,
    `
    event: {
      publish: _denied('event:publish'),
      on: () => { throw new Error('Permission denied: Missing permission: event:subscribe'); },
    },`,
  )
}

function agentNamespace(surface: SdkSurface, enabled: boolean): string {
  return cap(
    surface,
    enabled,
    `
    agent: {
      onInteraction: (type, callback) => {
        if (typeof type !== 'string' || typeof callback !== 'function') {
          throw new Error('interaction type and callback are required');
        }
        return addEventListener('agentInteractionV2', (raw) => {
          if (raw?.type !== type) return;
          callback({
            ...raw,
            accept: () => sendRequest('agent', 'v2.accept', [raw.interactionId]),
            submitResult: (result) => sendRequest('agent', 'v2.result', [raw.interactionId, {
              ...result,
              idempotencyKey: result?.idempotencyKey || ('result-' + raw.interactionId),
            }]),
            reject: (reason) => sendRequest('agent', 'v2.reject', [raw.interactionId, reason]),
            requestIntent: (request) => sendRequest('agent', 'v2.intent', [raw.interactionId, request]),
          });
        });
      },
    },`,
    `
    agent: {
      onInteraction: () => { throw new Error('Permission denied: Missing permission: component:agent'); },
    },`,
  )
}

function mediaLive(): string {
  return `
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
      playTrack: (id, idx) => sendRequest('media', 'playTrack', [
        id && typeof id === 'object' ? id : { trackId: id, trackIndex: idx },
      ]),
      jumpToIndex: (idx) => sendRequest('media', 'jumpToIndex', [{ index: idx }]),
      loadNeteasePlaylist: (playlistId) => sendRequest('media', 'loadNeteasePlaylist', [{ playlistId }]),
      getSkipVip: () => sendRequest('media', 'getSkipVip', []),
      setSkipVip: (value) => sendRequest('media', 'setSkipVip', [{ value }]),
      onStateChange: (cb) => addEventListener('mediaStateChange', cb),
      onProgress: (cb) => addEventListener('mediaProgress', cb),
      onSpectrum: (cb) => {
        const off = addEventListener('mediaSpectrum', cb);
        spectrumStreamSubscribers += 1;
        if (spectrumStreamSubscribers === 1) {
          sendRequest('media', 'spectrumStream', [{ enabled: true }]).catch(() => {});
        }
        let active = true;
        return () => {
          if (!active) return;
          active = false;
          off();
          spectrumStreamSubscribers = Math.max(0, spectrumStreamSubscribers - 1);
          if (spectrumStreamSubscribers === 0) {
            sendRequest('media', 'spectrumStream', [{ enabled: false }]).catch(() => {});
          }
        };
      },
    },`
}

function mediaNamespace(surface: SdkSurface, enabled: boolean): string {
  return cap(
    surface,
    enabled,
    mediaLive(),
    `
    media: {
      play: _denied('media:control'), pause: _denied('media:control'), next: _denied('media:control'),
      prev: _denied('media:control'), seek: _denied('media:control'), setVolume: _denied('media:control'),
      setMode: _denied('media:control'), mute: _denied('media:control'), unmute: _denied('media:control'),
      getStatus: _denied('media:read'), getPlaylist: _denied('media:read'), getSpectrum: _denied('media:read'),
      getLyrics: _denied('media:read'), getBeatGrid: _denied('media:read'), playTrack: _denied('media:control'),
      jumpToIndex: _denied('media:control'), loadNeteasePlaylist: _denied('media:control'),
      getSkipVip: _denied('media:read'), setSkipVip: _denied('media:control'),
      onStateChange: _deniedThrow('media:read'),
      onProgress: _deniedThrow('media:read'),
      onSpectrum: _deniedThrow('media:read'),
    },`,
  )
}

function platformNamespace(surface: SdkSurface, enabled: boolean): string {
  const read = `
    platform: {
      listEnabled: () => sendRequest('platform', 'listEnabled', []),
      getData: (p, o) => sendRequest('platform', 'getData', [p, o]),
      getStats: (p) => sendRequest('platform', 'getStats', [p]),
      getDistribution: (p, d) => sendRequest('platform', 'getDistribution', [p, d])`
  const widgetWrites = `
      addItem: _unavailable('platform.addItem'),
      addItems: _unavailable('platform.addItems'),
      registerPlatform: _unavailable('platform.registerPlatform')`
  if (surface === 'widget') {
    return enabled
      ? `${read},${widgetWrites}
    },`
      : `
    platform: {
      listEnabled: _denied('platform:read'), getData: _denied('platform:read'),
      getStats: _denied('platform:read'), getDistribution: _denied('platform:read'),${widgetWrites}
    },`
  }
  return `${read},
      addItem: (d) => sendRequest('platform', 'addItem', [d]),
      addItems: (i) => sendRequest('platform', 'addItems', [i]),
      registerPlatform: (c) => sendRequest('platform', 'registerPlatform', [c]),
    },`
}

function analyticsNamespace(surface: SdkSurface, enabled: boolean): string {
  return cap(
    surface,
    enabled,
    `
    analytics: {
      getSummary: (o) => sendRequest('analytics', 'getSummary', [o]),
      getVisitorCard: () => sendRequest('analytics', 'getVisitorCard', []),
    },`,
    `
    analytics: {
      getSummary: _denied('analytics:read'),
      getVisitorCard: _denied('analytics:read'),
    },`,
  )
}

function reportNamespace(surface: SdkSurface, enabled: boolean): string {
  const platformLive = `
      platform: {
        list: () => sendRequest('report', 'platform.list', []),
        get: (id) => sendRequest('report', 'platform.get', [id]),
        byPlatform: (p) => sendRequest('report', 'platform.byPlatform', [p]),
      }`
  const platformDenied = `
      platform: {
        list: _denied('report:read'),
        get: _denied('report:read'),
        byPlatform: _denied('report:read'),
      }`
  const mine = `
      list: () => sendRequest('report', 'list', []),
      get: (id) => sendRequest('report', 'get', [{ reportId: id }])`
  const widgetWrites = `
      create: _unavailable('report.create'),
      update: _unavailable('report.update'),
      delete: _unavailable('report.delete')`
  if (surface === 'widget') {
    return enabled
      ? `
    report: {
${platformLive},
${mine},${widgetWrites}
    },`
      : `
    report: {
${platformDenied},
      list: _denied('report:read'), get: _denied('report:read'),${widgetWrites}
    },`
  }
  return `
    report: {
${platformLive},
${mine},
      create: (t, rt, c, m) => sendRequest('report', 'create', [
        t && typeof t === 'object' && !Array.isArray(t)
          ? t
          : { title: t, reportType: rt, content: c, metadata: m },
      ]),
      update: (id, t, c, m) => sendRequest('report', 'update', [{ reportId: id, title: t, content: c, metadata: m }]),
      delete: (id) => sendRequest('report', 'delete', [{ reportId: id }]),
    },`
}

function schedulerNamespace(surface: SdkSurface, enabled: boolean): string {
  return cap(
    surface,
    enabled,
    `
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
        let active = true;
        let removeListener = () => {};
        sendRequest('scheduler', 'subscribe', [taskId]).then(
          () => {
            if (!active) {
              sendRequest('scheduler', 'unsubscribe', [taskId]).catch(() => {});
              return;
            }
            removeListener = addEventListener('schedulerTask', (d) => {
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
          },
          () => {},
        );
        return () => {
          if (!active) return;
          active = false;
          removeListener();
          sendRequest('scheduler', 'unsubscribe', [taskId]).catch(() => {});
        };
      },
    },`,
    `
    scheduler: {
      register: _denied('scheduler:register'), unregister: _denied('scheduler:register'),
      list: _denied('scheduler:register'), get: _denied('scheduler:register'),
      enable: _denied('scheduler:register'), disable: _denied('scheduler:register'),
      trigger: _denied('scheduler:register'),
      onTask: () => { throw new Error('Permission denied: Missing permission: scheduler:register'); },
    },`,
  )
}

function speechNamespace(surface: SdkSurface, enabled: boolean): string {
  return cap(
    surface,
    enabled,
    `
    speech: {
      tts: (r) => sendRequest('speech', 'tts', [r]),
      getVoices: () => sendRequest('speech', 'getVoices', []),
      getStatus: () => sendRequest('speech', 'getStatus', []),
      asr: (r) => sendRequest('speech', 'asr', [r]),
    },`,
    `
    speech: {
      tts: _denied('speech:tts'), getVoices: _denied('speech:tts'),
      getStatus: _denied('speech:tts'), asr: _denied('speech:asr'),
    },`,
  )
}

function model3dLive(): string {
  return `
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
        try { URL.revokeObjectURL(url); } catch {}
        _model3dUrls.delete(url);
        _model3dUrlById.forEach((value, key) => {
          if (value && value.url === url) _model3dUrlById.delete(key);
        });
      },
    },`
}

function model3dNamespace(surface: SdkSurface): string {
  if (surface !== 'page') return ''
  return model3dLive()
}

function uiNamespace(surface: SdkSurface): string {
  const theme = `
      getTheme: () => sendRequest('ui', 'getTheme', []),
      onThemeChange: (cb) => addEventListener('themeChange', cb),
      getPrimaryColor: () => sendRequest('ui', 'getPrimaryColor', []),
      onPrimaryColorChange: (cb) => addEventListener('primaryColorChange', cb),
      getLocale: () => sendRequest('ui', 'getLocale', []),
      onLocaleChange: (cb) => addEventListener('localeChange', cb),
      showNotification: (o) => sendRequest('ui', 'showNotification', [o])`
  if (surface === 'headless') {
    return `
    ui: {${theme},
    },`
  }
  if (surface === 'widget') {
    return `
    ui: {${theme},
      openUrl: (req) => sendRequest('ui', 'openUrl', [req]),
      listOpenUrls: () => sendRequest('ui', 'listOpenUrls', []),
    },`
  }
  return `
    ui: {
      setTitle: (t) => sendRequest('ui', 'setTitle', [t]),${theme},
      confirm: (m) => sendRequest('ui', 'confirm', [m]),
      openUrl: (req) => sendRequest('ui', 'openUrl', [req]),
      listOpenUrls: () => sendRequest('ui', 'listOpenUrls', []),
      fullscreen: {
        request: () => sendRequest('ui', 'fullscreen.request', []),
        exit: () => sendRequest('ui', 'fullscreen.exit', []),
        toggle: () => sendRequest('ui', 'fullscreen.toggle', []),
        isFullscreen: () => sendRequest('ui', 'fullscreen.isFullscreen', []),
      },
    },`
}

function widgetNamespace(surface: SdkSurface): string {
  if (surface === 'widget') {
    return `
    widget: {
      getInstanceSettings: () => ({ ...((window._TAPP_WIDGET_PROPS && window._TAPP_WIDGET_PROPS.config) || {}) }),
      updateInstanceSettings: (patch) => sendRequest('widget', 'instanceSettings.update', [patch]),
      invalidate: (reason, options) => {
        if (options == null) return sendRequest('widget', 'invalidate', [reason]);
        return sendRequest('widget', 'invalidateTarget', [reason, options]);
      },
    },`
  }
  if (surface === 'headless') {
    return `
    widget: {
      invalidate: (reason, options) => sendRequest('widget', 'invalidateTarget', [reason, options]),
    },`
  }
  return `
    widget: {
      register: (cfg) => sendRequest('widget', 'register', [cfg]),
      unregister: (id) => sendRequest('widget', 'unregister', [id]),
      listRegistered: () => sendRequest('widget', 'listRegistered', []),
      updateConfig: (id, cfg) => sendRequest('widget', 'updateConfig', [id, cfg]),
      invalidate: (reason, options) => sendRequest('widget', 'invalidateTarget', [reason, options]),
    },`
}

function lifecycleNamespace(
  surface: SdkSurface,
  idLiteral: string,
  nameLiteral: string,
  versionLiteral: string,
  permissionsLiteral: string,
): string {
  if (surface === 'widget') {
    return `
    lifecycle: {
      onReady: (cb) => {
        if (document.readyState === 'complete') setTimeout(cb, 0);
        else window.addEventListener('load', cb);
      },
      onDestroy: (cb) => lifecycleCallbacks.destroy.push(cb),
      onPause: (cb) => lifecycleCallbacks.pause.push(cb),
      onResume: (cb) => lifecycleCallbacks.resume.push(cb),
    },`
  }
  return `
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
    },`
}

function pageHostNamespaces(gameTypeLiteral: string): string {
  return `
    tappList: {
      list: () => sendRequest('tappList', 'list', []),
      get: (id) => sendRequest('tappList', 'get', [id]),
      getRecent: (limit) => sendRequest('tappList', 'getRecent', [limit]),
      getInstallPackage: (id, opts) => sendRequest('tappList', 'getInstallPackage', [id, opts]),
      resolveStoreSource: (id) => sendRequest('tappList', 'resolveStoreSource', [id]),
      install: (req) => sendRequest('tappList', 'install', [req]),
      uninstall: (id) => sendRequest('tappList', 'uninstall', [id]),
      start: (id) => sendRequest('tappList', 'start', [id]),
      stop: (id) => sendRequest('tappList', 'stop', [id]),
      export: (id) => sendRequest('tappList', 'export', [id]),
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
    dynamicContent: {
      set: (c) => sendRequest('dynamicContent', 'set', [c]),
      update: (u) => sendRequest('dynamicContent', 'update', [u]),
      get: () => sendRequest('dynamicContent', 'get', []),
      remove: () => sendRequest('dynamicContent', 'remove', []),
    },
    data: { transform: (r) => sendRequest('data', 'transform', [r]) },
${brewListNamespace()}
${federationNamespace()}
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
    },`
}

function brewAndFederation(gameTypeLiteral: string): string {
  return `
${brewListNamespace()}
${federationNamespace()}
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
    data: { transform: (r) => sendRequest('data', 'transform', [r]) },`
}

function brewListNamespace(): string {
  return `
    brewList: {
      list: (o) => sendRequest('brewList', 'list', [o]),
      get: (id) => sendRequest('brewList', 'get', [id]),
      sources: () => sendRequest('brewList', 'sources', []),
      categories: () => sendRequest('brewList', 'categories', []),
      stats: () => sendRequest('brewList', 'stats', []),
      discover: (url) => sendRequest('brewList', 'discover', [url]),
      exportOpml: () => sendRequest('brewList', 'exportOpml', []),
      markRead: (id) => sendRequest('brewList', 'markRead', [id]),
      markUnread: (id) => sendRequest('brewList', 'markUnread', [id]),
      star: (id) => sendRequest('brewList', 'star', [id]),
      unstar: (id) => sendRequest('brewList', 'unstar', [id]),
      markAllRead: (o) => sendRequest('brewList', 'markAllRead', [o]),
      getComments: (itemId) => sendRequest('brewList', 'getComments', [itemId]),
      createComment: (itemId, req) => sendRequest('brewList', 'createComment', [itemId, req]),
      updateComment: (commentId, req) => sendRequest('brewList', 'updateComment', [commentId, req]),
      deleteComment: (commentId) => sendRequest('brewList', 'deleteComment', [commentId]),
      getReplies: (commentId) => sendRequest('brewList', 'getReplies', [commentId]),
      createReply: (itemId, parentId, content) => sendRequest('brewList', 'createReply', [itemId, parentId, content]),
      addSource: (req) => sendRequest('brewList', 'addSource', [req]),
      updateSource: (id, req) => sendRequest('brewList', 'updateSource', [id, req]),
      deleteSource: (id) => sendRequest('brewList', 'deleteSource', [id]),
      refreshSource: (id) => sendRequest('brewList', 'refreshSource', [id]),
      importOpml: (opml) => sendRequest('brewList', 'importOpml', [opml]),
      createCategory: (req) => sendRequest('brewList', 'createCategory', [req]),
      deleteCategory: (id) => sendRequest('brewList', 'deleteCategory', [id]),
    },`
}

function federationNamespace(): string {
  return `
    federation: {
      getIdentity: () => sendRequest('federation', 'getIdentity', []),
      rotateKeys: (confirm) => sendRequest('federation', 'rotateKeys', [confirm]),
      getFeed: () => sendRequest('federation', 'getFeed', []),
      getRoomsFeed: () => sendRequest('federation', 'getRoomsFeed', []),
      getTimeline: () => sendRequest('federation', 'getTimeline', []),
      getObject: (objectId) => sendRequest('federation', 'getObject', [objectId]),
      follow: (target) => sendRequest('federation', 'follow', [target]),
      unfollow: (target) => sendRequest('federation', 'unfollow', [target]),
      getFollowing: () => sendRequest('federation', 'getFollowing', []),
      getFollowers: () => sendRequest('federation', 'getFollowers', []),
      publish: (req) => sendRequest('federation', 'publish', [req]),
      createNote: (req) => sendRequest('federation', 'createNote', [req]),
      like: (objectId) => sendRequest('federation', 'like', [objectId]),
      unlike: (objectId) => sendRequest('federation', 'unlike', [objectId]),
      bookmark: (objectId) => sendRequest('federation', 'bookmark', [objectId]),
      unbookmark: (objectId) => sendRequest('federation', 'unbookmark', [objectId]),
      getBookmarks: () => sendRequest('federation', 'getBookmarks', []),
      announce: (objectId, content) => sendRequest('federation', 'announce', [objectId, content]),
      unannounce: (objectId) => sendRequest('federation', 'unannounce', [objectId]),
      getExternalShareStatus: () => sendRequest('federation', 'getExternalShareStatus', []),
      composeExternalShare: (req) => sendRequest('federation', 'composeExternalShare', [req]),
      uploadMedia: (req) => sendRequest('federation', 'uploadMedia', [req]),
      unpublish: (req) => sendRequest('federation', 'unpublish', [req]),
      getPublished: () => sendRequest('federation', 'getPublished', []),
      getChannels: () => sendRequest('federation', 'getChannels', []),
      getChannel: (id) => sendRequest('federation', 'getChannel', [id]),
      createChannel: (req) => sendRequest('federation', 'createChannel', [req]),
      acceptChannel: (id) => sendRequest('federation', 'acceptChannel', [id]),
      closeChannel: (id) => sendRequest('federation', 'closeChannel', [id]),
      deleteChannel: (id) => sendRequest('federation', 'deleteChannel', [id]),
      getMessages: (channelId, before, limit) => sendRequest('federation', 'getMessages', [channelId, before, limit]),
      sendMessage: (channelId, req) => sendRequest('federation', 'sendMessage', [channelId, req]),
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
      setMemberRole: (roomId, actorUrl, role) => sendRequest('federation', 'setMemberRole', [roomId, actorUrl, role]),
      leaveRoom: (roomId) => sendRequest('federation', 'leaveRoom', [roomId]),
      transferRoomOwnership: (roomId, newOwner) => sendRequest('federation', 'transferRoomOwnership', [roomId, newOwner]),
      initiateChannelE2e: (channelId) => sendRequest('federation', 'initiateChannelE2e', [channelId]),
      initiateRoomE2e: (roomId) => sendRequest('federation', 'initiateRoomE2e', [roomId]),
      addRoomSticker: (roomId, req) => sendRequest('federation', 'addRoomSticker', [roomId, req]),
      removeRoomSticker: (roomId, stickerId) => sendRequest('federation', 'removeRoomSticker', [roomId, stickerId]),
      deleteRoom: (roomId) => sendRequest('federation', 'deleteRoom', [roomId]),
      getRings: () => sendRequest('federation', 'getRings', []),
      getRing: (id) => sendRequest('federation', 'getRing', [id]),
      getRingPeers: (id) => sendRequest('federation', 'getRingPeers', [id]),
      createRing: (req) => sendRequest('federation', 'createRing', [req]),
      leaveRing: (ringId) => sendRequest('federation', 'leaveRing', [ringId]),
      addPeer: (ringId, req) => sendRequest('federation', 'addPeer', [ringId, req]),
      removePeer: (ringId, peerUrl) => sendRequest('federation', 'removePeer', [ringId, peerUrl]),
      triggerSync: (ringId) => sendRequest('federation', 'triggerSync', [ringId]),
      getTrustPolicy: () => sendRequest('federation', 'getTrustPolicy', []),
      updateTrustPolicy: (req) => sendRequest('federation', 'updateTrustPolicy', [req]),
      getInstances: () => sendRequest('federation', 'getInstances', []),
      getDeliveryStats: () => sendRequest('federation', 'getDeliveryStats', []),
      listDelivery: (limit) => sendRequest('federation', 'listDelivery', [limit]),
      retryDelivery: (id) => sendRequest('federation', 'retryDelivery', [id]),
      cancelDelivery: (id) => sendRequest('federation', 'cancelDelivery', [id]),
      retryAllDeadDelivery: (limit) => sendRequest('federation', 'retryAllDeadDelivery', [limit]),
      cancelAllPendingDelivery: (limit) => sendRequest('federation', 'cancelAllPendingDelivery', [limit]),
      dismissDelivery: (id) => sendRequest('federation', 'dismissDelivery', [id]),
      purgeDeadDelivery: (opts) => sendRequest('federation', 'purgeDeadDelivery', [opts]),
      joinRoom: (roomId, opts) => sendRequest('federation', 'joinRoom', [roomId, opts]),
      updateInstanceTrust: (req) => sendRequest('federation', 'updateInstanceTrust', [req]),
      toggleInstanceBlock: (req) => sendRequest('federation', 'toggleInstanceBlock', [req]),
      initiateTransfer: (channelId, req) => sendRequest('federation', 'initiateTransfer', [channelId, req]),
      listTransfers: (channelId) => sendRequest('federation', 'listTransfers', [channelId]),
      initiateRoomTransfer: (roomId, req) => sendRequest('federation', 'initiateRoomTransfer', [roomId, req]),
      listRoomTransfers: (roomId) => sendRequest('federation', 'listRoomTransfers', [roomId]),
      listRoomFiles: (roomId, params) => sendRequest('federation', 'listRoomFiles', [roomId, params]),
      getTransfer: (transferId) => sendRequest('federation', 'getTransfer', [transferId]),
      downloadTransfer: (transferId) => sendRequest('federation', 'downloadTransfer', [transferId]),
      uploadChunk: (transferId, req) => sendRequest('federation', 'uploadChunk', [transferId, req]),
      cancelTransfer: (transferId) => sendRequest('federation', 'cancelTransfer', [transferId]),
      subscribeChannel: (channelId) => sendRequest('federation', 'subscribeChannel', [channelId]),
      unsubscribeChannel: (channelId) => sendRequest('federation', 'unsubscribeChannel', [channelId]),
      subscribeRoom: (roomId) => sendRequest('federation', 'subscribeRoom', [roomId]),
      unsubscribeRoom: (roomId) => sendRequest('federation', 'unsubscribeRoom', [roomId]),
      onMessage: (cb) => addEventListener('federation:message', cb),
      onChannelUpdate: (cb) => addEventListener('federation:channelUpdate', cb),
      onRoomUpdate: (cb) => addEventListener('federation:roomUpdate', cb),
    },`
}

function assetsNamespace(): string {
  return `
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
        const entry = { url: url, mimeType: asset.mimeType, size: asset.size, path: path };
        _assetUrlByPath.set(path, entry);
        _assetUrls.add(url);
        return entry;
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
        await Promise.all(paths.map(async (path) => {
          const entry = await Tapp.assets.getUrl(path);
          map[path] = entry.url;
        }));
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
        try { URL.revokeObjectURL(url); } catch {}
        _assetUrls.delete(url);
        _assetUrlByPath.forEach((value, key) => {
          if (value && value.url === url) _assetUrlByPath.delete(key);
        });
      },
      revokeAll: () => revokeAllAssetUrls(),
    },`
}

export function generateSdkBody(input: GenerateSdkBodyInput): string {
  const {
    surface,
    idLiteral,
    nameLiteral,
    versionLiteral,
    tokenLiteral,
    permissionsLiteral,
    gameTypeLiteral,
  } = input
  const caps = input.caps ?? ALL_WIDGET_CAPS
  const isWidget = surface === 'widget'
  const isHeadless = surface === 'headless'
  const isPage = surface === 'page'
  const idPrefix = isWidget ? 'widget' : 'tapp'
  const bufferedEvents = isWidget && !caps.media
    ? `new Set(['themeChange', 'primaryColorChange', 'localeChange', 'animationLevelChange'])`
    : `new Set(['mediaStateChange', 'mediaProgress', 'themeChange', 'primaryColorChange', 'localeChange', 'animationLevelChange'])`

  return `
(() => {
  'use strict';

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
      try { URL.revokeObjectURL(url); } catch {}
    });
    _assetUrls.clear();
    _assetUrlByPath.clear();
    _model3dUrls.forEach((url) => {
      try { URL.revokeObjectURL(url); } catch {}
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

  const _HOST_WINDOW = (() => {
    const takeNativeParent = window.__TAPP_TAKE_NATIVE_PARENT__;
    const hostWindow = typeof takeNativeParent === 'function'
      ? takeNativeParent()
      : window.parent;
    try { delete window.__TAPP_TAKE_NATIVE_PARENT__; } catch {}
    return hostWindow;
  })();

  const _eventBuffer = new Map();
  const _BUFFERED_EVENTS = ${bufferedEvents};
  const _ACTION_TO_EVENT = { 'theme:change': 'themeChange', 'locale:change': 'localeChange', 'primaryColor:change': 'primaryColorChange', 'animationLevel:change': 'animationLevelChange' };
  let spectrumStreamSubscribers = 0;

  ${SDK_HOST_CHROME_CODE}

  const generateId = () => '${idPrefix}-' + (++messageIdCounter) + '-' + Date.now();

  ${generateStorageKeyValidator()}
  ${sdkRequestTimeoutHelper()}
  ${isWidget ? DENIED_HELPER : ''}

  const sendRequest = (api, method, args = []) => {
    return new Promise((resolve, reject) => {
      const id = generateId();
      const timeout = setTimeout(() => {
        pendingRequests.delete(id);
        reject(new Error('Request timeout'));
      }, requestTimeoutMs(api, method));

      pendingRequests.set(id, { resolve, reject, timeout });

      try {
        _HOST_WINDOW.postMessage({
          type: 'request',
          id,
          action: api + '.' + method,
          payload: { api, method, args },
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

  const addEventListener = (event, callback) => {
    let listeners = eventListeners.get(event);
    if (!listeners) {
      listeners = new Set();
      eventListeners.set(event, listeners);
    }
    listeners.add(callback);
    const buffered = _eventBuffer.get(event);
    if (buffered !== undefined) {
      try { callback(buffered); } catch {}
    }
    return () => listeners.delete(callback);
  };

  window.addEventListener('message', (event) => {
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
      const _bufKey = _ACTION_TO_EVENT[message.action] || message.action;
      if (_BUFFERED_EVENTS.has(_bufKey)) {
        _eventBuffer.set(_bufKey, message.payload);
      }
      eventListeners.get(message.action)?.forEach((cb) => { try { cb(message.payload); } catch {} });

      if (message.action === 'lifecycle:destroy') notifyLifecycleDestroy();
      else if (message.action === 'lifecycle:pause') {
        runLifecycleCallbacks('pause');
        eventListeners.get('pause')?.forEach((cb) => { try { cb(); } catch {} });
      }
      else if (message.action === 'lifecycle:resume') {
        runLifecycleCallbacks('resume');
        eventListeners.get('resume')?.forEach((cb) => { try { cb(); } catch {} });
      }
      else if (message.action === 'theme:change') {
        eventListeners.get('themeChange')?.forEach((cb) => { try { cb(message.payload); } catch {} });
        applyTappTheme(message.payload);
      }
      else if (message.action === 'locale:change') {
        currentLocale = typeof message.payload === 'string' ? message.payload : currentLocale;
        eventListeners.get('localeChange')?.forEach((cb) => { try { cb(message.payload); } catch {} });
      }
      else if (message.action === 'primaryColor:change') {
        eventListeners.get('primaryColorChange')?.forEach((cb) => { try { cb(message.payload); } catch {} });
        applyTappPrimaryColor(message.payload);
      }
      else if (message.action === 'animationLevel:change') {
        eventListeners.get('animationLevelChange')?.forEach((cb) => { try { cb(message.payload); } catch {} });
      }
      else if (message.action === 'container:resize') {
        window._TAPP_DIMENSIONS = message.payload;
        const root = document.documentElement;
        root.style.setProperty('--tapp-scale', message.payload.scale || 1);
        root.style.setProperty('--tapp-font-scale', message.payload.fontScale || 1);
        window.dispatchEvent(new CustomEvent('tapp:resize', { detail: message.payload }));
      }
    }
  });

  let currentLocale = typeof window._TAPP_LOCALE === 'string'
    ? window._TAPP_LOCALE
    : (typeof document !== 'undefined' && document.documentElement && document.documentElement.lang)
      || (typeof navigator !== 'undefined' && navigator.language)
      || 'en-US';
  const translate = (key, variables = {}) => {
    const all = window._TAPP_I18N && typeof window._TAPP_I18N === 'object'
      ? window._TAPP_I18N
      : {};
    const language = currentLocale.split('-')[0];
    const table = all[currentLocale] || all[language] || all['en-US'] || {};
    const directValue = table && typeof table === 'object' ? table[String(key)] : undefined;
    const value = typeof directValue === 'string'
      ? directValue
      : String(key).split('.').reduce(
          (current, part) => current && typeof current === 'object' ? current[part] : undefined,
          table,
        );
    const text = typeof value === 'string' ? value : String(key);
    return text.replaceAll(/\\{([a-zA-Z0-9_]+)\\}/g, (match, name) =>
      Object.hasOwn(variables, name) ? String(variables[name]) : match
    );
  };

  const Tapp = {
    id: ${idLiteral},
    version: ${versionLiteral},
    name: ${nameLiteral},
    permissions: ${permissionsLiteral},
${lifecycleNamespace(surface, idLiteral, nameLiteral, versionLiteral, permissionsLiteral)}
    i18n: {
      t: translate,
      getLocale: () => currentLocale,
      getAll: () => {
        const all = window._TAPP_I18N;
        return all && typeof all === 'object' ? structuredClone(all) : {};
      },
    },
${widgetNamespace(surface)}
${when(isPage, pageHostNamespaces(gameTypeLiteral))}
${when(isHeadless, brewAndFederation(gameTypeLiteral))}
${platformNamespace(surface, caps.platform)}
${analyticsNamespace(surface, caps.analytics)}
${model3dNamespace(surface)}
${aiNamespace(surface, caps.ai)}
${reportNamespace(surface, caps.report)}
${generateKvNamespaceCode('storage', 'arrow')}
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
${generateSettingsNamespaceCode('arrow')}
${generateKvNamespaceCode('shared', 'arrow')}
${generateKvNamespaceCode('private', 'arrow')}
${uiNamespace(surface)}
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
      getGeo: () => sendRequest('context', 'getGeo', []),
    },
    persona: {
      get: () => sendRequest('persona', 'get', []),
    },
${mediaNamespace(surface, caps.media)}
${eventNamespace(surface, caps.event)}
${agentNamespace(surface, caps.agent)}
${when(
  !isHeadless,
  `
    dom: ${DOM_HELPERS_CODE},
    file: {
      ${FILE_DOWNLOAD_METHOD_CODE},
    },`,
)}
${assetsNamespace()}
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
${schedulerNamespace(surface, caps.scheduler)}
    animation: {
      getLevel: () => sendRequest('animation', 'getLevel', []),
      shouldAnimate: () => sendRequest('animation', 'shouldAnimate', []),
      getConfig: () => sendRequest('animation', 'getConfig', []),
      getStaggerDelay: (i, d) => sendRequest('animation', 'getStaggerDelay', [i, d]),
      onLevelChange: (cb) => addEventListener('animationLevelChange', cb),
    },
${speechNamespace(surface, caps.speech)}
    on: addEventListener,
${when(
  !isHeadless,
  `
    widgets: {},
    pages: {},`,
)}
  };

  window.Tapp = Tapp;
  ${SDK_FREEZE_TAPP_CODE}

  Object.defineProperty(window, 'Tapp', {
    value: window.Tapp,
    writable: false,
    configurable: false
  });
${when(
  !isWidget,
  `
  window.addEventListener('error', (event) => {
    const error = event.error || new Error(event.message || 'Unknown window error');
    Tapp.lifecycle._notifyError(error).catch(() => {});
  });
  window.addEventListener('unhandledrejection', (event) => {
    const reason = event.reason;
    const error = reason instanceof Error ? reason : new Error(String(reason));
    Tapp.lifecycle._notifyError(error).catch(() => {});
  });

  setTimeout(() => Tapp.lifecycle._notifyReady(), 0);`,
)}
})();
`
}
