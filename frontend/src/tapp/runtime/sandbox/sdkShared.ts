/**
 * Shared fragments for page/headless and widget sandbox SDK generation.
 */

// 预缓存的静态代码片段

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

/** Default host round-trip (storage / lifecycle / list). */
const SDK_DEFAULT_REQUEST_TIMEOUT_MS = 30_000
/** Floor for Tapp.ai.* (create may wait on image-reference fetch). */
const SDK_AI_REQUEST_TIMEOUT_MS = 5 * 60 * 1000
/** model3d.awaitTask waits on the provider; match MEROPE_PROXY / default Tripo. */
const SDK_MODEL3D_AWAIT_TIMEOUT_MS = 15 * 60 * 1000

export function sdkRequestTimeoutHelper(): string {
  return `
  var requestTimeoutMs = function(api, method) {
    if (api === 'ai') return ${SDK_AI_REQUEST_TIMEOUT_MS};
    if (api === 'model3d' && method === 'awaitTask') return ${SDK_MODEL3D_AWAIT_TIMEOUT_MS};
    return ${SDK_DEFAULT_REQUEST_TIMEOUT_MS};
  };
`
}

/** Page 与 Widget 共用的安全 DOM helper，避免两套 SDK 能力漂移。 */
export const DOM_HELPERS_CODE = `{
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
 * Page / Widget 共用的 file.download 包装：文本、本站生成资源 URL、
 * data URL / base64，以及沙箱 blob:（先读成 base64 再交给宿主）。
 */
export const FILE_DOWNLOAD_METHOD_CODE = `download: function(contentOrOptions, filename, mimeType) {
        var send = function(options) {
          return sendRequest('file', 'download', [options]);
        };
        var fetchBlob = function(blobUrl, name, type) {
          return fetch(blobUrl).then(function(res) {
            if (!res.ok) throw new Error('Could not read blob for download');
            return res.arrayBuffer().then(function(buf) {
              var bytes = new Uint8Array(buf);
              var binary = '';
              var chunk = 0x8000;
              for (var i = 0; i < bytes.length; i += chunk) {
                binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
              }
              return send({
                base64: btoa(binary),
                filename: name,
                mimeType: type || res.type || undefined
              });
            });
          });
        };
        var looksLikeBase64 = function(value) {
          var compact = String(value).replace(/\\s/g, '');
          return compact.length >= 32 && compact.length % 4 === 0 && /^[A-Za-z0-9+/]+=*$/.test(compact);
        };
        var looksBinaryDownload = function(name, type) {
          var n = String(name || '').toLowerCase();
          var t = String(type || '').toLowerCase();
          return /\\.(mp3|wav|ogg|png|jpe?g|gif|webp|glb|gltf|wasm|pdf|bin)$/.test(n)
            || /^(audio|image|model)\\//.test(t)
            || t === 'application/octet-stream'
            || t === 'application/pdf'
            || t === 'model/gltf-binary';
        };
        if (contentOrOptions && typeof contentOrOptions === 'object') {
          var assetId = contentOrOptions.assetId;
          var nestedUrl = contentOrOptions.url;
          var hasTextSource = typeof contentOrOptions.content === 'string' || typeof contentOrOptions.base64 === 'string';
          if (!hasTextSource && typeof assetId === 'string' && /^[0-9a-f]{64}$/i.test(assetId)) {
            return send({
              url: '/api/model3d/assets/' + assetId,
              filename: filename || contentOrOptions.filename,
              mimeType: mimeType || contentOrOptions.mimeType
            });
          }
          if (typeof nestedUrl === 'string' && nestedUrl.indexOf('blob:') === 0) {
            var blobName = filename || contentOrOptions.filename;
            if (!blobName && typeof contentOrOptions.path === 'string') {
              var pathParts = String(contentOrOptions.path).split('/');
              blobName = pathParts[pathParts.length - 1] || undefined;
            }
            return fetchBlob(nestedUrl, blobName, mimeType || contentOrOptions.mimeType);
          }
          return send(contentOrOptions);
        }
        if (typeof contentOrOptions !== 'string') {
          return send({ content: contentOrOptions, filename: filename, mimeType: mimeType });
        }
        if (contentOrOptions.indexOf('/api/brew/image-cache/') !== -1 || contentOrOptions.indexOf('/api/model3d/assets/') !== -1) {
          return send({ url: contentOrOptions, filename: filename, mimeType: mimeType });
        }
        if (contentOrOptions.indexOf('data:') === 0 && contentOrOptions.indexOf(';base64,') !== -1) {
          return send({ base64: contentOrOptions, filename: filename, mimeType: mimeType });
        }
        if (contentOrOptions.indexOf('blob:') === 0) {
          return fetchBlob(contentOrOptions, filename, mimeType);
        }
        if (looksBinaryDownload(filename, mimeType) && looksLikeBase64(contentOrOptions)) {
          return send({ base64: contentOrOptions, filename: filename, mimeType: mimeType });
        }
        return send({ content: contentOrOptions, filename: filename, mimeType: mimeType });
      }`

/**
 * 生成存储 key 验证代码（使用缓存）
 */
export function generateStorageKeyValidator(): string {
  return STORAGE_KEY_VALIDATOR_CODE
}
