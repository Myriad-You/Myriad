/**
 * RSS 内容处理器
 *
 * 专门处理 RSSHub 等 RSS 源返回的内容，适配各种 HTML 标签和格式
 * 参考 RSSHub 文档: https://docs.rsshub.app/zh/guide/parameters
 */

/**
 * RSSHub 常见的内容格式:
 * - 图片: <img>, <figure>, <picture>
 * - 视频: <video>, <iframe> (YouTube, Bilibili等)
 * - 音频: <audio>
 * - 代码: <pre>, <code>
 * - 引用: <blockquote>
 * - 表格: <table>
 * - 列表: <ul>, <ol>, <dl>
 * - 描述列表: <dl>, <dt>, <dd>
 * - 详情折叠: <details>, <summary>
 * - 文章结构: <article>, <section>, <aside>, <header>, <footer>
 * - 数学公式: <math> (MathML) 或 KaTeX 格式
 * - 来自各类源的特殊标签和样式
 */

export interface ProcessOptions {
  /** 是否暗色模式 */
  isDark?: boolean
  /** 最大图片宽度 */
  maxImageWidth?: number
  /** 是否懒加载图片 */
  lazyLoadImages?: boolean
  /** 是否移除跟踪参数 */
  removeTrackingParams?: boolean
  /** 是否清理空标签 */
  removeEmptyTags?: boolean
  /** 是否转换相对 URL */
  baseUrl?: string
}

const DEFAULT_OPTIONS: ProcessOptions = {
  isDark: false,
  lazyLoadImages: true,
  removeTrackingParams: true,
  removeEmptyTags: true,
}

// API URL for image proxy
const API_URL = typeof window !== 'undefined'
  ? (import.meta.env?.PUBLIC_API_URL || '')
  : ''

/**
 * 获取代理后的图片 URL
 * 外部图片通过代理访问，避免 CORS 问题
 */
function getProxiedImageUrl(src: string): string {
  // 已经是 data URL，直接返回
  if (src.startsWith('data:'))
    return src

  // 已经是本地 API 路径，直接返回
  if (src.startsWith('/api/') || src.startsWith(`${API_URL}/api/`))
    return src

  // 外部 URL，使用图片代理
  if (src.startsWith('http://') || src.startsWith('https://')) {
    return `${API_URL}/api/proxy/image?url=${encodeURIComponent(src)}`
  }

  // 其他情况（如相对路径），直接返回
  return src
}

/**
 * 需要移除的 URL 跟踪参数
 */
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
  // 微信
  'chksm',
  'mpshare',
  'isappinstalled',
  'from_msgid',
  'from_itemidx',
  // 微博
  'weibo_id',
  'mb_id',
  'is_hot',
  'hottop_id',
  // 知乎
  'traffic_source',
  'traffic_medium',
  'traffic_campaign',
]

/**
 * 危险标签 - 需要完全移除
 */
const DANGEROUS_TAGS = [
  'script',
  'noscript',
  'style',
  'link',
  'meta',
  'base',
  'object',
  'embed',
  'applet',
  'form',
  'input',
  'button',
  'select',
  'textarea',
  'option',
  'optgroup',
  'datalist',
]

/**
 * 需要移除的危险属性
 */
const DANGEROUS_ATTRS = [
  'onload',
  'onerror',
  'onclick',
  'onmouseover',
  'onmouseout',
  'onmousedown',
  'onmouseup',
  'onfocus',
  'onblur',
  'onchange',
  'onsubmit',
  'onreset',
  'onkeydown',
  'onkeyup',
  'onkeypress',
  'ondblclick',
  'oncontextmenu',
  'ondrag',
  'ondragstart',
  'ondragend',
  'ondrop',
  'onscroll',
  'onwheel',
  'ontouchstart',
  'ontouchmove',
  'ontouchend',
  'onpointerdown',
  'onpointerup',
  'formaction',
  'xlink:href',
  'xmlns',
  'srcdoc',
]

/**
 * 空内容标签 - 可以移除
 */
const EMPTY_CONTENT_TAGS = ['p', 'div', 'span', 'section', 'article']

/**
 * 清理 URL 中的跟踪参数
 */
function cleanUrl(url: string): string {
  try {
    const parsed = new URL(url)
    TRACKING_PARAMS.forEach((param) => {
      parsed.searchParams.delete(param)
    })
    return parsed.toString()
  }
  catch {
    return url
  }
}

/**
 * 移除危险标签
 */
function removeDangerousTags(html: string): string {
  const tagPattern = DANGEROUS_TAGS.join('|')
  const regex = new RegExp(`<(${tagPattern})[^>]*>([\\s\\S]*?)<\\/\\1>|<(${tagPattern})[^>]*\\/?>`, 'gi')
  return html.replace(regex, '')
}

