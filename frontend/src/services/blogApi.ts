import type { BlogSchemeSwitch, BlogSnapshot, BlogTheme } from '../types/blog'
import { apiService } from './api'
import { blogSnapshotFromValue, blogThemeFromValue } from './blogState'

export interface BlogSettingsWrite {
  theme_id: string
  site_title: string
  description: string
  params: Record<string, unknown>
}

export async function getBlogSettings(signal?: AbortSignal): Promise<BlogSnapshot> {
  const data = await apiService.get<unknown>('/blog/settings', { signal })
  return blogSnapshotFromValue(data)
}

export async function putBlogSettings(body: BlogSettingsWrite): Promise<BlogSnapshot> {
  const data = await apiService.put<unknown>('/blog/settings', body)
  return blogSnapshotFromValue(data)
}

export async function uploadBlogTheme(file: File): Promise<BlogTheme> {
  const body = new FormData()
  body.set('file', file)
  const data = await apiService.post<{ theme: BlogTheme }>('/blog/themes', body)
  return data.theme
}

export async function installBlogThemeFromUrl(url: string): Promise<BlogTheme> {
  const data = await apiService.post<{ theme: BlogTheme }>('/blog/themes', { url })
  return data.theme
}

/** 站长确认或改了主题的深浅色切换办法。 */
export async function setBlogThemeScheme(id: string, value: BlogSchemeSwitch): Promise<BlogTheme | null> {
  const data = await apiService.put<{ theme?: unknown }>(
    `/blog/themes/${encodeURIComponent(id)}/scheme`,
    { switch: value },
  )
  return blogThemeFromValue(data.theme)
}

export async function deleteBlogTheme(id: string): Promise<void> {
  await apiService.delete(`/blog/themes/${encodeURIComponent(id)}`)
}

export async function rebuildBlog(): Promise<BlogSnapshot['build']> {
  const data = await apiService.post<{ build?: unknown }>('/blog/rebuild', {})
  return blogSnapshotFromValue({ build: data.build }).build
}
