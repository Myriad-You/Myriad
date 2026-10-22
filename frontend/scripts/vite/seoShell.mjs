/**
 * Dest copy of proxy crawler-shell routing.
 * Prod path: `proxy/src/main.rs` `is_seo_document_shell_path` / `is_seo_crawler_ua`.
 * Humans stay on the SPA. These routes do not define the React Router table.
 */

export const SEO_SHELL_EXACT = [
  '/',
  '/tapp',
  '/library',
]

export const SEO_SHELL_PREFIXES = [
  '/tapp/run/',
]

export const SEO_CRAWLER_MARKERS = [
  'googlebot',
  'bingbot',
  'slurp',
  'duckduckbot',
  'baiduspider',
  'yandexbot',
  'facebookexternalhit',
  'facebot',
  'twitterbot',
  'linkedinbot',
  'embedly',
  'quora link preview',
  'pinterest',
  'applebot',
  'semrushbot',
  'ahrefsbot',
  'mj12bot',
  'dotbot',
  'petalbot',
  'bytespider',
  'discordbot',
  'telegrambot',
  'whatsapp',
  'slackbot',
  'redditbot',
  'skypeuripreview',
  'rogerbot',
  'screaming frog',
  'ia_archiver',
  'archive.org_bot',
  'chatgpt-user',
  'gptbot',
  'claudebot',
  'anthropic-ai',
  'storebot-google',
  'google-inspectiontool',
  'google-site-verification',
  'preview',
  'qq-url-preview',
  'dingtalkbot',
]

export const SEO_INAPP_SHARE_MARKERS = [
  'micromessenger',
  'windowswechat',
  'wxwork',
  'weibo',
]

/** @param {string} path */
export function isSeoDocumentShellPath(path) {
  return (
    SEO_SHELL_EXACT.includes(path) ||
    SEO_SHELL_PREFIXES.some((prefix) => path.startsWith(prefix))
  )
}

/** @param {string} [ua] */
export function isSeoCrawlerUserAgent(ua) {
  const s = String(ua || '').toLowerCase()
  if (SEO_CRAWLER_MARKERS.some((marker) => s.includes(marker))) return true
  return s.includes('bot/') || s.includes('spider') || s.includes('crawler')
}

/** @param {string} [ua] */
export function isInappShareUserAgent(ua) {
  const s = String(ua || '').toLowerCase()
  return SEO_INAPP_SHARE_MARKERS.some((marker) => s.includes(marker))
}

/** @param {string} urlPath */
export function hasSpaBypass(urlPath) {
  const raw = String(urlPath || '')
  try {
    const url =
      raw.startsWith('http://') || raw.startsWith('https://')
        ? new URL(raw)
        : new URL(raw, 'http://dev.invalid')
    return url.searchParams.get('_spa') === '1'
  } catch {
    return /(?:^|[?&])_spa=1(?:&|$)/.test(raw)
  }
}

/** @param {string} [userAgent] */
export function wantsSeoHtmlShell(userAgent) {
  return isSeoCrawlerUserAgent(userAgent) || isInappShareUserAgent(userAgent)
}