/**
 * 移除危险属性
 */
function removeDangerousAttrs(html: string): string {
  const attrPattern = DANGEROUS_ATTRS.join('|')
  const regex = new RegExp(`\\s(${attrPattern})\\s*=\\s*["'][^"']*["']|\\s(${attrPattern})\\s*=\\s*[^\\s>]+`, 'gi')
  return html.replace(regex, '')
}

/**
 * 移除 javascript: 协议链接
 */
function removeJavascriptLinks(html: string): string {
  return html.replace(/href\s*=\s*["']javascript:[^"']*["']/gi, 'href="#"')
}

/**
 * 移除空标签
 */
function removeEmptyTags(html: string): string {
  const tagPattern = EMPTY_CONTENT_TAGS.join('|')
  // 递归移除空标签（因为移除内层空标签后外层也可能变成空的）
  let result = html
  let prevLength = 0
  while (result.length !== prevLength) {
    prevLength = result.length
    // 匹配只包含空白字符或 &nbsp; 的标签
    const regex = new RegExp(`<(${tagPattern})[^>]*>\\s*(<br\\s*\\/?>\\s*)*<\\/\\1>`, 'gi')
    result = result.replace(regex, '')
    // 移除只有 &nbsp; 的标签
    const nbspRegex = new RegExp(`<(${tagPattern})[^>]*>(&nbsp;|\\s)*<\\/\\1>`, 'gi')
    result = result.replace(nbspRegex, '')
  }
  return result
}

/**
 * 处理图片标签
 */
function processImages(html: string, options: ProcessOptions): string {
  // 处理 <img> 标签 - 只处理 src，样式由渲染层处理
  const result = html.replace(/<img([^>]*)>/gi, (match, attrs) => {
    // 提取 src
    const srcMatch = attrs.match(/src\s*=\s*["']([^"']+)["']/i)
    if (!srcMatch)
      return match

    let src = srcMatch[1]

    // 清理跟踪参数
    if (options.removeTrackingParams) {
      src = cleanUrl(src)
    }

    // 转换相对 URL
    if (options.baseUrl && !src.startsWith('http') && !src.startsWith('data:')) {
      try {
        src = new URL(src, options.baseUrl).toString()
      }
      catch {
        // 忽略无效 URL
      }
    }

    // 通过代理访问外部图片（避免 CORS 问题）
    src = getProxiedImageUrl(src)

    // 构建新属性 - 只保留必要属性，样式由渲染层添加
    const newAttrs: string[] = [
      `src="${src}"`,
      'data-rss-image="true"',
    ]

    // 懒加载
    if (options.lazyLoadImages) {
      newAttrs.push('loading="lazy"')
    }

    // 保留 alt
    const altMatch = attrs.match(/alt\s*=\s*["']([^"']*)["']/i)
    if (altMatch) {
      newAttrs.push(`alt="${altMatch[1]}"`)
    }

    // 保留 title
    const titleMatch = attrs.match(/title\s*=\s*["']([^"']*)["']/i)
    if (titleMatch) {
      newAttrs.push(`title="${titleMatch[1]}"`)
    }

    return `<img ${newAttrs.join(' ')}>`
  })

  // 处理 <picture> 标签 - 保留结构但处理内部 img
  // picture 已经在上面处理了内部的 img

  return result
}

/**
 * 处理 figure 和 figcaption
 */
function processFigures(html: string): string {
  // 为 figure 添加样式类
  let result = html.replace(/<figure([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(/class\s*=\s*["']([^"']*)["']/i, 'class="$1 rss-content-figure my-6"')
    }
    return `<figure${attrs} class="rss-content-figure my-6">`
  })

  // 为 figcaption 添加样式
  result = result.replace(/<figcaption([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(/class\s*=\s*["']([^"']*)["']/i, 'class="$1 rss-content-figcaption text-center text-sm mt-2 opacity-60"')
    }
    return `<figcaption${attrs} class="rss-content-figcaption text-center text-sm mt-2 opacity-60">`
  })

  return result
}

/**
 * 处理视频标签
 */
function processVideos(html: string): string {
  // 处理 <video> 标签
  const result = html.replace(/<video([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(/class\s*=\s*["']([^"']*)["']/i, 'class="$1 rss-content-video w-full rounded-xl my-4"')
    }
    return `<video${attrs} class="rss-content-video w-full rounded-xl my-4" controls>`
  })

  return result
}

/**
 * 处理音频标签
 */
function processAudio(html: string): string {
  return html.replace(/<audio([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(/class\s*=\s*["']([^"']*)["']/i, 'class="$1 rss-content-audio w-full my-4"')
    }
    return `<audio${attrs} class="rss-content-audio w-full my-4" controls>`
  })
}

/**
 * 处理 iframe 嵌入
 */
