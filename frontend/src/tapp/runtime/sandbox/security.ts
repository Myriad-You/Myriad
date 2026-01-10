/**
 * 沙箱安全策略
 *
 * 提供 CSP 和安全包装代码
 *
 * 安全设计：
 * 1. CSP 严格限制脚本执行来源
 * 2. 使用 nonce 代替 unsafe-inline（当可行时）
 * 3. 禁止 eval 和动态代码执行
 * 4. 阻止所有外部连接
 *
 * 🎯 性能优化：
 * - CSP 字符串预计算
 * - 安全包装代码缓存
 */

/**
 * 生成随机 nonce（用于 CSP script-src）
 *
 * 每个沙箱实例应该生成唯一的 nonce，然后：
 * 1. 将 nonce 传递给 generateCSP(nonce) 生成 CSP 策略
 * 2. 给所有 <script> 标签添加 nonce="${nonce}" 属性
 *
 * 这样只有带正确 nonce 的脚本才能执行，防止 XSS 攻击
 */
export function generateNonce(): string {
  const array = new Uint8Array(16)
  crypto.getRandomValues(array)
  return Array.from(array, b => b.toString(16).padStart(2, '0')).join('')
}

/** 会话 token 生成（用于 postMessage 验证） */
export function generateSessionToken(): string {
  const array = new Uint8Array(32)
  crypto.getRandomValues(array)
  return Array.from(array, b => b.toString(16).padStart(2, '0')).join('')
}

// ========================
// 🎯 CSP 安全策略
// ========================

/**
 * CSP 基础策略片段（不包含 script-src）
 *
 * 安全说明：
 * - 严格限制所有外部资源加载
 * - 🔒 img-src 移除 https: 防止通过图片泄露数据
 * - 🔒 font-src 允许 data: URI 和 Google Fonts
 */
const CSP_BASE_DIRECTIVES = [
  'default-src \'none\'',
  // style-src 允许 unsafe-inline 和 Google Fonts CSS
  'style-src \'unsafe-inline\' https://fonts.googleapis.com',
  // 🔒 安全加强：只允许 data:、blob: 和可信图片源
  // 网易云音乐图片域名：p1.music.126.net, p2.music.126.net 等
  'img-src data: blob: https://*.music.126.net https://*.netease.com',
  // font-src 允许 data: URI 和 Google Fonts 字体文件
  'font-src data: https://fonts.gstatic.com',
  'connect-src \'none\'',
  'frame-src \'none\'',
  'object-src \'none\'',
  'media-src \'none\'',
  'worker-src \'none\'',
  'form-action \'none\'',
  'base-uri \'none\'',
  'manifest-src \'none\'',
  // 注意：prefetch-src 已在现代浏览器中废弃，不再需要
]

/** Tailwind CDN 域名（用于 CSP） */
const TAILWIND_CDN = 'https://cdn.tailwindcss.com'

/**
 * 生成带 nonce 的 CSP 策略
 *
 * 🔒 安全加强：使用 nonce 替代 unsafe-inline
 * - 每个沙箱实例生成唯一的 nonce
 * - 只有带正确 nonce 属性的 script 标签才能执行
 * - 防止注入的恶意脚本执行
 *
 * @param nonce - 唯一的 nonce 值（由 generateNonce() 生成）
 * @returns 完整的 CSP 策略字符串
 */
export function generateCSP(nonce?: string, allowTailwindCDN = true): string {
  // 🔒 script-src: 使用 nonce + 可选 Tailwind CDN
  const cdnPart = allowTailwindCDN ? ` ${TAILWIND_CDN}` : ''
  const scriptSrc = nonce
    ? `script-src 'nonce-${nonce}'${cdnPart}` // 🔒 使用 nonce，更安全
    : `script-src 'unsafe-inline'${cdnPart}` // 回退到 unsafe-inline

  return [scriptSrc, ...CSP_BASE_DIRECTIVES].join('; ')
}

/**
 * 生成安全包装代码
 * 冻结全局对象，防止沙箱逃逸
 *
 * 安全特性：
 * 1. 冻结 window.parent/top/opener，防止父窗口访问
 * 2. 禁用所有对话框（alert/confirm/prompt）
 * 3. 禁用本地存储和网络 API
 * 4. 禁用 eval 和 Function 构造器
 * 5. 冻结原型链防止原型污染攻击
 *
 * @param sessionToken 会话 token，用于消息验证
 */
