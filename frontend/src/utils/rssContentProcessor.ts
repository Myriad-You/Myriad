import DOMPurify from 'isomorphic-dompurify'
import { API_URL } from '../config'
import { currentCopy } from '../i18n/localeCopy'
import { proxyImageUrl } from './proxyImageUrl'
import { yieldIfSliceExceeded } from './yieldToMain'

export interface ProcessOptions {
  maxImageWidth?: number
  lazyLoadImages?: boolean
  removeTrackingParams?: boolean
  removeEmptyTags?: boolean
  baseUrl?: string
}

const DEFAULT_OPTIONS: ProcessOptions = {
  lazyLoadImages: true,
  removeTrackingParams: true,
  removeEmptyTags: true,
}

/** Hotlink CDNs via proxy; else original https. */
function getProxiedImageUrl(src: string): string {
  if (src.startsWith('data:')) return src
  if (src.startsWith('/api/') || src.startsWith(`${API_URL}/api/`)) return src
  return proxyImageUrl(src) ?? src
}

const TRACKING_PARAMS = [
  'utm_source',
  'utm_medium',
  'utm_campaign',
  'utm_term',
  'utm_content',
  'fbclid',
  'gclid',
  'ref',
  'source',
  'share',
  'from',
  'app',
  'isappinstalled',
  'wfr',
  's_r',
  'nsukey',
  'scene',
  'sub_channel',
  'key',
  'tn',
  'timestamp',
  'sign',
  'token',
  '_t',
  't',
  'random',
  'r',
  '_',
  'chksm',
  'mpshare',
  'isappinstalled',
  'from_msgid',
  'from_itemidx',
  'weibo_id',
  'mb_id',
  'is_hot',
  'hottop_id',
  'traffic_source',
  'traffic_medium',
  'traffic_campaign',
]

/** XSS: DOMPurify allowlist, not a regex denylist. */
const RSS_ALLOWED_TAGS: readonly string[] = [
  'a',
  'abbr',
  'article',
  'aside',
  'audio',
  'b',
  'blockquote',
  'br',
  'caption',
  'code',
  'col',
  'colgroup',
  'dd',
  'del',
  'details',
  'dfn',
  'div',
  'dl',
  'dt',
  'em',
  'figcaption',
  'figure',
  'footer',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'header',
  'hr',
  'i',
  'iframe',
  'img',
  // 只放任务列表的勾选框；别的 input 在 hook 里删掉
  'input',
  'ins',
  'kbd',
  'li',
  'main',
  'mark',
  'nav',
  'ol',
  'p',
  'picture',
  'pre',
  'q',
  's',
  'samp',
  'section',
  'small',
  'source',
  'span',
  'strong',
  'sub',
  'summary',
  'sup',
  'table',
  'tbody',
  'td',
  'tfoot',
  'th',
  'thead',
  'time',
  'tr',
  'u',
  'ul',
  'var',
  'video',
  'wbr',
]

/** Never allow on*, style, srcdoc, formaction. */
const RSS_ALLOWED_ATTR: readonly string[] = [
  'href',
  'src',
  'srcset',
  'alt',
  'title',
  'class',
  'id',
  'target',
  'rel',
  'width',
  'height',
  'loading',
  'decoding',
  'controls',
  'poster',
  'type',
  'media',
  'sizes',
  'colspan',
  'rowspan',
  'scope',
  'headers',
  'open',
  'datetime',
  'cite',
  'start',
  'reversed',
  'value',
  'span',
  'allow',
  'allowfullscreen',
  'referrerpolicy',
  'sandbox',
  'frameborder',
  'data-rss-image',
  'checked',
  'disabled',
  'align',
]