function processIframes(html: string): string {
  return html.replace(/<iframe([^>]*)>/gi, (match, attrs) => {
    // 提取 src
    const srcMatch = attrs.match(/src\s*=\s*["']([^"']+)["']/i)
    if (!srcMatch)
      return match

    const src = srcMatch[1]

    // 根据来源添加不同样式
    let aspectClass = 'aspect-video' // 默认 16:9

    // 音乐类嵌入使用不同比例
    if (src.includes('music.163.com') || src.includes('xiami.com') || src.includes('spotify.com')) {
      aspectClass = 'aspect-[3/1]' // 音乐播放器更扁
    }

    return `<div class="rss-content-iframe-wrapper ${aspectClass} my-4 rounded-xl overflow-hidden">
      <iframe${attrs} class="w-full h-full border-0" allowfullscreen loading="lazy">`
  })
}

/**
 * 处理引用块
 */
function processBlockquotes(html: string, isDark: boolean): string {
  const bgClass = isDark ? 'bg-white/5' : 'bg-black/[0.03]'

  return html.replace(/<blockquote([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(
        /class\s*=\s*["']([^"']*)["']/i,
        `class="$1 rss-content-blockquote ${bgClass} rounded-xl px-4 py-3 my-4 italic"`,
      )
    }
    return `<blockquote${attrs} class="rss-content-blockquote ${bgClass} rounded-xl px-4 py-3 my-4 italic">`
  })
}

/**
 * 处理代码块
 */
function processCodeBlocks(html: string, isDark: boolean): string {
  const bgClass = isDark ? 'bg-white/5' : 'bg-black/[0.03]'

  // 处理 <pre><code> 组合
  let result = html.replace(/<pre([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(
        /class\s*=\s*["']([^"']*)["']/i,
        `class="$1 rss-content-pre ${bgClass} rounded-xl p-4 my-4 overflow-x-auto text-sm"`,
      )
    }
    return `<pre${attrs} class="rss-content-pre ${bgClass} rounded-xl p-4 my-4 overflow-x-auto text-sm">`
  })

  // 处理内联 code（不在 pre 内的）
  // 这个比较复杂，需要避免处理 pre 内的 code
  const inlineCodeBg = isDark ? 'bg-white/10' : 'bg-black/5'
  result = result.replace(
    /(?<!<pre[^>]*>[\s\S]*?)<code(?![^>]*class=)([^>]*)>/gi,
    `<code$1 class="rss-content-inline-code ${inlineCodeBg} px-1.5 py-0.5 rounded text-[0.9em]">`,
  )

  return result
}

/**
 * 处理表格
 */
function processTables(html: string, isDark: boolean): string {
  const borderClass = isDark ? 'border-white/10' : 'border-black/10'
  const headerBg = isDark ? 'bg-white/5' : 'bg-black/[0.03]'
  const stripeBg = isDark ? 'bg-white/[0.02]' : 'bg-black/[0.01]'

  let result = html

  // 包装表格以支持横向滚动
  result = result.replace(/<table([^>]*)>/gi, (match, attrs) => {
    const tableClass = `rss-content-table w-full text-sm border-collapse rounded-xl overflow-hidden border ${borderClass}`
    if (attrs.includes('class=')) {
      const newTag = match.replace(/class\s*=\s*["']([^"']*)["']/i, `class="$1 ${tableClass}"`)
      return `<div class="rss-content-table-wrapper overflow-x-auto my-4 rounded-xl">${newTag}`
    }
    return `<div class="rss-content-table-wrapper overflow-x-auto my-4 rounded-xl"><table${attrs} class="${tableClass}">`
  })

  // 关闭包装
  result = result.replace(/<\/table>/gi, '</table></div>')

  // 处理表头
  result = result.replace(/<thead([^>]*)>/gi, `<thead$1 class="rss-content-thead ${headerBg}">`)
  result = result.replace(/<th([^>]*)>/gi, `<th$1 class="rss-content-th py-2 px-3 text-left font-medium border-b ${borderClass}">`)

  // 处理表格行
  result = result.replace(/<tr([^>]*)>/gi, `<tr$1 class="rss-content-tr even:${stripeBg}">`)
  result = result.replace(/<td([^>]*)>/gi, `<td$1 class="rss-content-td py-2 px-3 border-b ${borderClass}">`)

  return result
}

/**
 * 处理描述列表
 */
function processDescriptionLists(html: string): string {
  let result = html

  result = result.replace(/<dl([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(/class\s*=\s*["']([^"']*)["']/i, 'class="$1 rss-content-dl my-4"')
    }
    return `<dl${attrs} class="rss-content-dl my-4">`
  })

  result = result.replace(/<dt([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(/class\s*=\s*["']([^"']*)["']/i, 'class="$1 rss-content-dt font-semibold mt-2"')
    }
    return `<dt${attrs} class="rss-content-dt font-semibold mt-2">`
  })

  result = result.replace(/<dd([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(/class\s*=\s*["']([^"']*)["']/i, 'class="$1 rss-content-dd ml-4 pl-4 mt-1"')
    }
    return `<dd${attrs} class="rss-content-dd ml-4 pl-4 mt-1">`
  })

  return result
}

