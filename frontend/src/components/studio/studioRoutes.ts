/**
 * 创作台地址。创作台只在博客页的覆盖层里：每个工具窗口是嵌入页
 * `/embed/studio/tool/{tool}`，控制器是 `/embed/studio/overlay`。订阅类面板不在这里。
 */

import type { MediaOrigin } from '../../services/mediaApi'
import type { StudioTool } from './overlay/studioTools'
import { formatToolSpec, isStudioTool, legacySectionSpec } from './overlay/studioTools'

export const STUDIO_TOOL_ROOT = '/embed/studio/tool'

/** 旧地址，整页跳去博客并展开覆盖层。 */
export const LEGACY_STUDIO_ROOT = '/studio'

function trimPath(pathname: string): string {
  if (pathname.length > 1 && pathname.endsWith('/')) {
    return pathname.replace(/\/+$/, '') || '/'
  }
  return pathname
}

/** `/embed/studio/tool/comments` 里的工具名；别的地址为 null。 */
export function parseStudioToolPath(pathname: string): StudioTool | null {
  const path = trimPath(pathname)
  if (!path.startsWith(`${STUDIO_TOOL_ROOT}/`)) return null
  const tool = path.slice(STUDIO_TOOL_ROOT.length + 1)
  return isStudioTool(tool) ? tool : null
}

/** 编辑器窗口的 `arg`：`new` 或笔记文档 id。 */
export function editorTarget(arg: string | null): number | 'new' {
  if (arg && /^[1-9]\d*$/.test(arg)) {
    const id = Number(arg)
    if (Number.isSafeInteger(id)) return id
  }
  return 'new'
}

/** 媒体窗口的 `arg` 对应的来源。沿用旧地址里的写法（`imported`）。 */
export function mediaOriginForArg(arg: string | null): 'all' | MediaOrigin {
  switch (arg) {
    case 'uploads':
    case 'upload':
      return 'upload'
    case 'generated':
      return 'generated'
    case 'kept':
      return 'kept'
    case 'channel':
      return 'channel'
    case 'imported':
    case 'import':
      return 'import'
    default:
      return 'all'
  }
}

/**
 * 旧的 `/studio/...` 换成博客页上的地址：`/blog/?studio=editor-12`。
 * 后端看到 `studio` 参数就让覆盖层展开，打开那个工具。
 */
export function legacyStudioRedirect(pathname: string, search = ''): string {
  const path = trimPath(pathname)
  const rest = path === LEGACY_STUDIO_ROOT ? '' : path.slice(LEGACY_STUDIO_ROOT.length)
  const query = new URLSearchParams(search)
  query.set('studio', formatToolSpec(legacySectionSpec(rest)))
  return `/blog/?${query.toString()}`
}