/** Iframe host allowlist; no executable sandboxes. */
export const TRUSTED_IFRAME_HOSTS: readonly string[] = [
  'player.bilibili.com',
  'www.bilibili.com',
  'player.youku.com',
  'v.qq.com',
  'open.iqiyi.com',
  'www.youtube.com',
  'youtube.com',
  'www.youtube-nocookie.com',
  'youtube-nocookie.com',
  'player.vimeo.com',
  'www.dailymotion.com',
  'geo.dailymotion.com',
  'music.163.com',
  'y.music.163.com',
  'i.y.qq.com',
  'y.qq.com',
  'open.spotify.com',
  'embed.music.apple.com',
  'w.soundcloud.com',
  'www.mixcloud.com',
  'www.slideshare.net',
  'docs.google.com',
  'drive.google.com',
  'www.figma.com',
  'platform.twitter.com',
  'platform.x.com',
  'www.instagram.com',
  'www.google.com',
  'maps.google.com',
  'www.openstreetmap.org',
]

export function isTrustedIframeHost(hostname: string): boolean {
  const host = hostname.toLowerCase().replaceAll(/\.$/g, '')
  if (TRUSTED_IFRAME_HOSTS.includes(host)) return true
  if (host.endsWith('.music.163.com')) return true
  if (host.endsWith('.youtube.com') && host.includes('nocookie')) return true
  return false
}

const EMPTY_CONTENT_TAGS = ['p', 'div', 'span', 'section', 'article']

function cleanUrl(url: string): string {
  try {
    const parsed = new URL(url)
    TRACKING_PARAMS.forEach((param) => {
      parsed.searchParams.delete(param)
    })
    return parsed.toString()
  } catch {
    return url
  }
}

let purifyHooksRegistered = false

/** Drop untrusted iframe hosts / srcdoc / non-http(s). */
function ensurePurifyHooks(): void {
  if (purifyHooksRegistered) return
  purifyHooksRegistered = true

  DOMPurify.addHook('uponSanitizeElement', (node, data) => {
    if (data.tagName === 'input') {
      // 任务列表的勾选框是正文里唯一合法的 input，而且只能是只读的。
      const input = node as Element
      if (
        typeof input.getAttribute !== 'function'
        || (input.getAttribute('type') || '').toLowerCase() !== 'checkbox'
      ) {
        input.parentNode?.removeChild(input)
        return
      }
      input.setAttribute('disabled', '')
      return
    }
    if (data.tagName !== 'iframe') return
    const el = node as Element
    if (typeof el.getAttribute !== 'function') {
      el.parentNode?.removeChild(el)
      return
    }
    // srcdoc is never safe.
    if (el.hasAttribute?.('srcdoc')) {
      el.parentNode?.removeChild(el)
      return
    }
    const rawSrc = (el.getAttribute('src') || '').trim()
    if (!rawSrc || /^(javascript|data|vbscript|blob):/i.test(rawSrc)) {
      el.parentNode?.removeChild(el)
      return
    }
    try {
      const href = rawSrc.startsWith('//') ? `https:${rawSrc}` : rawSrc
      const parsed = new URL(href, 'https://example.invalid')
      if (
        !['http:', 'https:'].includes(parsed.protocol)
        || !isTrustedIframeHost(parsed.hostname)
      ) {
        el.parentNode?.removeChild(el)
      }
    } catch {
      el.parentNode?.removeChild(el)
    }
  })
}

/** XSS: DOMPurify allowlist for innerHTML. */
export function sanitizeRssHtml(html: string): string {
  if (!html) return ''
  ensurePurifyHooks()
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS: Iterator.from(RSS_ALLOWED_TAGS).toArray(),
    ALLOWED_ATTR: Iterator.from(RSS_ALLOWED_ATTR).toArray(),
    ALLOW_DATA_ATTR: true,
    ALLOW_UNKNOWN_PROTOCOLS: false,
  })
}