export function generateSecurityWrapper(sessionToken: string): string {
  return `
(() => {
  'use strict';
  
  // =====================================================
  // 🔒 第一优先级：立即冻结原型链（在任何用户代码之前）
  // =====================================================
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
  const _parentPostMessage = window.parent !== window ? window.parent.postMessage.bind(window.parent) : null;
  
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
    } catch (e) {
      // 静默失败，某些浏览器限制了对这些属性的修改
      return false;
    }
  };
  
  // === 禁用危险的全局 API ===
  
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
  } catch (e) {}
  
  // 🔒 安全加强：拦截 setTimeout/setInterval 的字符串参数
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
  
  // 限制 parent 访问，只允许 postMessage
  safeDefineProperty(window, 'parent', {
    value: { 
      postMessage: _parentPostMessage,
      // 包装 postMessage 自动添加 session token
      __postMessageWithToken: (message, origin) => {
        if (_parentPostMessage && message && typeof message === 'object') {
          message._sessionToken = _SESSION_TOKEN;
        }
        return _parentPostMessage ? _parentPostMessage(message, origin) : undefined;
      }
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
  
  // 禁用网络 API（强制使用 Tapp.api() 声明式 API）
  window.fetch = () => Promise.reject(new Error('fetch disabled - use Tapp.api() with manifest.apis declarations'));
  window.XMLHttpRequest = class { constructor() { throw new Error('XMLHttpRequest disabled - use Tapp.api() with manifest.apis declarations'); } };
  window.WebSocket = class { constructor() { throw new Error('WebSocket is disabled in Tapp sandbox'); } };
  window.EventSource = class { constructor() { throw new Error('EventSource is disabled in Tapp sandbox'); } };
  window.Worker = class { constructor() { throw new Error('Worker is disabled in Tapp sandbox'); } };
  window.SharedWorker = class { constructor() { throw new Error('SharedWorker is disabled in Tapp sandbox'); } };
  
  // 🔒 安全加强：拦截 Image 构造器，防止通过图片 URL 泄露数据
  // 虽然 CSP 已禁止外部图片，但双重防护更安全
  const _OriginalImage = window.Image;
  window.Image = class SecureImage extends _OriginalImage {
    constructor(width, height) {
      super(width, height);
      const originalSrcDescriptor = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src');
      Object.defineProperty(this, 'src', {
        set(value) {
          // 只允许 data: 和 blob: URL
          if (typeof value === 'string') {
            const lowerValue = value.toLowerCase().trim();
            if (!lowerValue.startsWith('data:') && !lowerValue.startsWith('blob:')) {
              console.warn('[Security] External image URLs are blocked:', value.substring(0, 50));
              return;
            }
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
  
  // 🔒 安全加强：拦截 createElement，阻止危险元素创建
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
        if (name.toLowerCase() === 'src') {
          const lowerValue = String(value).toLowerCase().trim();
          if (!lowerValue.startsWith('data:') && !lowerValue.startsWith('blob:')) {
            console.warn('[Security] External image src blocked via setAttribute');
            return;
          }
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
  
  // 🔒 安全加强：阻止 innerHTML 注入 script 标签
  const _originalInnerHTMLDescriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'innerHTML');
  if (_originalInnerHTMLDescriptor) {
    Object.defineProperty(Element.prototype, 'innerHTML', {
      set(value) {
        // 简单的 script 标签检测（不完美，但增加一层防护）
        if (typeof value === 'string' && /<script[^>]*>/i.test(value)) {
          console.warn('[Security] Script tag in innerHTML blocked');
          // 移除 script 标签
          value = value.replace(/<script[^>]*>[\\s\\S]*?<\\/script>/gi, '<!-- script removed -->');
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
  
  // 🔒 安全加强：阻止 outerHTML 注入 script 标签
  const _originalOuterHTMLDescriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'outerHTML');
  if (_originalOuterHTMLDescriptor) {
    Object.defineProperty(Element.prototype, 'outerHTML', {
      set(value) {
        if (typeof value === 'string' && /<script[^>]*>/i.test(value)) {
          console.warn('[Security] Script tag in outerHTML blocked');
          value = value.replace(/<script[^>]*>[\\s\\S]*?<\\/script>/gi, '<!-- script removed -->');
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
  
  // 🔒 安全加强：拦截 insertAdjacentHTML
  const _originalInsertAdjacentHTML = Element.prototype.insertAdjacentHTML;
  Element.prototype.insertAdjacentHTML = function(position, text) {
    if (typeof text === 'string' && /<script[^>]*>/i.test(text)) {
      console.warn('[Security] Script tag in insertAdjacentHTML blocked');
      text = text.replace(/<script[^>]*>[\\s\\S]*?<\\/script>/gi, '<!-- script removed -->');
    }
    return _originalInsertAdjacentHTML.call(this, position, text);
  };
  
  // 🔒 安全加强：阻止 document.write/writeln
  document.write = () => { console.warn('[Security] document.write is disabled'); };
  document.writeln = () => { console.warn('[Security] document.writeln is disabled'); };
  
  // 禁用可能泄露信息的 API
  if (navigator.sendBeacon) {
    navigator.sendBeacon = () => false;
  }
  if (navigator.geolocation) {
    safeDefineProperty(navigator, 'geolocation', { value: null, writable: false });
  }
  
  // === 完整性验证 ===
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

/**
 * iframe sandbox 属性值
 *
 * 安全说明：
 * - allow-scripts: 允许脚本执行（必需）
 * - allow-pointer-lock: 允许指针锁定（用于游戏等交互）
 * - 不使用 allow-same-origin 以获得更强的隔离
 * - 使用 blob: URL 时，sandbox 内容的 origin 为 'null'
 *
 * 注意：移除 allow-same-origin 可能导致某些功能受限，
 * 但显著提高安全性（防止沙箱逃逸）
 *
 * iOS/iPadOS 兼容性：
 * - allow-pointer-lock: 确保触摸交互正常工作
 */
export const IFRAME_SANDBOX_ATTRS = 'allow-scripts allow-pointer-lock'

/**
 * 验证存储 key 格式（防止路径遍历攻击）
 *
 * 规则：
 * - 只允许字母、数字、下划线、连字符、点
 * - 不允许连续的点（..）
 * - 不允许以点开头或结尾
 * - 长度限制 1-256 字符
 */
export function validateStorageKey(key: string): { valid: boolean, reason?: string } {
  if (!key || typeof key !== 'string') {
    return { valid: false, reason: 'Key must be a non-empty string' }
  }

  if (key.length > 256) {
    return { valid: false, reason: 'Key too long (max 256 chars)' }
  }

  // 禁止路径遍历字符
  if (key.includes('..') || key.includes('/') || key.includes('\\')) {
    return { valid: false, reason: 'Key contains invalid path characters' }
  }

  // 禁止以点开头或结尾
  if (key.startsWith('.') || key.endsWith('.')) {
    return { valid: false, reason: 'Key cannot start or end with a dot' }
  }

  // 只允许安全字符
  if (!/^[\w.\-:]+$/.test(key)) {
    return { valid: false, reason: 'Key contains invalid characters (allowed: a-z, A-Z, 0-9, _, -, ., :)' }
  }

  return { valid: true }
}

/**
 * 清理存储 value（防止 XSS 和数据注入）
 *
 * 注意：这是基本的清理，实际存储时后端也应该验证
 */
export function sanitizeStorageValue(value: unknown): unknown {
  // null/undefined 原样返回
  if (value === null || value === undefined) {
    return value
  }

  // 基本类型原样返回
  if (typeof value === 'number' || typeof value === 'boolean') {
    return value
  }

  // 字符串：限制长度
  if (typeof value === 'string') {
    const MAX_STRING_LENGTH = 1024 * 1024 // 1MB
    if (value.length > MAX_STRING_LENGTH) {
      throw new Error(`Value too large (max ${MAX_STRING_LENGTH} chars)`)
    }
    return value
  }

  // 对象/数组：序列化后检查大小
  if (typeof value === 'object') {
    const serialized = JSON.stringify(value)
    const MAX_OBJECT_SIZE = 1024 * 1024 // 1MB
    if (serialized.length > MAX_OBJECT_SIZE) {
      throw new Error(`Value too large (max ${MAX_OBJECT_SIZE} bytes)`)
    }
    return value
  }

  return value
}
