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

const SDK_DEFAULT_REQUEST_TIMEOUT_MS = 30_000
const SDK_AI_REQUEST_TIMEOUT_MS = 5 * 60 * 1000
const SDK_MODEL3D_AWAIT_TIMEOUT_MS = 15 * 60 * 1000

export function sdkRequestTimeoutHelper(): string {
  return `
  const requestTimeoutMs = function(api, method) {
    if (api === 'ai') return ${SDK_AI_REQUEST_TIMEOUT_MS};
    if (api === 'model3d' && method === 'awaitTask') return ${SDK_MODEL3D_AWAIT_TIMEOUT_MS};
    return ${SDK_DEFAULT_REQUEST_TIMEOUT_MS};
  };
`
}

export const DOM_HELPERS_CODE = `{
      escapeHtml: function(text) {
        if (text == null) return '';
        const htmlEscapes = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#x27;' };
        return String(text).replaceAll(/[&<>"']/g, function(c) { return htmlEscapes[c]; });
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
        const normalizedName = String(name).toLowerCase();
        const dangerous = ['onclick', 'onerror', 'onload', 'onmouseover', 'onfocus', 'onblur', 'onchange', 'onsubmit', 'onkeydown', 'onkeyup'];
        if (dangerous.includes(normalizedName)) return;
        const normalizedValue = String(value).toLowerCase().trim();
        if (['href', 'src', 'action'].includes(normalizedName) &&
            (normalizedValue.startsWith('javascript:') || normalizedValue.startsWith('data:text/html') || normalizedValue.startsWith('vbscript:'))) return;
        el.setAttribute(name, value);
      },
      createElement: function(tag, options) {
        const el = document.createElement(tag);
        if (options) {
          if (Object.hasOwn(options, 'text')) el.textContent = options.text;
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
          const el = renderItem(item, index);
          if (el) container.appendChild(el);
        });
      }
    }`

export const FILE_DOWNLOAD_METHOD_CODE = `download: function(contentOrOptions, filename, mimeType) {
        const send = function(options) {
          return sendRequest('file', 'download', [options]);
        };
        const fetchBlob = function(blobUrl, name, type) {
          return fetch(blobUrl).then(function(res) {
            if (!res.ok) throw new Error('Could not read blob for download');
            return res.arrayBuffer().then(function(buf) {
              const bytes = new Uint8Array(buf);
              let binary = '';
              const chunk = 0x8000;
              for (let i = 0; i < bytes.length; i += chunk) {
                binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
              }
              return send({
                base64: btoa(binary),
                filename: name,
                mimeType: type || res.type || undefined
              });
            });
          });
        };
        const looksLikeBase64 = function(value) {
          const compact = String(value).replaceAll(/\\s/g, '');
          return compact.length >= 32 && compact.length % 4 === 0 && /^[A-Za-z0-9+/]+=*$/.test(compact);
        };
        const looksBinaryDownload = function(name, type) {
          const n = String(name || '').toLowerCase();
          const t = String(type || '').toLowerCase();
          return /\\.(mp3|wav|ogg|png|jpe?g|gif|webp|glb|gltf|wasm|pdf|bin)$/.test(n)
            || /^(audio|image|model)\\//.test(t)
            || t === 'application/octet-stream'
            || t === 'application/pdf'
            || t === 'model/gltf-binary';
        };
        if (contentOrOptions && typeof contentOrOptions === 'object') {
          const assetId = contentOrOptions.assetId;
          const nestedUrl = contentOrOptions.url;
          const hasTextSource = typeof contentOrOptions.content === 'string' || typeof contentOrOptions.base64 === 'string';
          if (!hasTextSource && typeof assetId === 'string' && /^[0-9a-f]{64}$/i.test(assetId)) {
            return send({
              url: '/api/model3d/assets/' + assetId,
              filename: filename || contentOrOptions.filename,
              mimeType: mimeType || contentOrOptions.mimeType
            });
          }
          if (typeof nestedUrl === 'string' && nestedUrl.startsWith('blob:')) {
            let blobName = filename || contentOrOptions.filename;
            if (!blobName && typeof contentOrOptions.path === 'string') {
              const pathParts = String(contentOrOptions.path).split('/');
              blobName = pathParts.at(-1) || undefined;
            }
            return fetchBlob(nestedUrl, blobName, mimeType || contentOrOptions.mimeType);
          }
          return send(contentOrOptions);
        }
        if (typeof contentOrOptions !== 'string') {
          return send({ content: contentOrOptions, filename, mimeType });
        }
        if (contentOrOptions.includes('/api/brew/image-cache/') || contentOrOptions.includes('/api/model3d/assets/')) {
          return send({ url: contentOrOptions, filename, mimeType });
        }
        if (contentOrOptions.startsWith('data:') && contentOrOptions.includes(';base64,')) {
          return send({ base64: contentOrOptions, filename, mimeType });
        }
        if (contentOrOptions.startsWith('blob:')) {
          return fetchBlob(contentOrOptions, filename, mimeType);
        }
        if (looksBinaryDownload(filename, mimeType) && looksLikeBase64(contentOrOptions)) {
          return send({ base64: contentOrOptions, filename, mimeType });
        }
        return send({ content: contentOrOptions, filename, mimeType });
      }`