/** Strip iframes not on the host allowlist. */
export function stripUntrustedIframes(html: string): string {
  if (!html) return html

  const keepIfTrusted = (tag: string): string => {
    const srcMatch =
      tag.match(/\bsrc\s*=\s*(["'])([^"']*)\1/i) ??
      tag.match(/\bsrc\s*=\s*([^\s>]+)/i)
    if (!srcMatch) return ''
    const rawSrc = (srcMatch[2] || srcMatch[1] || '').trim()
    // Reject data:/javascript: embeds.
    if (/^(javascript|data|vbscript|blob):/i.test(rawSrc.trim())) return ''
    try {
      // Protocol-relative //host needs a scheme to parse.
      const href = rawSrc.startsWith('//') ? `https:${rawSrc}` : rawSrc
      const parsed = new URL(href, 'https://example.invalid')
      if (!['http:', 'https:'].includes(parsed.protocol)) return ''
      if (isTrustedIframeHost(parsed.hostname)) {
        return tag
      }
    } catch {
      return ''
    }
    return ''
  }

  return html
    .replaceAll(/<iframe\b[\s\S]*?<\/iframe>/gi, (m) => keepIfTrusted(m))
    .replaceAll(/<iframe\b[^>]*>/gi, (m) => keepIfTrusted(m))
}

function removeEmptyTags(html: string): string {
  const tagPattern = EMPTY_CONTENT_TAGS.join('|')
  let result = html
  let prevLength = 0
  while (result.length !== prevLength) {
    prevLength = result.length
    const regex = new RegExp(
      `<(${tagPattern})[^>]*>\\s*(<br\\s*\\/?>\\s*)*<\\/\\1>`,
      'gi',
    )
    result = result.replaceAll(regex, '')
    const nbspRegex = new RegExp(
      `<(${tagPattern})[^>]*>(&nbsp;|\\s)*<\\/\\1>`,
      'gi',
    )
    result = result.replaceAll(nbspRegex, '')
  }
  return result
}

function processImages(html: string, options: ProcessOptions): string {
  const result = html.replaceAll(/<img([^>]*)>/gi, (match, attrs) => {
    const srcMatch = attrs.match(/src\s*=\s*["']([^"']+)["']/i)
    if (!srcMatch) return match

    let src = srcMatch[1]

    if (options.removeTrackingParams) {
      src = cleanUrl(src)
    }

    if (
      options.baseUrl &&
      !src.startsWith('http') &&
      !src.startsWith('data:')
    ) {
      try {
        src = new URL(src, options.baseUrl).toString()
      } catch {
      }
    }

    // Proxy images (CORS).
    src = getProxiedImageUrl(src)

    const newAttrs: string[] = [`src="${src}"`, 'data-rss-image="true"']

    if (options.lazyLoadImages) {
      newAttrs.push('loading="lazy"')
    }

    const altMatch = attrs.match(/alt\s*=\s*["']([^"']*)["']/i)
    if (altMatch) {
      newAttrs.push(`alt="${altMatch[1]}"`)
    }

    const titleMatch = attrs.match(/title\s*=\s*["']([^"']*)["']/i)
    if (titleMatch) {
      newAttrs.push(`title="${titleMatch[1]}"`)
    }

    return `<img ${newAttrs.join(' ')}>`
  })

  return result
}

function processFigures(html: string): string {
  let result = html.replaceAll(/<figure([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-figure my-6"',
      )
    }
    return `<figure${attrs} class="rss-content-figure my-6">`
  })

  result = result.replaceAll(/<figcaption([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-figcaption text-center text-sm mt-2 opacity-60"',
      )
    }
    return `<figcaption${attrs} class="rss-content-figcaption text-center text-sm mt-2 opacity-60">`
  })

  return result
}

function processVideos(html: string): string {
  const result = html.replaceAll(/<video([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-video w-full rounded-xl my-4"',
      )
    }
    return `<video${attrs} class="rss-content-video w-full rounded-xl my-4" controls>`
  })

  return result
}

function processAudio(html: string): string {
  return html.replaceAll(/<audio([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-audio w-full my-4"',
      )
    }
    return `<audio${attrs} class="rss-content-audio w-full my-4" controls>`
  })
}

function processBlockquotes(html: string): string {
  return html.replaceAll(/<blockquote([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-blockquote rounded-xl px-4 py-3 my-4 italic"',
      )
    }
    return `<blockquote${attrs} class="rss-content-blockquote rounded-xl px-4 py-3 my-4 italic">`
  })
}

