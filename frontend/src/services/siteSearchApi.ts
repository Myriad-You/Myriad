import { apiService } from './api'

/** A published note that matched, as the blog shows it. */
export interface SiteSearchPost {
  title: string
  href: string
  excerpt: string
  published_at: string
  category: string | null
}

/** A library entry whose title matched. Entries have no page of their own. */
export interface SiteSearchLibraryItem {
  title: string
  item_type: string
  platform: string
  cover: string | null
}

export interface SiteSearchResults {
  posts: SiteSearchPost[]
  library: SiteSearchLibraryItem[]
}

/** Public site content for `query`, limited to the modules this visitor can see. */
export async function searchSite(
  query: string,
  signal?: AbortSignal,
): Promise<SiteSearchResults> {
  const body = await apiService.get<Partial<SiteSearchResults>>('/search', {
    params: { q: query },
    requireAuth: false,
    signal,
  })
  return { posts: body.posts ?? [], library: body.library ?? [] }
}
