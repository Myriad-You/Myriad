/**
 * 主站模块页与私有页的 SEO 构建（客户端路由级）
 *
 * - 公开模块（library / brew / reports / tapp 列表）：模块对游客为 `all` 时可收录
 * - 私有页（login / register / config / setup / playground）：一律 noindex
 * - 首页：站级 title/description，规范 path 为 `/`
 */

import type { PageSeoInput } from './siteMetadata'
import { formatPageTitle } from './siteMetadata'

/** 公开模块列表页（与 sitemap 中的模块 path 对齐） */
export function buildModulePageSeo(opts: {
  label: string
  description?: string
  path: string
  /** 模块可见性对「未登录游客」是否为 all */
  moduleOpenToAll: boolean
}): PageSeoInput {
  return {
    title: formatPageTitle(opts.label),
    description: opts.description?.trim() || undefined,
    path: opts.path,
    noindex: !opts.moduleOpenToAll,
  }
}

/** 登录 / 设置 / Playground 等不应被收录的页面 */
export function buildPrivatePageSeo(opts: {
  label: string
  description?: string
  path: string
}): PageSeoInput {
  return {
    title: formatPageTitle(opts.label),
    description: opts.description?.trim() || undefined,
    path: opts.path,
    noindex: true,
  }
}

/** 首页：不覆盖站级 title/description，只固定 canonical */
export function buildHomePageSeo(): PageSeoInput {
  return {
    path: '/',
  }
}
