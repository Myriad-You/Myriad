/** helper 源码不要含 TypeScript 语法，以便内联进沙箱。 */

export const ASSET_URL_HELPER_SOURCE = `
function normalizeDeclaredAssetPath(url) {
  if (typeof url !== 'string') return '';
  let path = url.trim();
  if (!path) return '';
  if (/^blob:/i.test(path) || /^data:/i.test(path)) return '';
  if (/^https?:\\/\\//i.test(path) || path.startsWith('//')) return '';
  try { path = decodeURI(path); } catch {}
  let cut = path.split('#')[0].split('?')[0];
  const marker = cut.indexOf('assets/');
  if (marker >= 0) cut = cut.slice(marker);
  cut = cut.replaceAll(/^(\\.\\/)+/g, '');
  if (cut.includes('..') || cut.includes('\\\\') || !cut.startsWith('assets/')) return '';
  if (cut.length > 512) return '';
  return cut;
}

function isSandboxedFetchUrl(url) {
  if (typeof url !== 'string') return false;
  const value = url.trim().toLowerCase();
  return value.startsWith('blob:') || value.startsWith('data:');
}

function rewriteAssetUrl(url, urls) {
  if (typeof url !== 'string' || !url || !urls) return '';
  if (isSandboxedFetchUrl(url)) return url;
  if (/^https?:\\/\\//i.test(url) || url.startsWith('//')) return '';
  const declared = normalizeDeclaredAssetPath(url);
  if (declared && urls[declared]) return urls[declared];
  if (urls[url]) return urls[url];
  const base = url.split('?')[0].split('#')[0].split('/').pop();
  if (!base) return '';
  const hits = [];
  for (const key of Object.keys(urls)) {
    if (key === base || key.endsWith('/' + base)) hits.push(key);
  }
  return hits.length === 1 ? urls[hits[0]] : '';
}

function resolveDeclaredAssetPath(url, urls) {
  const declared = normalizeDeclaredAssetPath(url);
  if (declared) return declared;
  if (typeof url !== 'string' || !url || !urls) return '';
  const rewritten = rewriteAssetUrl(url, urls);
  if (!rewritten) return '';
  for (const key of Object.keys(urls)) {
    if (urls[key] === rewritten) return key;
  }
  return '';
}
`

/** 与 ASSET_URL_HELPER_SOURCE 行为保持一致。 */
export function normalizeDeclaredAssetPath(url: string): string {
  if (typeof url !== 'string') return ''
  let path = url.trim()
  if (!path) return ''
  if (/^blob:/i.test(path) || /^data:/i.test(path)) return ''
  if (/^https?:\/\//i.test(path) || path.startsWith('//')) return ''
  try {
    path = decodeURI(path)
  } catch {
  }
  let cut = path.split('#')[0].split('?')[0]
  const marker = cut.indexOf('assets/')
  if (marker >= 0) cut = cut.slice(marker)
  cut = cut.replaceAll(/^(\.\/)+/g, '')
  if (cut.includes('..') || cut.includes('\\') || !cut.startsWith('assets/')) return ''
  if (cut.length > 512) return ''
  return cut
}

export function isSandboxedFetchUrl(url: string): boolean {
  if (typeof url !== 'string') return false
  const value = url.trim().toLowerCase()
  return value.startsWith('blob:') || value.startsWith('data:')
}

export function rewriteAssetUrl(url: string, urls: Record<string, string>): string {
  if (typeof url !== 'string' || !url || !urls) return ''
  if (isSandboxedFetchUrl(url)) return url
  if (/^https?:\/\//i.test(url) || url.startsWith('//')) return ''
  const declared = normalizeDeclaredAssetPath(url)
  if (declared && urls[declared]) return urls[declared]
  if (urls[url]) return urls[url]
  const base = url.split('?')[0].split('#')[0].split('/').pop()
  if (!base) return ''
  const hits: string[] = []
  for (const key of Object.keys(urls)) {
    if (key === base || key.endsWith(`/${base}`)) hits.push(key)
  }
  return hits.length === 1 ? urls[hits[0]] : ''
}

export function resolveDeclaredAssetPath(url: string, urls: Record<string, string>): string {
  const declared = normalizeDeclaredAssetPath(url)
  if (declared) return declared
  if (typeof url !== 'string' || !url || !urls) return ''
  const rewritten = rewriteAssetUrl(url, urls)
  if (!rewritten) return ''
  for (const key of Object.keys(urls)) {
    if (urls[key] === rewritten) return key
  }
  return ''
}

export const SANDBOXED_FETCH_INSTALL_SOURCE = `
${ASSET_URL_HELPER_SOURCE}
function installSandboxedFetch(window) {
  const _nativeFetch = typeof window.fetch === 'function' ? window.fetch.bind(window) : null;
  window.fetch = function(input, init) {
    let url = '';
    try {
      if (typeof input === 'string') url = input;
      else if (input && typeof input.url === 'string') url = input.url;
    } catch {}
    if (_nativeFetch && isSandboxedFetchUrl(url)) {
      return _nativeFetch(input, init);
    }
    return Promise.reject(new Error('fetch disabled - use Tapp.api() with manifest.apis declarations'));
  };
}
`