function processCodeBlocks(html: string): string {
  let result = html.replaceAll(/<pre([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-pre rounded-xl p-4 my-4 overflow-x-auto text-sm"',
      )
    }
    return `<pre${attrs} class="rss-content-pre rounded-xl p-4 my-4 overflow-x-auto text-sm">`
  })

  result = result.replaceAll(
    /(?<!<pre[^>]*>[\s\S]*?)<code(?![^>]*class=)([^>]*)>/gi,
    '<code$1 class="rss-content-inline-code px-1.5 py-0.5 rounded text-[0.9em]">',
  )

  return result
}

function processTables(html: string): string {
  let result = html

  result = result.replaceAll(/<table([^>]*)>/gi, (match, attrs) => {
    const tableClass =
      'rss-content-table w-full text-sm border-collapse rounded-xl overflow-hidden border'
    if (attrs.includes('class=')) {
      const newTag = match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        `class="$1 ${tableClass}"`,
      )
      return `<div class="rss-content-table-wrapper overflow-x-auto my-4 rounded-xl">${newTag}`
    }
    return `<div class="rss-content-table-wrapper overflow-x-auto my-4 rounded-xl"><table${attrs} class="${tableClass}">`
  })

  result = result.replaceAll(/<\/table>/gi, '</table></div>')

  result = result.replaceAll(/<thead([^>]*)>/gi, '<thead$1 class="rss-content-thead">')
  result = result.replaceAll(
    /<th([^>]*)>/gi,
    '<th$1 class="rss-content-th py-2 px-3 text-left font-medium border-b">',
  )

  result = result.replaceAll(/<tr([^>]*)>/gi, '<tr$1 class="rss-content-tr">')
  result = result.replaceAll(
    /<td([^>]*)>/gi,
    '<td$1 class="rss-content-td py-2 px-3 border-b">',
  )

  return result
}

function processDescriptionLists(html: string): string {
  let result = html

  result = result.replaceAll(/<dl([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-dl my-4"',
      )
    }
    return `<dl${attrs} class="rss-content-dl my-4">`
  })

  result = result.replaceAll(/<dt([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-dt font-semibold mt-2"',
      )
    }
    return `<dt${attrs} class="rss-content-dt font-semibold mt-2">`
  })

  result = result.replaceAll(/<dd([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-dd ml-4 pl-4 mt-1"',
      )
    }
    return `<dd${attrs} class="rss-content-dd ml-4 pl-4 mt-1">`
  })

  return result
}

function processDetails(html: string): string {
  let result = html

  result = result.replaceAll(/<details([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-details rounded-xl my-4 overflow-hidden"',
      )
    }
    return `<details${attrs} class="rss-content-details rounded-xl my-4 overflow-hidden">`
  })

  result = result.replaceAll(/<summary([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-summary cursor-pointer py-3 px-4 font-medium select-none transition-colors"',
      )
    }
    return `<summary${attrs} class="rss-content-summary cursor-pointer py-3 px-4 font-medium select-none transition-colors">`
  })

  return result
}

function processLinks(html: string, options: ProcessOptions): string {
  return html.replaceAll(/<a([^>]*)>/gi, (match, attrs) => {
    const hrefMatch = attrs.match(/href\s*=\s*["']([^"']+)["']/i)
    if (!hrefMatch) return match

    let href = hrefMatch[1]

    if (options.removeTrackingParams) {
      href = cleanUrl(href)
    }

    if (
      options.baseUrl &&
      !href.startsWith('http') &&
      !href.startsWith('#') &&
      !href.startsWith('mailto:')
    ) {
      try {
        href = new URL(href, options.baseUrl).toString()
      } catch {
        // 忽略
      }
    }

    let newAttrs = attrs.replaceAll(/href\s*=\s*["'][^"']+["']/gi, `href="${href}"`)

    if (href.startsWith('http')) {
      if (!newAttrs.includes('target=')) {
        newAttrs += ' target="_blank"'
      }
      if (!newAttrs.includes('rel=')) {
        newAttrs += ' rel="noopener noreferrer"'
      }
    }

    if (!newAttrs.includes('class=')) {
      newAttrs +=
        ' class="rss-content-link text-inherit underline underline-offset-2 decoration-1 wrap-break-word"'
    }

    return `<a${newAttrs}>`
  })
}

