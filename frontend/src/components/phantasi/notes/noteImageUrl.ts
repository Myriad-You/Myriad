/**
 * 正文里图片的「显示地址」。Markdown 里存的是上传时拿到的地址（带站点公开域名），
 * 但浏览器此刻连的可能是别的 origin（开发机、内网、域名还没切过去）。
 * 本站自己托管的媒体（/media/assets、/media/federation、/api/…）一律改成走当前 API origin；
 * 外站图按热链名单决定要不要代理。Markdown 里的原地址不动。
 */

import { API_URL } from '../../../config'
import { proxyImageUrl } from '../../../utils/proxyImageUrl'
import { siteMediaUrl } from '../../../utils/siteMediaUrl'
import { emptyNoteWidgetText, stampNoteWidgetNotProse } from './noteWidgetHtml'

const SELF_HOSTED_PREFIXES = ['/media/assets/', '/media/federation/', '/api/']

export function displayImageUrl(src: string, apiUrl: string = API_URL): string {
  const raw = src.trim()
  if (!raw || raw.startsWith('data:') || raw.startsWith('blob:')) return raw
  if (SELF_HOSTED_PREFIXES.some((prefix) => raw.startsWith(prefix))) {
    return siteMediaUrl(raw, apiUrl)
  }
  try {
    const url = new URL(raw)
    if (SELF_HOSTED_PREFIXES.some((prefix) => url.pathname.startsWith(prefix))) {
      return siteMediaUrl(`${url.pathname}${url.search}`, apiUrl)
    }
  } catch {
    return raw
  }
  return proxyImageUrl(raw) ?? raw
}

/** 后端渲染出来的预览 HTML：只改 `src` 给浏览器看，别的不碰。 */
export function withDisplayImages(html: string, apiUrl: string = API_URL): string {
  return html.replaceAll(/<img(\s[^>]*?)src="([^"]*)"/g, (_m, before: string, src: string) => {
    const shown = displayImageUrl(decodeAttr(src), apiUrl)
    return `<img${before}src="${encodeAttr(shown)}"`
  })
}

/**
 * 预览和阅读器共用。笔记发布 HTML 已经消过毒，只改显示地址。
 * 摘要是纯文本，不当 HTML 灌进去。
 */
export function prepareNoteReaderHtml(
  html: string | null | undefined,
  emptyHtml: string,
  apiUrl: string = API_URL,
): string {
  const content = html?.trim() ? html : emptyHtml
  return emptyNoteWidgetText(stampNoteWidgetNotProse(withDisplayImages(content, apiUrl)))
}

function decodeAttr(value: string): string {
  return value.replaceAll('&amp;', '&').replaceAll('&quot;', '"')
}

function encodeAttr(value: string): string {
  return value.replaceAll('&', '&amp;').replaceAll('"', '&quot;')
}