export function generateStorageKeyValidator(): string {
  return STORAGE_KEY_VALIDATOR_CODE
}

type SdkFnStyle = 'arrow' | 'fn'

function kvMethod(
  style: SdkFnStyle,
  signature: string,
  body: string,
): string {
  if (style === 'arrow') {
    return `${signature} => { ${body} }`
  }
  return `function${signature} { ${body} }`
}

/** storage / shared / private 同一套方法面；权限位相同，REST 闸不同。 */
export function generateKvNamespaceCode(
  api: 'storage' | 'shared' | 'private',
  style: SdkFnStyle,
): string {
  const event = `${api}Changed`
  const call = (method: string, args: string) =>
    `return sendRequest('${api}', '${method}', [${args}]);`
  return `
    ${api}: {
      get: ${kvMethod(style, '(k)', `validateStorageKey(k); ${call('get', 'k')}`)},
      set: ${kvMethod(style, '(k, v)', `validateStorageKey(k); ${call('set', 'k, v')}`)},
      remove: ${kvMethod(style, '(k)', `validateStorageKey(k); ${call('remove', 'k')}`)},
      keys: ${kvMethod(style, '()', call('keys', ''))},
      getAll: ${kvMethod(style, '()', call('getAll', ''))},
      clear: ${kvMethod(style, '()', call('clear', ''))},
      usage: ${kvMethod(style, '()', call('usage', ''))},
      onChanged: ${kvMethod(style, '(cb)', `return addEventListener('${event}', cb);`)}
    },`
}

export function generateSettingsNamespaceCode(style: SdkFnStyle): string {
  return `
    settings: {
      get: ${kvMethod(style, '(k)', "validateStorageKey(k); return sendRequest('settings', 'get', [k]);")},
      set: ${kvMethod(style, '(k, v)', "validateStorageKey(k); return sendRequest('settings', 'set', [k, v]);")},
      getAll: ${kvMethod(style, '()', "return sendRequest('settings', 'getAll', []);")},
      onChanged: ${kvMethod(style, '(cb)', "return addEventListener('settingsChanged', cb);")}
    },`
}

/** Page / Widget 共用：宿主主题推送时写 CSS 变量。依赖同作用域的 forceRepaint。 */
export const SDK_HOST_CHROME_CODE = `
  const forceRepaint = function () {
    void document.body.offsetHeight;
    if (window._TAPP_DISABLE_TRANSFORM_REPAINT) return;
    try {
      requestAnimationFrame(function () {
        document.body.style.transform = 'translateZ(0)';
        requestAnimationFrame(function () {
          document.body.style.transform = '';
        });
      });
    } catch {}
  };
  const applyTappTheme = function(payload) {
    const isDark = payload === 'dark';
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
    root.style.setProperty('--text-primary', isDark ? 'rgba(255,255,255,.92)' : '#1a1a1a');
    root.style.setProperty('--text-secondary', isDark ? 'rgba(255,255,255,.5)' : '#999');
    root.style.setProperty('--bg-primary', isDark ? '#0a0a0a' : '#fff');
    document.body.style.background = isDark ? '#0a0a0a' : '#fff';
    document.body.style.color = isDark ? 'rgba(255,255,255,.92)' : '#1a1a1a';
    forceRepaint();
  };
  const applyTappPrimaryColor = function(payload) {
    if (!payload) return;
    document.documentElement.style.setProperty('--tapp-primary', payload);
    forceRepaint();
  };
`

/** widgets/pages 必须能注册 render，不能冻。其余命名空间一律冻上。 */
export const SDK_FREEZE_TAPP_CODE = `
  (function(tapp) {
    const skip = { widgets: 1, pages: 1 };
    Object.keys(tapp).forEach(function(key) {
      if (skip[key]) return;
      const value = tapp[key];
      if (!value || (typeof value !== 'object' && typeof value !== 'function')) return;
      if (Array.isArray(value)) return;
      Object.freeze(value);
      if (value && typeof value === 'object') {
        if (value.fullscreen) Object.freeze(value.fullscreen);
        if (value.tasks) Object.freeze(value.tasks);
        if (value.platform) Object.freeze(value.platform);
      }
    });
    Object.freeze(tapp);
    try { Object.freeze(Object.getPrototypeOf(tapp)); } catch {}
  })(window.Tapp);
`