/**
 * 处理 details/summary 折叠组件
 */
function processDetails(html: string, isDark: boolean): string {
  const bgClass = isDark ? 'bg-white/5' : 'bg-black/[0.03]'
  const hoverBg = isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'

  let result = html

  result = result.replace(/<details([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(/class\s*=\s*["']([^"']*)["']/i, `class="$1 rss-content-details ${bgClass} rounded-xl my-4 overflow-hidden"`)
    }
    return `<details${attrs} class="rss-content-details ${bgClass} rounded-xl my-4 overflow-hidden">`
  })

  result = result.replace(/<summary([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(/class\s*=\s*["']([^"']*)["']/i, `class="$1 rss-content-summary cursor-pointer py-3 px-4 font-medium select-none ${hoverBg} transition-colors"`)
    }
    return `<summary${attrs} class="rss-content-summary cursor-pointer py-3 px-4 font-medium select-none ${hoverBg} transition-colors">`
  })

  return result
}

/**
 * 处理链接
 */
function processLinks(html: string, options: ProcessOptions): string {
  return html.replace(/<a([^>]*)>/gi, (match, attrs) => {
    // 提取 href
    const hrefMatch = attrs.match(/href\s*=\s*["']([^"']+)["']/i)
    if (!hrefMatch)
      return match

    let href = hrefMatch[1]

    // 清理跟踪参数
    if (options.removeTrackingParams) {
      href = cleanUrl(href)
    }

    // 转换相对 URL
    if (options.baseUrl && !href.startsWith('http') && !href.startsWith('#') && !href.startsWith('mailto:')) {
      try {
        href = new URL(href, options.baseUrl).toString()
      }
      catch {
        // 忽略
      }
    }

    // 构建新属性
    let newAttrs = attrs.replace(/href\s*=\s*["'][^"']+["']/i, `href="${href}"`)

    // 外部链接添加安全属性
    if (href.startsWith('http')) {
      if (!newAttrs.includes('target=')) {
        newAttrs += ' target="_blank"'
      }
      if (!newAttrs.includes('rel=')) {
        newAttrs += ' rel="noopener noreferrer"'
      }
    }

    // 添加样式类
    if (!newAttrs.includes('class=')) {
      newAttrs += ' class="rss-content-link text-inherit underline underline-offset-2 decoration-1 break-words"'
    }

    return `<a${newAttrs}>`
  })
}

/**
 * 处理 kbd 按键标签
 */
function processKbd(html: string, isDark: boolean): string {
  const bgClass = isDark ? 'bg-white/10' : 'bg-black/5'
  const borderClass = isDark ? 'border-white/20' : 'border-black/10'

  return html.replace(/<kbd([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(
        /class\s*=\s*["']([^"']*)["']/i,
        `class="$1 rss-content-kbd ${bgClass} ${borderClass} border px-1.5 py-0.5 rounded text-[0.85em] font-mono"`,
      )
    }
    return `<kbd${attrs} class="rss-content-kbd ${bgClass} ${borderClass} border px-1.5 py-0.5 rounded text-[0.85em] font-mono">`
  })
}

/**
 * 处理 mark 高亮标签
 */
function processMark(html: string, isDark: boolean): string {
  const bgClass = isDark ? 'bg-yellow-500/30' : 'bg-yellow-200/60'

  return html.replace(/<mark([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(
        /class\s*=\s*["']([^"']*)["']/i,
        `class="$1 rss-content-mark ${bgClass} px-0.5 rounded"`,
      )
    }
    return `<mark${attrs} class="rss-content-mark ${bgClass} px-0.5 rounded">`
  })
}

/**
 * 处理 abbr 缩写标签
 */
function processAbbr(html: string, isDark: boolean): string {
  const borderClass = isDark ? 'border-white/30' : 'border-black/30'

  return html.replace(/<abbr([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(
        /class\s*=\s*["']([^"']*)["']/i,
        `class="$1 rss-content-abbr border-b border-dashed ${borderClass} cursor-help"`,
      )
    }
    return `<abbr${attrs} class="rss-content-abbr border-b border-dashed ${borderClass} cursor-help">`
  })
}

/**
 * 处理水平分隔线
 */
function processHr(html: string, isDark: boolean): string {
  const bgClass = isDark ? 'bg-white/10' : 'bg-black/10'

  return html.replace(/<hr([^>]*)\/?>/gi, (match, attrs) => {
    return `<hr${attrs} class="rss-content-hr border-0 h-px ${bgClass} my-8">`
  })
}

