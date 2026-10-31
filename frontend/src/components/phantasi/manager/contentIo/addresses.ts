import { MAX_NOTE_OLD_PATHS, normalizeOldPath } from '../../notes/noteOldPaths'
import { noteSlugWire } from '../../notes/noteSlug'

/** 导出到 WordPress 时旧地址放进这个自定义字段，再导回来时照样认。 */
export const OLD_PATH_META_KEY = '_myriad_old_path'

/**
 * 旧站上文章的地址：完整网址或路径都行，只留路径。根路径（`?p=123` 这种）、站内地址
 * 这些用不上的直接丢掉，不让一条坏地址拖垮整篇导入。
 */
export function importOldPaths(values: readonly (string | null | undefined)[]): string[] {
  const out: string[] = []
  for (const value of values) {
    if (!value?.trim()) continue
    const path = normalizeOldPath(value)
    if (path === 'invalid' || path === 'reserved' || path === 'too-long') continue
    if (!out.includes(path)) out.push(path)
  }
  return out.slice(0, MAX_NOTE_OLD_PATHS)
}

/** 旧站的 slug（可能是 URL 编码的）。只收这里能用的样子，别的发布时自动生成。 */
export function importSlug(raw: string | null | undefined): string | null {
  let value = raw?.trim() ?? ''
  if (!value) return null
  try {
    value = decodeURIComponent(value)
  } catch {
    // 不是合法的百分号编码就按原样看。
  }
  const wire = noteSlugWire(value)
  return typeof wire === 'string' ? wire : null
}

/** 只在有值时带上 slug 和旧地址，省得导出的文件里全是空字段。 */
export function addressFields(slug: string | null, oldPaths: readonly string[]): { slug?: string; old_paths?: string[] } {
  return {
    ...(slug ? { slug } : {}),
    ...(oldPaths.length > 0 ? { old_paths: [...oldPaths] } : {}),
  }
}
