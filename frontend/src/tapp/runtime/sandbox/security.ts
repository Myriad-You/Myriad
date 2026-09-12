import { SANDBOXED_FETCH_INSTALL_SOURCE } from './assetUrlRewriter'

export function generateNonce(): string {
  const array = new Uint8Array(16)
  crypto.getRandomValues(array)
  return Iterator.from(array).map((b) => b.toString(16).padStart(2, '0')).toArray().join('')
}

export function generateSessionToken(): string {
  const array = new Uint8Array(32)
  crypto.getRandomValues(array)
  return Iterator.from(array).map((b) => b.toString(16).padStart(2, '0')).toArray().join('')
}

export function escapeSandboxHtmlText(value: string): string {
  return value.replaceAll(/[&<>]/g, (character) => {
    if (character === '&') return '&amp;'
    if (character === '<') return '&lt;'
    return '&gt;'
  })
}

export function serializeSandboxScriptValue(value: unknown): string {
  const serialized = JSON.stringify(value)
  if (serialized === undefined) return 'undefined'
  return serialized
    .replaceAll('<', '\\u003c')
    .replaceAll('\u2028', '\\u2028')
    .replaceAll('\u2029', '\\u2029')
}

export function escapeSandboxScriptSource(source: string): string {
  return source.replaceAll(/<\/script/gi, '<\\/script')
}

export interface GenerateCSPOptions {
  /** 仅 blob:/data: 媒体。授予 media:audio 时放行。 */
  allowMediaBlob?: boolean
  allowWasm?: boolean
  /** 远端 https/http 图/媒体须授予 network:fetch。connect-src 仍只有 blob:/data:。 */
  allowRemoteMedia?: boolean
}

/** srcdoc 继承宿主 base URL；相对路径解析到宿主源。 */
function hostOrigin(): string {
  return typeof window !== 'undefined' ? window.location.origin : ''
}

export function generateCSP(
  nonce?: string,
  options: GenerateCSPOptions = {},
): string {
  const allowMediaBlob = options.allowMediaBlob === true
  const allowWasm = options.allowWasm !== false
  const allowRemoteMedia = options.allowRemoteMedia === true

  // script-src 仅 nonce。不放行外部脚本 host（query 外泄）。
  const wasmPart = allowWasm ? " 'wasm-unsafe-eval'" : ''
  const scriptSrc = nonce
    ? `script-src 'nonce-${nonce}'${wasmPart}`
    : `script-src 'unsafe-inline'${wasmPart}`

  // 默认只放行 data:/blob:/同源。裸 http(s) 挂 network:fetch。
  const origin = hostOrigin()
  const originPart = origin ? ` ${origin}` : ''
  const remotePart = allowRemoteMedia ? ' https: http:' : ''

  const imgSrc = `img-src data: blob:${remotePart}${originPart}`
  // blob/data 音视频仅在授予 media:audio 时放行。
  const mediaSrc = allowMediaBlob
    ? `media-src data: blob:${remotePart}${originPart}`
    : `media-src${remotePart}${originPart}${remotePart || originPart ? '' : " 'none'"}`

  const directives = [
    scriptSrc,
    "default-src 'none'",
    "style-src 'unsafe-inline' https://fonts.googleapis.com",
    imgSrc,
    'font-src data: https://fonts.gstatic.com',
    'connect-src blob: data:',
    "frame-src 'none'",
    "object-src 'none'",
    mediaSrc,
    "worker-src 'none'",
    "form-action 'none'",
    "base-uri 'none'",
    "manifest-src 'none'",
  ]

  return directives.join('; ')
}

export function cspOptionsFromPermissions(
  grantedPermissions: readonly string[] | undefined,
): GenerateCSPOptions {
  return {
    allowMediaBlob: grantedPermissions?.includes('media:audio') === true,
    allowWasm: true,
    // 远端 https/http 图/媒体均须授予 network:fetch。
    allowRemoteMedia: grantedPermissions?.includes('network:fetch') === true,
  }
}