/**
 * 处理文章语义标签
 */
function processSemanticTags(html: string): string {
  // 简化处理，保留语义但移除可能干扰的样式
  let result = html

  // article
  result = result.replace(/<article([^>]*)>/gi, '<article$1 class="rss-content-article">')

  // section
  result = result.replace(/<section([^>]*)>/gi, '<section$1 class="rss-content-section">')

  // aside - 侧边栏内容，给予特殊样式
  const asideClass = 'rss-content-aside my-4 p-4 rounded-xl opacity-80'
  result = result.replace(/<aside([^>]*)>/gi, (match, attrs) => {
    if (attrs.includes('class=')) {
      return match.replace(/class\s*=\s*["']([^"']*)["']/i, `class="$1 ${asideClass}"`)
    }
    return `<aside${attrs} class="${asideClass}">`
  })

  // header/footer 在文章内容中通常是元信息
  result = result.replace(/<header([^>]*)>/gi, '<header$1 class="rss-content-header mb-4">')
  result = result.replace(/<footer([^>]*)>/gi, '<footer$1 class="rss-content-footer mt-4 text-sm opacity-70">')

  return result
}

/**
 * 处理行内格式标签
 * 注意：正则必须精确匹配标签名，避免匹配到其他标签
 * 例如 <s> 不能匹配 <strong>、<span>；<u> 不能匹配 <ul>
 */
function processInlineFormatting(html: string): string {
  let result = html

  // 删除线 - 分别处理每个标签
  // <del> 标签：完整标签名，不会有歧义
  result = result.replace(/<del(\s[^>]*)?>/gi, '<del$1 class="rss-content-del line-through opacity-60">')
  // <strike> 标签：完整标签名
  result = result.replace(/<strike(\s[^>]*)?>/gi, '<strike$1 class="rss-content-del line-through opacity-60">')
  // <s> 标签：必须后面是 > 或空格+属性，不能是字母（排除 strong, span, section, small, sub, sup, svg, style 等）
  result = result.replace(/<s(\s[^>]*)?>(?![a-z])/gi, '<s$1 class="rss-content-del line-through opacity-60">')

  // 插入标签 - <ins> 不会有歧义
  result = result.replace(/<ins(\s[^>]*)?>/gi, '<ins$1 class="rss-content-ins underline">')
  // 下划线 <u> 标签：必须后面是 > 或空格+属性，不能是字母（排除 ul 等）
  result = result.replace(/<u(\s[^>]*)?>(?![a-z])/gi, '<u$1 class="rss-content-ins underline">')

  // 小号文本 - <small> 不会有歧义
  result = result.replace(/<small(\s[^>]*)?>/gi, '<small$1 class="rss-content-small text-[0.85em] opacity-80">')

  // 上标/下标 - <sup> 和 <sub> 不会有歧义
  result = result.replace(/<sup(\s[^>]*)?>/gi, '<sup$1 class="rss-content-sup text-[0.75em]">')
  result = result.replace(/<sub(\s[^>]*)?>/gi, '<sub$1 class="rss-content-sub text-[0.75em]">')

  return result
}

/**
 * 修复损坏的 HTML 格式
 * 某些 RSS 源返回的 HTML 格式不标准，需要预处理
 *
 * 典型的损坏例子（没有尖括号、属性无空格）：
 * iframe width=640height=360src=https://...frameborder=0allowfullscreen=referrerpolicy=no-referrer/iframebrimg src=...br
 */
