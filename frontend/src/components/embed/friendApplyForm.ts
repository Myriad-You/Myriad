/**
 * 友链申请表的规则。和后端同一套（backend/src/api/phantasi/applications.rs、helpers.rs 的
 * normalize_http_url），在填的时候就说清楚哪里不对，不等提交后再被退回。
 */

export interface FriendApplyFields {
  siteName: string
  siteUrl: string
  feedUrl: string
  description: string
  message: string
  applicantName: string
  applicantEmail: string
}

export type FriendApplyField = keyof FriendApplyFields

export const EMPTY_FRIEND_APPLY: FriendApplyFields = {
  siteName: '',
  siteUrl: '',
  feedUrl: '',
  description: '',
  message: '',
  applicantName: '',
  applicantEmail: '',
}

/** 后端的字数上限（按字符算）。 */
export const FRIEND_APPLY_MAX: Record<FriendApplyField, number> = {
  siteName: 120,
  siteUrl: 2048,
  feedUrl: 2048,
  description: 500,
  message: 1000,
  applicantName: 80,
  applicantEmail: 200,
}

export type FriendApplyProblem = 'required' | 'url' | 'email' | 'tooLong'

/**
 * 没写协议的补 https（`example.com/blog` 也认），只收 http / https，不许带账号密码，去掉 `#…`。
 * 认不出时返回 null。
 */
export function normalizeSiteUrl(raw: string): URL | null {
  const trimmed = raw.trim()
  if (!trimmed) return null
  let url: URL
  if (URL.canParse(trimmed)) {
    url = new URL(trimmed)
  } else if (!trimmed.includes('://') && URL.canParse(`https://${trimmed}`)) {
    url = new URL(`https://${trimmed}`)
  } else {
    return null
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return null
  if (url.username || url.password) return null
  if (!url.hostname) return null
  url.hash = ''
  return url
}

export function looksLikeEmail(raw: string): boolean {
  const at = raw.indexOf('@')
  if (at <= 0) return false
  const host = raw.slice(at + 1)
  return host.includes('.') && !host.startsWith('.') && !host.endsWith('.') && !host.includes(' ')
}

function charCount(text: string): number {
  return [...text].length
}

/** 每一栏的问题；没问题的栏不出现。 */
export function friendApplyProblems(fields: FriendApplyFields): Partial<Record<FriendApplyField, FriendApplyProblem>> {
  const problems: Partial<Record<FriendApplyField, FriendApplyProblem>> = {}
  for (const key of Object.keys(FRIEND_APPLY_MAX) as FriendApplyField[]) {
    if (charCount(fields[key].trim()) > FRIEND_APPLY_MAX[key]) problems[key] = 'tooLong'
  }
  if (!fields.siteName.trim()) problems.siteName = 'required'
  if (!fields.siteUrl.trim()) problems.siteUrl = 'required'
  else if (!problems.siteUrl && !normalizeSiteUrl(fields.siteUrl)) problems.siteUrl = 'url'
  if (fields.feedUrl.trim() && !problems.feedUrl && !normalizeSiteUrl(fields.feedUrl)) problems.feedUrl = 'url'
  if (fields.applicantEmail.trim() && !problems.applicantEmail && !looksLikeEmail(fields.applicantEmail.trim())) {
    problems.applicantEmail = 'email'
  }
  return problems
}

/** 交给接口的样子：去掉首尾空白，网址补全，空的可选栏不发。 */
export function friendApplyPayload(fields: FriendApplyFields) {
  const optional = (value: string) => value.trim() || undefined
  const feed = fields.feedUrl.trim()
  return {
    site_name: fields.siteName.trim(),
    site_url: normalizeSiteUrl(fields.siteUrl)?.href ?? fields.siteUrl.trim(),
    feed_url: feed ? (normalizeSiteUrl(feed)?.href ?? feed) : undefined,
    description: optional(fields.description),
    message: optional(fields.message),
    applicant_name: optional(fields.applicantName),
    applicant_email: optional(fields.applicantEmail),
  }
}

/** 预览卡上显示的地址：只要主机名和路径，不带协议和末尾的斜杠。 */
export function displaySiteUrl(url: URL): string {
  const path = url.pathname === '/' ? '' : url.pathname.replace(/\/$/, '')
  return `${url.hostname.replace(/^www\./, '')}${path}`
}
