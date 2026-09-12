/** 可收录：站未 noindex、模块全体可见、公开安装且 visibility !== admin。 */

import type { PageSeoInput } from '../../utils/siteMetadata'
import type { TappInstance } from '../types'
import {
  formatPageTitle,

  tappIconAsOgImage,
} from '../../utils/siteMetadata'
import { resolveManifestText } from './manifestLocale'
import {
  TAPP_LIST_PATH,
  TAPP_STORE_PATH,
  tappDetailPath,
  tappRunPath,
} from './tappPaths'

export function isTappIndexable(tapp: TappInstance | null | undefined): boolean {
  if (!tapp) return false
  if (!tapp.isAdminTapp) return false
  if (tapp.visibility === 'admin') return false
  return true
}

export function buildTappRunPageSeo(opts: {
  tapp: TappInstance | null
  tappId: string
  locale: string
  moduleOpenToAll: boolean
}): PageSeoInput {
  const { tapp, tappId, locale, moduleOpenToAll } = opts
  const path = tappRunPath(tappId)
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
    // 详情规范到 run，避免重复收录。索引只留 run。
    path: tappDetailPath(opts.tappId),
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
    path: TAPP_LIST_PATH,
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
    path: TAPP_STORE_PATH,
    noindex: !opts.moduleOpenToAll,
  }
}