function fixMalformedHtml(html: string): string {
  let result = html

  // ========== 第一阶段：处理完全缺失尖括号的标签 ==========

  // 检测是否存在缺失尖括号的标签模式
  // 典型特征: "iframe " 开头 + "/iframe" 结尾，但没有 <> 包裹
  const hasMalformedTags = /(?:^|[^<])(iframe\s[^<]*\/iframe)/i.test(result)
    || /(?:^|[^<])(img\s+src=)/i.test(result)

  if (hasMalformedTags) {
    // 先处理 /iframe -> </iframe>
    result = result.replace(/\/iframe(?![a-z])/gi, '~CLOSE_IFRAME~')

    // 处理 br（可能粘连在其他内容后面）
    // 例如: referrer/iframebrimg -> referrer/iframe<br/>img
    result = result.replace(/~CLOSE_IFRAME~br(?![a-z])/gi, '~CLOSE_IFRAME~<br/>')
    result = result.replace(/([a-z0-9"'])br(?=img|iframe|p|div|$)/gi, '$1<br/>')

    // 现在处理 iframe 开始标签
    // 匹配: iframe + 属性内容 + ~CLOSE_IFRAME~
    result = result.replace(/(?:^|(?<=[>\s]))iframe(\s[^~]*)~CLOSE_IFRAME~/gi, '<iframe$1></iframe>')
    result = result.replace(/(?<![</a-z])iframe\s/gi, '<iframe ')

    // 处理 img 标签（可能粘连）
    // 例如: <br/>img src=... referrerpolicy=no-referrer<br/>
    result = result.replace(/(?<![</a-z])img\s+(src=[^\s<>]*(?:\s+[a-z]+=(?:"[^"]*"|[^\s<>]*))*)/gi, '<img $1/>')

    // 清理剩余的标记
    result = result.replace(/~CLOSE_IFRAME~/g, '</iframe>')
  }

  // ========== 第二阶段：修复属性格式问题 ==========

  // 1. 修复 HTML 实体编码问题（amp; 变成 &）
  result = result.replace(/amp;/g, '&')

  // 2. 修复缺少空格的属性（如 width=640height=360）
  // 匹配 数字或引号结尾 后面直接跟 字母开头的属性名
  const attrNames = 'width|height|src|href|class|id|style|alt|title|frameborder|allowfullscreen|loading|referrerpolicy|data-[a-z-]+'
  result = result.replace(new RegExp(`(\\d)(${attrNames})=`, 'gi'), '$1 $2=')
  result = result.replace(new RegExp(`(["'])(${attrNames})=`, 'gi'), '$1 $2=')
  // 修复无值属性后面紧跟的属性
  result = result.replace(new RegExp(`(allowfullscreen|readonly|disabled|checked|selected)(${attrNames})=`, 'gi'), '$1 $2=')

  // 3. 修复无引号的属性值（为常见属性添加引号）
  // src=https://... -> src="https://..."
  result = result.replace(/\s(src|href)=([^"'\s>][^\s>]*)/gi, ' $1="$2"')

  // 4. 修复自闭合标签
  result = result.replace(/<br\s*>/gi, '<br/>')
  result = result.replace(/<hr\s*>/gi, '<hr/>')
  // 确保 img 标签正确闭合（如果还没有）
  result = result.replace(/<img([^>]*)(?<!\/)>/gi, '<img$1/>')

  // 5. 修复缺少闭合的 iframe 标签
  result = result.replace(/<iframe([^>]*)>(?![\s\S]*?<\/iframe>)/gi, '<iframe$1></iframe>')

  // ========== 第三阶段：处理开头就是没有 < 的 iframe ==========
  // 处理整段内容开头就是 "iframe" 的情况
  if (result.match(/^iframe\s/i)) {
    result = `<${result}`
  }

  return result
}

/**
 * 清理多余的空白和换行
 */
function normalizeWhitespace(html: string): string {
  // 移除标签之间的多余换行
  let result = html.replace(/>\s+</g, '> <')

  // 但保留 pre 内的空白
  // 这个需要更复杂的处理，暂时跳过

  // 移除开头和结尾的空白
  result = result.trim()

  return result
}

/**
 * 转换 RSSHub 特定的内容格式
 */
function processRssHubSpecific(html: string): string {
  let result = html

  // 处理 RSSHub 的时间戳格式 (有些源会包含)
  // 通常格式如: <time datetime="2024-01-01T00:00:00Z">
  result = result.replace(/<time([^>]*)>([^<]*)<\/time>/gi, (match, attrs, content) => {
    // 保留 time 标签但添加样式
    return `<time${attrs} class="rss-content-time tabular-nums">${content}</time>`
  })

  // 处理一些源会包含的作者信息
  result = result.replace(/<author>([^<]*)<\/author>/gi, '<span class="rss-content-author font-medium">$1</span>')

  // 处理类别标签
  result = result.replace(/<category>([^<]*)<\/category>/gi, '<span class="rss-content-category inline-block px-2 py-0.5 text-xs rounded-full bg-black/5 dark:bg-white/10 mr-1">$1</span>')

  return result
}

/**
 * 处理特定来源的内容格式
 * 针对不同 RSS 源的特殊处理
 */
function processSourceSpecific(html: string, options: ProcessOptions): string {
  let result = html

  // 移除微信文章的头像等干扰元素
  result = result.replace(/<img[^>]*class="[^"]*rich_pages[^"]*"[^>]*>/gi, '')
  result = result.replace(/<img[^>]*class="[^"]*wx_profile[^"]*"[^>]*>/gi, '')

  // 移除一些网站的广告占位
  result = result.replace(/<div[^>]*class="[^"]*ad[^"]*"[^>]*>[\s\S]*?<\/div>/gi, '')
  result = result.replace(/<aside[^>]*class="[^"]*ad[^"]*"[^>]*>[\s\S]*?<\/aside>/gi, '')

  // 移除 "阅读原文" 等重复链接
  result = result.replace(/<a[^>]*>[\s\S]*?(阅读原文|点击阅读|查看原文|Read more|Continue reading)[\s\S]*?<\/a>/gi, '')

  // 移除 RSS 底部的订阅提示
  result = result.replace(/<p[^>]*>[\s\S]*?(订阅|RSS|Feed|Subscribe)[\s\S]*?<\/p>$/gi, '')

  return result
}

/**
 * 主处理函数 - 处理 RSS 内容
 */
export function processRssContent(html: string, options: Partial<ProcessOptions> = {}): string {
  const opts: ProcessOptions = { ...DEFAULT_OPTIONS, ...options }

  if (!html || typeof html !== 'string') {
    return '<p class="opacity-50">暂无内容</p>'
  }

  let result = html

  // 0. 预处理 - 修复损坏的 HTML 格式
  result = fixMalformedHtml(result)

  // 1. 安全性处理 - 最优先
  result = removeDangerousTags(result)
  result = removeDangerousAttrs(result)
  result = removeJavascriptLinks(result)

  // 2. 来源特定处理
  result = processSourceSpecific(result, opts)
  result = processRssHubSpecific(result)

  // 3. 结构性标签处理
  result = processSemanticTags(result)
  result = processFigures(result)
  result = processDetails(result, opts.isDark || false)
  result = processDescriptionLists(result)
  result = processTables(result, opts.isDark || false)

  // 4. 媒体处理
  result = processImages(result, opts)
  result = processVideos(result)
  result = processAudio(result)
  // 注意：不再调用 processIframes，让 embedProcessor 统一处理

  // 5. 文本格式处理
  result = processBlockquotes(result, opts.isDark || false)
  result = processCodeBlocks(result, opts.isDark || false)
  result = processKbd(result, opts.isDark || false)
  result = processMark(result, opts.isDark || false)
  result = processAbbr(result, opts.isDark || false)
  result = processHr(result, opts.isDark || false)
  result = processInlineFormatting(result)

  // 6. 链接处理
  result = processLinks(result, opts)

  // 7. 清理
  if (opts.removeEmptyTags) {
    result = removeEmptyTags(result)
  }
  // 注意：不再调用 normalizeWhitespace，避免破坏 HTML 结构
  result = result.trim()

  return result
}

/**
 * 提取纯文本（用于摘要等）
 */
export function extractPlainText(html: string, maxLength?: number): string {
  if (!html)
    return ''

  // 移除 HTML 标签
  let text = html.replace(/<[^>]+>/g, ' ')

  // 解码 HTML 实体
  text = text
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, '\'')
    .replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number.parseInt(code)))
    .replace(/&[a-z]+;/gi, ' ')

  // 规范化空白
  text = text.replace(/\s+/g, ' ').trim()

  // 截断
  if (maxLength && text.length > maxLength) {
    text = `${text.slice(0, maxLength).trim()}...`
  }

  return text
}

/**
 * 检测内容是否主要是代码
 */
export function isCodeContent(html: string): boolean {
  const codePattern = /<(pre|code)[^>]*>/gi
  const matches = html.match(codePattern) || []
  const textLength = extractPlainText(html).length

  // 如果代码块数量较多或者内容较短但有代码，认为是代码内容
  return matches.length >= 3 || (matches.length > 0 && textLength < 500)
}

/**
 * 检测内容是否主要是图片
 */
export function isImageContent(html: string): boolean {
  const imgPattern = /<img[^>]+>/gi
  const matches = html.match(imgPattern) || []
  const textLength = extractPlainText(html).length

  // 图片多且文字少
  return matches.length >= 3 && textLength < 200
}

/**
 * 估算阅读时间（分钟）
 */
export function estimateReadingTime(html: string, wordsPerMinute = 300): number {
  const text = extractPlainText(html)
  // 中文按字符计数，英文按单词计数
  const chineseChars = (text.match(/[\u4E00-\u9FFF]/g) || []).length
  const englishWords = text.replace(/[\u4E00-\u9FFF]/g, '').split(/\s+/).filter(Boolean).length

  // 中文阅读速度约 300-400 字/分钟，英文约 200-250 词/分钟
  const totalChars = chineseChars + englishWords * 1.5
  const minutes = Math.ceil(totalChars / wordsPerMinute)

  return Math.max(1, minutes)
}

/**
 * 统计内容字数
 */
export function countWords(html: string): number {
  const text = extractPlainText(html)
  const chineseChars = (text.match(/[\u4E00-\u9FFF]/g) || []).length
  const englishWords = text.replace(/[\u4E00-\u9FFF]/g, '').split(/\s+/).filter(Boolean).length

  return chineseChars + englishWords
}

/**
 * 提取图片 URL 列表
 */
export function extractImageUrls(html: string): string[] {
  const imgPattern = /<img[^>]+src\s*=\s*["']([^"']+)["'][^>]*>/gi
  const urls: string[] = []
  let match

  while ((match = imgPattern.exec(html)) !== null) {
    if (match[1] && !match[1].startsWith('data:')) {
      urls.push(match[1])
    }
  }

  return urls
}

/**
 * 提取第一张图片作为封面
 */
export function extractCoverImage(html: string): string | null {
  const urls = extractImageUrls(html)

  // 过滤掉小图标和表情
  const validImages = urls.filter((url) => {
    const lower = url.toLowerCase()
    // 排除常见的表情、图标等
    if (lower.includes('emoji') || lower.includes('icon') || lower.includes('avatar')) {
      return false
    }
    // 排除 GIF 表情
    if (lower.includes('sticker') || (lower.includes('.gif') && lower.includes('face'))) {
      return false
    }
    return true
  })

  return validImages[0] || null
}

/**
 * 提取标题列表（用于生成目录）
 */
export function extractHeadings(html: string): Array<{ level: number, text: string, id: string }> {
  const headingPattern = /<h([1-6])([^>]*)>([^<]*(?:<[^/h][^>]*>[^<]*)*)<\/h\1>/gi
  const headings: Array<{ level: number, text: string, id: string }> = []
  let match
  let index = 0

  while ((match = headingPattern.exec(html)) !== null) {
    const level = Number.parseInt(match[1])
    const text = extractPlainText(match[3]).trim()

    if (text) {
      const id = `heading-${index}-${text.slice(0, 30).replace(/\s+/g, '-').toLowerCase()}`
      headings.push({ level, text, id })
      index++
    }
  }

  return headings
}

/**
 * 检测内容语言（简单判断）
 */
export function detectLanguage(html: string): 'zh' | 'en' | 'mixed' {
  const text = extractPlainText(html)
  const chineseChars = (text.match(/[\u4E00-\u9FFF]/g) || []).length
  const totalChars = text.replace(/\s/g, '').length

  if (totalChars === 0)
    return 'en'

  const chineseRatio = chineseChars / totalChars

  if (chineseRatio > 0.3)
    return 'zh'
  if (chineseRatio < 0.1)
    return 'en'
  return 'mixed'
}

/**
 * 清理 HTML 实体
 */
export function decodeHtmlEntities(text: string): string {
  const entities: Record<string, string> = {
    '&nbsp;': ' ',
    '&amp;': '&',
    '&lt;': '<',
    '&gt;': '>',
    '&quot;': '"',
    '&#39;': '\'',
    '&apos;': '\'',
    '&copy;': String.fromCharCode(169),
    '&reg;': String.fromCharCode(174),
    '&trade;': String.fromCharCode(8482),
    '&mdash;': String.fromCharCode(8212),
    '&ndash;': String.fromCharCode(8211),
    '&hellip;': String.fromCharCode(8230),
    '&lsquo;': String.fromCharCode(8216),
    '&rsquo;': String.fromCharCode(8217),
    '&ldquo;': String.fromCharCode(8220),
    '&rdquo;': String.fromCharCode(8221),
  }

  let result = text

  // 替换命名实体
  for (const [entity, char] of Object.entries(entities)) {
    result = result.replace(new RegExp(entity, 'g'), char)
  }

  // 替换数字实体 &#123;
  result = result.replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number.parseInt(code)))

  // 替换十六进制实体 &#x1F;
  result = result.replace(/&#x([0-9a-fA-F]+);/g, (_, code) => String.fromCharCode(Number.parseInt(code, 16)))

  return result
}

/**
 * 内容分析结果
 */
export interface ContentAnalysis {
  wordCount: number
  readingTime: number
  imageCount: number
  hasCode: boolean
  hasTable: boolean
  hasVideo: boolean
  language: 'zh' | 'en' | 'mixed'
  coverImage: string | null
  headings: Array<{ level: number, text: string, id: string }>
}

/**
 * 分析内容
 */
export function analyzeContent(html: string): ContentAnalysis {
  const imageUrls = extractImageUrls(html)

  return {
    wordCount: countWords(html),
    readingTime: estimateReadingTime(html),
    imageCount: imageUrls.length,
    hasCode: /<(pre|code)[^>]*>/i.test(html),
    hasTable: /<table[^>]*>/i.test(html),
    hasVideo: /<(video|iframe)[^>]*>/i.test(html),
    language: detectLanguage(html),
    coverImage: extractCoverImage(html),
    headings: extractHeadings(html),
  }
}

export default {
  processRssContent,
  extractPlainText,
  isCodeContent,
  isImageContent,
  estimateReadingTime,
  countWords,
  extractImageUrls,
  extractCoverImage,
  extractHeadings,
  detectLanguage,
  decodeHtmlEntities,
  analyzeContent,
}