function processKbd(html: string): string {
  return html.replaceAll(/<kbd([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-kbd border px-1.5 py-0.5 rounded text-[0.85em] font-mono"',
      )
    }
    return `<kbd${attrs} class="rss-content-kbd border px-1.5 py-0.5 rounded text-[0.85em] font-mono">`
  })
}

function processMark(html: string): string {
  return html.replaceAll(/<mark([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-mark px-0.5 rounded"',
      )
    }
    return `<mark${attrs} class="rss-content-mark px-0.5 rounded">`
  })
}

function processAbbr(html: string): string {
  return html.replaceAll(/<abbr([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        'class="$1 rss-content-abbr border-b border-dashed cursor-help"',
      )
    }
    return `<abbr${attrs} class="rss-content-abbr border-b border-dashed cursor-help">`
  })
}

function processHr(html: string): string {
  return html.replaceAll(/<hr([^>]*)>/gi, (_match, attrs) => {
    return `<hr${attrs} class="rss-content-hr border-0 h-px my-8">`
  })
}

function processSemanticTags(html: string): string {
  let result = html

  result = result.replaceAll(
    /<article([^>]*)>/gi,
    '<article$1 class="rss-content-article">',
  )

  result = result.replaceAll(
    /<section([^>]*)>/gi,
    '<section$1 class="rss-content-section">',
  )

  const asideClass = 'rss-content-aside my-4 p-4 rounded-xl opacity-80'
  result = result.replaceAll(/<aside([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replaceAll(
        /class\s*=\s*["']([^"']*)["']/gi,
        `class="$1 ${asideClass}"`,
      )
    }
    return `<aside${attrs} class="${asideClass}">`
  })

  result = result.replaceAll(
    /<header([^>]*)>/gi,
    '<header$1 class="rss-content-header mb-4">',
  )
  result = result.replaceAll(
    /<footer([^>]*)>/gi,
    '<footer$1 class="rss-content-footer mt-4 text-sm opacity-70">',
  )

  return result
}

/** <s>/<u> must not match strong/ul. */
function processInlineFormatting(html: string): string {
  let result = html

  result = result.replaceAll(
    /<del(\s[^>]*)?>/gi,
    '<del$1 class="rss-content-del line-through opacity-60">',
  )
  result = result.replaceAll(
    /<strike(\s[^>]*)?>/gi,
    '<strike$1 class="rss-content-del line-through opacity-60">',
  )
  result = result.replaceAll(
    /<s(\s[^>]*)?>(?![a-z])/gi,
    '<s$1 class="rss-content-del line-through opacity-60">',
  )

  result = result.replaceAll(
    /<ins(\s[^>]*)?>/gi,
    '<ins$1 class="rss-content-ins underline">',
  )
  result = result.replaceAll(
    /<u(\s[^>]*)?>(?![a-z])/gi,
    '<u$1 class="rss-content-ins underline">',
  )

  result = result.replaceAll(
    /<small(\s[^>]*)?>/gi,
    '<small$1 class="rss-content-small text-[0.85em] opacity-80">',
  )

  result = result.replaceAll(
    /<sup(\s[^>]*)?>/gi,
    '<sup$1 class="rss-content-sup text-[0.75em]">',
  )
  result = result.replaceAll(
    /<sub(\s[^>]*)?>/gi,
    '<sub$1 class="rss-content-sub text-[0.75em]">',
  )

  return result
}

