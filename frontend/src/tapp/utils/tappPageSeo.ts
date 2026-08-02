/**
 * Tapp 列表 / 运行 / 详情页的 SEO 策略（客户端 P0）
 *
 * 可收录条件（与可见性产品语义对齐）：
 * - 站级未 noindex
 * - 模块 tapp 对「全体」可见（由调用方判断）
 * - 应用为站点公开安装且 visibility !== 'admin'
 * 私有临时安装、仅管理员应用 → noindex
 */

import type { PageSeoInput } from '../../utils/siteMetadata'
import type { TappInstance } from '../types'
import {
  formatPageTitle,

  tappIconAsOgImage,
} from '../../utils/siteMetadata'
import { resolveManifestText } from './manifestLocale'

export function isTappIndexable(tapp: TappInstance | null | undefined): boolean {
  if (!tapp) return false
  // 仅站主公开安装可进索引；用户私有副本不分享给搜索引擎
  if (!tapp.isAdminTapp) return false
  if (tapp.visibility === 'admin') return false
  return true
}

export function buildTappRunPageSeo(opts: {
  tapp: TappInstance | null
  tappId: string
  locale: string
  /** 模块 tapp 是否对爬虫/游客可见（visibility level === 'all'） */
  moduleOpenToAll: boolean
}): PageSeoInput {
  const { tapp, tappId, locale, moduleOpenToAll } = opts
  const path = `/tapp/run/${encodeURIComponent(tappId)}`
  if (!tapp) {
    return {
      title: formatPageTitle(tappId),
      path,
      noindex: true,
    }
  }
  const { name, description } = resolveManifestText(tapp.manifest, locale)
  return {
    title: formatPageTitle(name || tappId),
    description: description || undefined,
    image: tappIconAsOgImage(tapp.manifest.icon),
    path,
    noindex: !moduleOpenToAll || !isTappIndexable(tapp),
  }
}

export function buildTappDetailPageSeo(opts: {
  tapp: TappInstance | null
  tappId: string
  locale: string
  moduleOpenToAll: boolean
}): PageSeoInput {
  const run = buildTappRunPageSeo(opts)
  return {
    ...run,
    // 详情页规范到 run（避免 run/detail 重复收录）；分享仍可用 detail URL 作 og:url
    path: `/tapp/detail/${encodeURIComponent(opts.tappId)}`,
    // 索引只保留 run：详情默认 noindex
    noindex: true,
  }
}

export function buildTappListPageSeo(opts: {
  listLabel: string
  listDescription?: string
  moduleOpenToAll: boolean
}): PageSeoInput {
  return {
    title: formatPageTitle(opts.listLabel),
    description: opts.listDescription,
    path: '/tapp',
    noindex: !opts.moduleOpenToAll,
  }
}

export function buildTappStorePageSeo(opts: {
  storeLabel: string
  storeDescription?: string
  moduleOpenToAll: boolean
}): PageSeoInput {
  return {
    title: formatPageTitle(opts.storeLabel),
    description: opts.storeDescription,
    path: '/tapp/store',
    noindex: !opts.moduleOpenToAll,
  }
}
