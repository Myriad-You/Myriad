import type { PageSeoInput } from './siteMetadata'
import { formatPageTitle } from './siteMetadata'

export function buildModulePageSeo(opts: {
  label: string
  description?: string
  path: string
  moduleOpenToAll: boolean
}): PageSeoInput {
  return {
    title: formatPageTitle(opts.label),
    description: opts.description?.trim() || undefined,
    path: opts.path,
    noindex: !opts.moduleOpenToAll,
  }
}

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

export function buildHomePageSeo(): PageSeoInput {
  return {
    path: '/',
  }
}
