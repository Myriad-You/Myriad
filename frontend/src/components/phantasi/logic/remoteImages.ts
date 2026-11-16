/**
 * 笔记正文里的外站图片：找出来，限量并行地存进本站，再把正文里的地址换成本站的。
 * 外站地址可能失效，也会把访客的 IP 交给图床。
 * 这里只做查找、替换和调度；真正去存的那一步由调用方传进来（`services/remoteMediaApi`）。
 */

import { isSiteMediaPath } from '../../../utils/siteMediaUrl'

/** 一次粘贴、一篇导入最多存这么多张，再多的保留原地址。 */
export const REMOTE_IMAGE_LIMIT = 50
/** 同时在存的张数：服务端每张都要去外站下载。 */
export const REMOTE_IMAGE_CONCURRENCY = 3

export type SaveRemoteImage = (url: string, signal?: AbortSignal) => Promise<string>

interface Span {
  start: number
  end: number
}

/** `![说明](地址 "标题")` 里的地址，`<地址>` 写法也认。 */
const INLINE_IMAGE = /!\[(?:[^\]\\\n]|\\.)*\]\(\s*(<[^>\n]*>|[^\s)]+)/g
/** `![说明][标签]`、`![标签][]`、`![标签]`：地址在定义行上。 */
const REFERENCE_IMAGE = /!\[((?:[^\]\\\n]|\\.)*)\](?:\[((?:[^\]\\\n]|\\.)*)\])?(?!\()/g
/** `[标签]: 地址 "标题"`，最多缩进三格。 */
const DEFINITION = /^ {0,3}\[((?:[^\]\\\n]|\\.)+)\]:[ \t]*(<[^>\n]*>|\S+)/gm
const FENCE = /^ {0,3}(`{3,}|~{3,})[^`~\n]*\n[\s\S]*?(?:^ {0,3}\1[`~]*[ \t]*$|(?![\s\S]))/gm
const INLINE_CODE = /(`+)(?!`)[\s\S]*?[^`]\1(?!`)/g

/** 代码块和行内代码里的「图片」是字，不能动。 */
function codeSpans(md: string): Span[] {
  const spans: Span[] = []
  for (const match of md.matchAll(FENCE)) {
    spans.push({ start: match.index, end: match.index + match[0].length })
  }
  for (const match of md.matchAll(INLINE_CODE)) {
    const start = match.index
    if (spans.some((span) => start >= span.start && start < span.end)) continue
    spans.push({ start, end: start + match[0].length })
  }
  return spans
}

function inside(spans: readonly Span[], at: number): boolean {
  return spans.some((span) => at >= span.start && at < span.end)
}

function normalizeLabel(label: string): string {
  return label.trim().replaceAll(/\s+/g, ' ').toLowerCase()
}

/** 地址那一段在原文里的位置；`<地址>` 只算尖括号里面。 */
function destination(raw: string, at: number): Span & { url: string } {
  if (raw.startsWith('<') && raw.endsWith('>')) {
    return { start: at + 1, end: at + raw.length - 1, url: raw.slice(1, -1) }
  }
  return { start: at, end: at + raw.length, url: raw }
}

/** 正文里每张图片地址出现的位置：行内图片，和图片引用用到的定义行。 */
function imageDestinations(md: string): Array<Span & { url: string }> {
  const code = codeSpans(md)
  const found: Array<Span & { url: string }> = []
  for (const match of md.matchAll(INLINE_IMAGE)) {
    if (inside(code, match.index)) continue
    const raw = match[1]!
    found.push(destination(raw, match.index + match[0].length - raw.length))
  }
  const labels = new Set<string>()
  for (const match of md.matchAll(REFERENCE_IMAGE)) {
    if (inside(code, match.index)) continue
    const label = match[2]?.trim() ? match[2] : match[1]!
    labels.add(normalizeLabel(label))
  }
  if (labels.size > 0) {
    for (const match of md.matchAll(DEFINITION)) {
      if (inside(code, match.index) || !labels.has(normalizeLabel(match[1]!))) continue
      const raw = match[2]!
      found.push(destination(raw, match.index + match[0].length - raw.length))
    }
  }
  return found.sort((a, b) => a.start - b.start)
}

/** Markdown 里出现的图片地址，去重、按出现顺序。 */
export function markdownImageSources(md: string): string[] {
  return [...new Set(imageDestinations(md).map((found) => found.url))]
}

/** 只换图片地址，同样的地址写在链接、代码、正文文字里的都不动。 */
export function markdownImageSrcRanges(md: string, src: string): Span[] {
  return imageDestinations(md)
    .filter((found) => found.url === src)
    .map(({ start, end }) => ({ start, end }))
}

export function replaceMarkdownImageSrc(md: string, from: string, to: string): string {
  let out = md
  for (const { start, end } of markdownImageSrcRanges(md, from).reverse()) {
    out = out.slice(0, start) + to + out.slice(end)
  }
  return out
}

function decodeAttr(value: string): string {
  return value
    .replaceAll('&quot;', '"')
    .replaceAll('&#39;', '\'')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&amp;', '&')
}

/** 洗过的可视层 HTML 里 `<img>` 的地址（写在 data-src 的是原地址）。 */
export function htmlImageSources(html: string): string[] {
  const found: string[] = []
  for (const match of html.matchAll(/<img\b([^>]*)>/gi)) {
    const attrs = match[1]!
    const raw = /\bdata-src="([^"]*)"/i.exec(attrs)?.[1] ?? /\bsrc="([^"]*)"/i.exec(attrs)?.[1]
    if (raw) found.push(decodeAttr(raw))
  }
  return [...new Set(found)]
}

/**
 * 要存进本站的图：别的网站上的 http(s) 地址。本站地址、data:、相对路径、
 * 本站媒体路径（哪怕挂着旧域名）都不算；带用户名密码的地址服务端不去拉，也不算。
 */
export function isExternalImageUrl(src: string, siteOrigins: readonly string[]): boolean {
  let url: URL
  try {
    url = new URL(src.trim())
  } catch {
    return false
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return false
  if (url.username || url.password) return false
  if (siteOrigins.includes(url.origin)) return false
  return !isSiteMediaPath(url.pathname)
}

function hostOf(url: string): string {
  try {
    return new URL(url).host
  } catch {
    return ''
  }
}

/**
 * 同一个地址只存一次：在存的和存好的都复用；失败的不记，下次还能再试。
 * `giveUpAfter`：同一个外站接连失败这么多次、一张都没存成，就当它已经关站，
 * 余下的图不再去等（导入旧站时，旧站常常已经关了，每张都要等到超时）。
 */
export function rememberSaves(
  save: SaveRemoteImage,
  options: { giveUpAfter?: number } = {},
): SaveRemoteImage {
  const known = new Map<string, Promise<string>>()
  const failures = new Map<string, number>()
  const giveUpAfter = options.giveUpAfter ?? 0
  return (url, signal) => {
    const existing = known.get(url)
    if (existing) return existing
    const host = hostOf(url)
    if (giveUpAfter > 0 && (failures.get(host) ?? 0) >= giveUpAfter) {
      return Promise.reject(new Error('host unreachable'))
    }
    const pending = save(url, signal)
    known.set(url, pending)
    pending.then(
      () => failures.set(host, -Infinity),
      () => {
        if (known.get(url) === pending) known.delete(url)
        failures.set(host, (failures.get(host) ?? 0) + 1)
      },
    )
    return pending
  }
}

export interface KeepRemoteImagesOptions {
  limit?: number
  concurrency?: number
  signal?: AbortSignal
  /** 每张存好就回调一次，调用方可以马上换地址，不必等全部做完。 */
  onSaved?: (from: string, to: string) => void
  onProgress?: (done: number, total: number) => void
}

export interface KeptRemoteImages {
  /** 原地址 → 本站地址。 */
  saved: Map<string, string>
  /** 没能存下（失败、超出上限、中途取消）而保留原地址的。 */
  kept: string[]
}

/** 把这些外站图片存进本站。一张失败不影响别的，整个过程不会抛错。 */
export async function keepRemoteImages(
  urls: readonly string[],
  save: SaveRemoteImage,
  options: KeepRemoteImagesOptions = {},
): Promise<KeptRemoteImages> {
  const unique = [...new Set(urls)]
  const limit = Math.max(0, options.limit ?? REMOTE_IMAGE_LIMIT)
  const queue = unique.slice(0, limit)
  const saved = new Map<string, string>()
  const kept = unique.slice(limit)
  let done = 0
  let next = 0
  options.onProgress?.(0, queue.length)
  const worker = async () => {
    while (next < queue.length) {
      const url = queue[next++]!
      if (options.signal?.aborted) {
        kept.push(url)
        continue
      }
      try {
        const local = (await save(url, options.signal)).trim()
        if (!local || local === url) throw new Error('not saved')
        saved.set(url, local)
        options.onSaved?.(url, local)
      } catch {
        kept.push(url)
      }
      done += 1
      options.onProgress?.(done, queue.length)
    }
  }
  const workers = Math.max(1, Math.min(options.concurrency ?? REMOTE_IMAGE_CONCURRENCY, queue.length))
  await Promise.all(Array.from({ length: workers }, worker))
  return { saved, kept }
}

/** 一篇正文：把外站图片存进本站，换好地址后交回。 */
export async function keepMarkdownImages(
  md: string,
  siteOrigins: readonly string[],
  save: SaveRemoteImage,
  options: KeepRemoteImagesOptions = {},
): Promise<{ markdown: string; kept: string[]; saved: number }> {
  const urls = markdownImageSources(md).filter((src) => isExternalImageUrl(src, siteOrigins))
  if (urls.length === 0) return { markdown: md, kept: [], saved: 0 }
  const result = await keepRemoteImages(urls, save, options)
  let markdown = md
  for (const [from, to] of result.saved) markdown = replaceMarkdownImageSrc(markdown, from, to)
  return { markdown, kept: result.kept, saved: result.saved.size }
}