function fixMalformedHtml(html: string): string {
  let result = html

  const hasMalformedTags =
    /(?:^|[^<])(iframe\s[^<]*\/iframe)/i.test(result) ||
    /(?:^|[^<])(img\s+src=)/i.test(result)

  if (hasMalformedTags) {
    result = result.replaceAll(/\/iframe(?![a-z])/gi, '~CLOSE_IFRAME~')

    result = result.replaceAll(
      /~CLOSE_IFRAME~br(?![a-z])/gi,
      '~CLOSE_IFRAME~<br/>',
    )
    result = result.replaceAll(/([a-z0-9"'])br(?=img|iframe|p|div|$)/gi, '$1<br/>')

    result = result.replaceAll(
      /(?:^|(?<=[>\s]))iframe(\s[^~]*)~CLOSE_IFRAME~/gi,
      '<iframe$1></iframe>',
    )
    result = result.replaceAll(/(?<![</a-z])iframe\s/gi, '<iframe ')

    result = result.replace(
      /(?<![</a-z])img\s+(src=[^\s<>]*(?:\s+[a-z]+=(?:"[^"]*"|[^\s<>"]*))*)/gi,
      '<img $1/>',
    )

    result = result.replaceAll('~CLOSE_IFRAME~', '</iframe>')
  }

  result = result.replaceAll('amp;', '&')

  const attrNames =
    'width|height|src|href|class|id|style|alt|title|frameborder|allowfullscreen|loading|referrerpolicy|data-[a-z-]+'
  result = result.replaceAll(new RegExp(`(\\d)(${attrNames})=`, 'gi'), '$1 $2=')
  result = result.replaceAll(new RegExp(`(["'])(${attrNames})=`, 'gi'), '$1 $2=')
  result = result.replaceAll(
    new RegExp(
      `(allowfullscreen|readonly|disabled|checked|selected)(${attrNames})=`,
      'gi',
    ),
    '$1 $2=',
  )

  result = result.replaceAll(/\s(src|href)=([^"'\s>][^\s>]*)/gi, ' $1="$2"')

  result = result.replaceAll(/<br\s*>/gi, '<br/>')
  result = result.replaceAll(/<hr\s*>/gi, '<hr/>')
  result = result.replaceAll(/<img([^>]*)(?<!\/)>/gi, '<img$1/>')

  result = result.replaceAll(
    /<iframe([^>]*)>(?![\s\S]*?<\/iframe>)/gi,
    '<iframe$1></iframe>',
  )

  if (/^iframe\s/i.test(result)) {
    result = `<${result}`
  }

  return result
}

function processRssHubSpecific(html: string): string {
  let result = html

  result = result.replaceAll(
    /<time([^>]*)>([^<]*)<\/time>/gi,
    (_match, attrs, content) => {
      return `<time${attrs} class="rss-content-time tabular-nums">${content}</time>`
    },
  )

  result = result.replaceAll(
    /<author>([^<]*)<\/author>/gi,
    '<span class="rss-content-author font-medium">$1</span>',
  )

  result = result.replaceAll(
    /<category>([^<]*)<\/category>/gi,
    '<span class="rss-content-category inline-block px-2 py-0.5 text-xs rounded-full bg-black/5 dark:bg-white/10 mr-1">$1</span>',
  )

  return result
}

function processSourceSpecific(html: string, _options: ProcessOptions): string {
  let result = html

  result = result.replaceAll(/<img[^>]*class="[^"]*rich_pages[^"]*"[^>]*>/gi, '')
  result = result.replaceAll(/<img[^>]*class="[^"]*wx_profile[^"]*"[^>]*>/gi, '')

  result = result.replaceAll(
    /<div[^>]*class="[^"]*ad[^"]*"[^>]*>[\s\S]*?<\/div>/gi,
    '',
  )
  result = result.replaceAll(
    /<aside[^>]*class="[^"]*ad[^"]*"[^>]*>[\s\S]*?<\/aside>/gi,
    '',
  )

  result = result.replaceAll(
    /<a[^>]*>[\s\S]*?(阅读原文|点击阅读|查看原文|Read more|Continue reading)[\s\S]*?<\/a>/gi,
    '',
  )

  result = result.replaceAll(
    /<p[^>]*>[\s\S]*?(订阅|RSS|Feed|Subscribe)[\s\S]*?<\/p>$/gi,
    '',
  )

  return result
}

function emptyRssContent(): string {
  return `<p class="opacity-50">${currentCopy().common.noContent}</p>`
}

function rssPipeline(
  opts: ProcessOptions,
): Array<(html: string) => string> {
  return [
    fixMalformedHtml,
    sanitizeRssHtml,
    (html) => processSourceSpecific(html, opts),
    processRssHubSpecific,
    processSemanticTags,
    processFigures,
    processDetails,
    processDescriptionLists,
    processTables,
    (html) => processImages(html, opts),
    processVideos,
    processAudio,
    stripUntrustedIframes,
    processBlockquotes,
    processCodeBlocks,
    processKbd,
    processMark,
    processAbbr,
    processHr,
    processInlineFormatting,
    (html) => processLinks(html, opts),
    (html) => (opts.removeEmptyTags ? removeEmptyTags(html) : html),
    (html) => html.trim(),
    sanitizeRssHtml,
  ]
}

export function processRssContent(
  html: string,
  options: Partial<ProcessOptions> = {},
): string {
  if (!html || typeof html !== 'string') return emptyRssContent()
  const opts: ProcessOptions = { ...DEFAULT_OPTIONS, ...options }
  let result = html
  for (const step of rssPipeline(opts)) result = step(result)
  return result
}

export async function processRssContentAsync(
  html: string,
  options: Partial<ProcessOptions> = {},
  signal?: AbortSignal,
): Promise<string> {
  if (!html || typeof html !== 'string') return emptyRssContent()
  const opts: ProcessOptions = { ...DEFAULT_OPTIONS, ...options }
  const slice = { ms: performance.now() }
  let result = html
  for (const step of rssPipeline(opts)) {
    signal?.throwIfAborted()
    result = step(result)
    await yieldIfSliceExceeded(slice)
  }
  return result
}

export function selfCheckSanitize(): string[] {
  const failures: string[] = []
  const xss = processRssContent(
    '<p>hi</p><script>alert(1)</script><img src=x onerror=alert(1)><iframe src="https://evil.example/phish"></iframe>',
  )
  if (/<script/i.test(xss)) failures.push('script tag survived')
  if (/onerror/i.test(xss)) failures.push('onerror survived')
  if (/evil\.example/i.test(xss)) failures.push('untrusted iframe survived')

  const nested = sanitizeRssHtml(
    '<div><scr<script>ipt>alert(1)</script></div><img src=x onerror="alert(1)">',
  )
  if (/<script/i.test(nested) || /onerror/i.test(nested)) {
    failures.push('nested/mutated XSS survived sanitizeRssHtml')
  }

  const trusted = processRssContent(
    '<iframe src="//player.bilibili.com/player.html?bvid=BV1xx411c7XW"></iframe>',
  )
  if (!/player\.bilibili\.com/i.test(trusted)) {
    failures.push('trusted bilibili iframe stripped')
  }

  const yt = processRssContent(
    '<iframe src="https://www.youtube.com/embed/dQw4w9WgXcQ"></iframe>',
  )
  if (!/youtube\.com/i.test(yt)) {
    failures.push('trusted youtube iframe stripped')
  }

  if (!isTrustedIframeHost('open.spotify.com')) {
    failures.push('spotify host not trusted')
  }
  if (isTrustedIframeHost('evil.example')) {
    failures.push('evil host incorrectly trusted')
  }

  const jsLink = processRssContent('<a href="javascript:alert(1)">x</a>')
  if (/javascript:/i.test(jsLink)) failures.push('javascript: href survived')

  const normal = processRssContent(
    '<p>Hello <strong>world</strong></p><a href="https://example.com">link</a><img src="https://cdn.example.com/a.jpg" alt="cover">',
  )
  if (!/<strong/i.test(normal)) failures.push('strong formatting stripped')
  if (!normal.includes('href=') || !normal.includes('example.com')) {
    failures.push('link stripped')
  }
  if (!/<img\b/i.test(normal)) failures.push('image stripped')

  return failures
}