/** 同 realm 拦截只是深度防御。真正边界是 iframe sandbox、CSP、TappBridge。 */
export function generateSecurityWrapper(
  sessionToken: string,
  allowRemoteMedia = false,
): string {
  const origin = hostOrigin()
  const allowRemote = allowRemoteMedia === true
  return `
(() => {
  'use strict';
  
  // 第一优先级：立即冻结原型链（在任何用户代码之前）
  try {
    Object.freeze(Object.prototype);
    Object.freeze(Array.prototype);
    Object.freeze(Function.prototype);
    Object.freeze(String.prototype);
    Object.freeze(Number.prototype);
    Object.freeze(Boolean.prototype);
    Object.freeze(Date.prototype);
    Object.freeze(RegExp.prototype);
    Object.freeze(Error.prototype);
    Object.freeze(Promise.prototype);
    Object.freeze(Map.prototype);
    Object.freeze(Set.prototype);
    Object.freeze(WeakMap.prototype);
    Object.freeze(WeakSet.prototype);
    Object.freeze(Symbol.prototype);
    Object.freeze(JSON);
    Object.freeze(Math);
    Object.freeze(Reflect);
  } catch (e) {
    console.error('[Security] CRITICAL: Failed to freeze prototypes:', e);
  }
  
  // 保存安全的 postMessage 引用
  // 无 allow-same-origin 时，直接访问 window.parent.postMessage 可能抛出 SecurityError
  let _parentPostMessage = null;
  try {
    if (window.parent !== window) {
      _parentPostMessage = window.parent.postMessage.bind(window.parent);
    }
  } catch {
    // 回退：使用 postMessage 的通用调用方式
    _parentPostMessage = (msg, origin) => window.parent.postMessage(msg, origin);
  }

  // SDK 需要使用真实的父窗口 WindowProxy 校验响应来源。下面会把
  // window.parent 收窄为仅暴露 postMessage 的代理，因此通过一次性 handoff
  // 将原始引用交给紧随其后加载的 SDK；SDK 读取后会立即删除该属性。
  const _nativeParentWindow = window.parent;
  try {
    Object.defineProperty(window, '__TAPP_TAKE_NATIVE_PARENT__', {
      value: () => _nativeParentWindow,
      writable: false,
      enumerable: false,
      configurable: true
    });
  } catch {
    window.__TAPP_TAKE_NATIVE_PARENT__ = () => _nativeParentWindow;
  }
  
  // 会话 token（用于消息验证）
  const _SESSION_TOKEN = '${sessionToken}';
  window._TAPP_SESSION_TOKEN = _SESSION_TOKEN;
  
  // 安全属性定义辅助函数
  // 某些浏览器不允许重定义 top/parent 等属性，这是正常的
  const safeDefineProperty = (obj, prop, descriptor) => {
    try {
      // 先检查属性是否可配置
      const existing = Object.getOwnPropertyDescriptor(obj, prop);
      if (existing && !existing.configurable) {
        // 属性不可配置，静默跳过（这在某些浏览器中是正常的）
        return false;
      }
      Object.defineProperty(obj, prop, { ...descriptor, configurable: false });
      return true;
    } catch {
      // 静默失败，某些浏览器限制了对这些属性的修改
      return false;
    }
  };
  
  // 禁用危险的全局 API
  
  // 禁用 eval 和 Function 构造器（防止动态代码执行）
  try {
    window.eval = () => { throw new Error('eval is disabled in Tapp sandbox'); };
    const _Function = window.Function;
    window.Function = function(...args) {
      if (args.length > 0) {
        throw new Error('Function constructor is disabled in Tapp sandbox');
      }
      return _Function.apply(this, args);
    };
    window.Function.prototype = _Function.prototype;
  } catch {}
  
  // 安全加强：拦截 setTimeout/setInterval 的字符串参数
  // 防止通过 setTimeout("malicious code", 0) 绕过 eval 禁用
  const _originalSetTimeout = window.setTimeout;
  const _originalSetInterval = window.setInterval;
  
  window.setTimeout = function(handler, timeout, ...args) {
    if (typeof handler === 'string') {
      console.warn('[Security] setTimeout with string code is blocked');
      throw new Error('setTimeout with string code is disabled in Tapp sandbox');
    }
    return _originalSetTimeout.call(this, handler, timeout, ...args);
  };
  
  window.setInterval = function(handler, timeout, ...args) {
    if (typeof handler === 'string') {
      console.warn('[Security] setInterval with string code is blocked');
      throw new Error('setInterval with string code is disabled in Tapp sandbox');
    }
    return _originalSetInterval.call(this, handler, timeout, ...args);
  };
  
  // 限制 parent 访问，只允许 postMessage；对象消息自动注入 session token
  // （Bridge 对 request/event 均校验 token；用户代码漏加也会被补上）
  const _hasOwn = Object.hasOwn;
  const _postMessageWithToken = function(message, origin) {
    if (!_parentPostMessage) return undefined;
    if (message && typeof message === 'object') {
      try {
        if (!_hasOwn(message, '_sessionToken') ||
            message._sessionToken == null || message._sessionToken === '') {
          message._sessionToken = _SESSION_TOKEN;
        }
      } catch {
        // frozen / non-extensible message: still attempt send; Bridge will reject if missing
      }
    }
    return _parentPostMessage(message, origin);
  };
  safeDefineProperty(window, 'parent', {
    value: {
      postMessage: _postMessageWithToken,
      __postMessageWithToken: _postMessageWithToken
    },
    writable: false
  });
  
  // 防止访问顶层窗口
  safeDefineProperty(window, 'top', {
    value: window,
    writable: false
  });
  
  // 禁用 opener
  safeDefineProperty(window, 'opener', {
    value: null,
    writable: false
  });
  
  // 禁用对话框
  window.open = () => { throw new Error('window.open is disabled in Tapp sandbox'); };
  window.alert = () => { throw new Error('alert is disabled in Tapp sandbox'); };
  window.confirm = () => { throw new Error('confirm is disabled - use Tapp.ui.confirm()'); };
  window.prompt = () => { throw new Error('prompt is disabled in Tapp sandbox'); };
  window.print = () => { throw new Error('print is disabled in Tapp sandbox'); };
  
  // 禁用本地存储（强制使用 Tapp.storage API）
  const fakeStorage = {
    getItem: () => { console.warn('localStorage disabled - use Tapp.storage'); return null; },
    setItem: () => { console.warn('localStorage disabled - use Tapp.storage'); },
    removeItem: () => { console.warn('localStorage disabled - use Tapp.storage'); },
    clear: () => { console.warn('localStorage disabled - use Tapp.storage'); },
    key: () => null,
    length: 0
  };
  Object.freeze(fakeStorage);
  
  safeDefineProperty(window, 'localStorage', { value: fakeStorage, writable: false });
  safeDefineProperty(window, 'sessionStorage', { value: fakeStorage, writable: false });
  safeDefineProperty(window, 'indexedDB', { value: null, writable: false });
  safeDefineProperty(window, 'caches', { value: null, writable: false });
  
  // 网络 API：禁止任意 URL。blob:/data: 留给包内 Loader（FileLoader / GLB）。
  ${SANDBOXED_FETCH_INSTALL_SOURCE}
  installSandboxedFetch(window);
  window.XMLHttpRequest = class { constructor() { throw new Error('XMLHttpRequest disabled - use Tapp.api() with manifest.apis declarations'); } };
  window.WebSocket = class { constructor() { throw new Error('WebSocket is disabled in Tapp sandbox'); } };
  window.EventSource = class { constructor() { throw new Error('EventSource is disabled in Tapp sandbox'); } };
  window.Worker = class { constructor() { throw new Error('Worker is disabled in Tapp sandbox'); } };
  window.SharedWorker = class { constructor() { throw new Error('SharedWorker is disabled in Tapp sandbox'); } };
  
  // 图片 URL 白名单：与 CSP img-src 对齐。
  // 默认 data:/blob:/宿主同源；裸 http(s) 仅在已授予 network:fetch 时放行。
  const _HOST_ORIGIN = '${origin}';
  const _ALLOW_REMOTE_MEDIA = ${allowRemote};
  const _isAllowedImageUrl = (value) => {
    if (typeof value !== 'string') return true;
    const v = value.trim();
    if (!v) return true;
    const lower = v.toLowerCase();
    if (lower.startsWith('data:') || lower.startsWith('blob:')) return true;
    if (
      _ALLOW_REMOTE_MEDIA &&
      (lower.startsWith('https://') ||
        lower.startsWith('http://') ||
        lower.startsWith('//'))
    ) {
      return true;
    }
    if (_HOST_ORIGIN) {
      const host = _HOST_ORIGIN.toLowerCase();
      if (
        lower === host ||
        lower.startsWith(host + '/') ||
        lower.startsWith(host + '?') ||
        lower.startsWith(host + '#')
      ) {
        return true;
      }
    }
    if (v.startsWith('/') && !v.startsWith('//')) return true;
    return false;
  };

  // 安全加强：拦截 Image 构造器，尽早提示外部图片 URL 被 CSP 拦截
  const _OriginalImage = window.Image;
  window.Image = class SecureImage extends _OriginalImage {
    constructor(width, height) {
      super(width, height);
      const originalSrcDescriptor = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src');
      Object.defineProperty(this, 'src', {
        set(value) {
          if (!_isAllowedImageUrl(value)) {
            console.warn('[Security] Image URL is blocked by the Tapp CSP:', String(value).slice(0, 50));
            return;
          }
          if (originalSrcDescriptor && typeof originalSrcDescriptor.set === 'function') {
            originalSrcDescriptor.set.call(this, value);
          }
        },
        get() {
          if (originalSrcDescriptor && typeof originalSrcDescriptor.get === 'function') {
            return originalSrcDescriptor.get.call(this);
          }
          return '';
        },
        configurable: false
      });
    }
  };
  
  // 安全加强：拦截 createElement，阻止危险元素创建
  const _originalCreateElement = document.createElement.bind(document);
  const BLOCKED_ELEMENTS = ['script', 'iframe', 'frame', 'object', 'embed', 'link'];
  
  document.createElement = function(tagName, options) {
    const lowerTag = String(tagName).toLowerCase();
    
    // 阻止创建危险元素
    if (BLOCKED_ELEMENTS.includes(lowerTag)) {
      console.warn('[Security] Blocked creation of dangerous element:', lowerTag);
      // 返回一个无害的 div 而不是抛出错误，避免破坏正常代码
      return _originalCreateElement('div', options);
    }
    
    const element = _originalCreateElement(tagName, options);
    
    // 对 img 元素添加 src 拦截
    if (lowerTag === 'img') {
      const originalSetAttribute = element.setAttribute.bind(element);
      element.setAttribute = function(name, value) {
        if (name.toLowerCase() === 'src' && !_isAllowedImageUrl(String(value))) {
          console.warn('[Security] Image src is blocked by the Tapp CSP');
          return;
        }
        return originalSetAttribute(name, value);
      };
    }
    
    // 对 a 元素添加 href 拦截（阻止 javascript: 协议）
    if (lowerTag === 'a') {
      const originalSetAttribute = element.setAttribute.bind(element);
      element.setAttribute = function(name, value) {
        if (name.toLowerCase() === 'href') {
          const lowerValue = String(value).toLowerCase().trim();
          if (lowerValue.startsWith('javascript:') || lowerValue.startsWith('vbscript:')) {
            console.warn('[Security] Dangerous href protocol blocked');
            return;
          }
        }
        return originalSetAttribute(name, value);
      };
    }
    
    return element;
  };
  
  // 安全加强：阻止 innerHTML 注入 script 标签
  const _originalInnerHTMLDescriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'innerHTML');
  if (_originalInnerHTMLDescriptor) {
    Object.defineProperty(Element.prototype, 'innerHTML', {
      set(value) {
        // 简单的 script 标签检测（不完美，但增加一层防护）
        if (typeof value === 'string' && /<script[^>]*>/i.test(value)) {
          console.warn('[Security] Script tag in innerHTML blocked');
          // 移除 script 标签
          value = value.replaceAll(/<script[^>]*>[\\s\\S]*?<\\/script>/gi, '<!-- script removed -->');
        }
        if (_originalInnerHTMLDescriptor && typeof _originalInnerHTMLDescriptor.set === 'function') {
          _originalInnerHTMLDescriptor.set.call(this, value);
        }
      },
      get() {
        if (_originalInnerHTMLDescriptor && typeof _originalInnerHTMLDescriptor.get === 'function') {
          return _originalInnerHTMLDescriptor.get.call(this);
        }
        return '';
      },
      configurable: false
    });
  }
  
  // 安全加强：阻止 outerHTML 注入 script 标签
  const _originalOuterHTMLDescriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'outerHTML');
  if (_originalOuterHTMLDescriptor) {
    Object.defineProperty(Element.prototype, 'outerHTML', {
      set(value) {
        if (typeof value === 'string' && /<script[^>]*>/i.test(value)) {
          console.warn('[Security] Script tag in outerHTML blocked');
          value = value.replaceAll(/<script[^>]*>[\\s\\S]*?<\\/script>/gi, '<!-- script removed -->');
        }
        if (_originalOuterHTMLDescriptor && typeof _originalOuterHTMLDescriptor.set === 'function') {
          _originalOuterHTMLDescriptor.set.call(this, value);
        }
      },
      get() {
        if (_originalOuterHTMLDescriptor && typeof _originalOuterHTMLDescriptor.get === 'function') {
          return _originalOuterHTMLDescriptor.get.call(this);
        }
        return '';
      },
      configurable: false
    });
  }
  
  // 安全加强：拦截 insertAdjacentHTML
  const _originalInsertAdjacentHTML = Element.prototype.insertAdjacentHTML;
  Element.prototype.insertAdjacentHTML = function(position, text) {
    if (typeof text === 'string' && /<script[^>]*>/i.test(text)) {
      console.warn('[Security] Script tag in insertAdjacentHTML blocked');
      text = text.replaceAll(/<script[^>]*>[\\s\\S]*?<\\/script>/gi, '<!-- script removed -->');
    }
    return _originalInsertAdjacentHTML.call(this, position, text);
  };
  
  // 安全加强：阻止 document.write/writeln
  document.write = () => { console.warn('[Security] document.write is disabled'); };
  document.writeln = () => { console.warn('[Security] document.writeln is disabled'); };
  
  // 禁用可能泄露信息的 API
  if (navigator.sendBeacon) {
    navigator.sendBeacon = () => false;
  }
  if (navigator.geolocation) {
    safeDefineProperty(navigator, 'geolocation', { value: null, writable: false });
  }
  
  // 完整性验证
  // 验证关键安全属性是否成功设置
  const securityChecks = [
    { check: () => window.parent !== window.top, name: 'parent isolation' },
    { check: () => window.localStorage === fakeStorage, name: 'localStorage disabled' },
    { check: () => window.opener === null, name: 'opener disabled' },
    { check: () => typeof window.eval === 'function' && window.eval.toString().includes('disabled'), name: 'eval disabled' },
  ];
  
  const failedChecks = securityChecks.filter(c => { try { return !c.check(); } catch { return true; } });
  if (failedChecks.length > 0) {
    console.error('[Security] Security checks failed:', failedChecks.map(c => c.name));
  }
  
  // 静默处理已知的无害错误（如 blob URL 访问限制）
  window.addEventListener('error', function(e) {
    // blob URL 相关错误是沙箱的正常行为，不需要显示
    if (e.message && e.message.includes('blob:')) {
      e.preventDefault();
      return true;
    }
  }, true);
  
  // 安全包装器初始化完成（仅在调试时显示）
  // console.log('[Security] Sandbox security wrapper initialized');
})();
`
}

/** 无 allow-same-origin。postMessage 目标 *，靠 event.source 校验。 */
export const IFRAME_SANDBOX_ATTRS = 'allow-scripts allow-pointer-lock'

/** 存储 key：防路径遍历。 */
export function validateStorageKey(key: string): {
  valid: boolean
  reason?: string
} {
  if (!key || typeof key !== 'string') {
    return { valid: false, reason: 'Key must be a non-empty string' }
  }

  if (key.length > 256) {
    return { valid: false, reason: 'Key too long (max 256 chars)' }
  }

  if (key.includes('..') || key.includes('/') || key.includes('\\')) {
    return { valid: false, reason: 'Key contains invalid path characters' }
  }

  if (key.startsWith('.') || key.endsWith('.')) {
    return { valid: false, reason: 'Key cannot start or end with a dot' }
  }

  if (!/^[\w.\-:]+$/.test(key)) {
    return {
      valid: false,
      reason:
        'Key contains invalid characters (allowed: a-z, A-Z, 0-9, _, -, ., :)',
    }
  }

  return { valid: true }
}

export function sanitizeStorageValue(value: unknown): unknown {
  if (value === null || value === undefined) {
    return value
  }

  if (typeof value === 'number' || typeof value === 'boolean') {
    return value
  }

  if (typeof value === 'string') {
    const MAX_STRING_LENGTH = 1024 * 1024
    if (value.length > MAX_STRING_LENGTH) {
      throw new Error(`Value too large (max ${MAX_STRING_LENGTH} chars)`)
    }
    return value
  }

  if (typeof value === 'object') {
    const serialized = JSON.stringify(value)
    const MAX_OBJECT_SIZE = 1024 * 1024
    if (serialized.length > MAX_OBJECT_SIZE) {
      throw new Error(`Value too large (max ${MAX_OBJECT_SIZE} bytes)`)
    }
    return value
  }

  return value
}
