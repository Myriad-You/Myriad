import type { WorkbenchNoteOpen } from '../phantasi/logic/workbench'
import type { StudioToolSpec } from './overlay/studioTools'
import * as phantasiApi from '../../services/phantasiApi'
import { userFacingError } from '../../utils/userFacingError'
import { formatToolSpec } from './overlay/studioTools'
import { openBlogPage, studioOverlayApi } from './studioIsland'

/** 在覆盖层里打开一个工具；不在覆盖层里就整页去博客，让覆盖层展开它。 */
export function openStudioTool(spec: StudioToolSpec): void {
  const api = studioOverlayApi()
  if (api) {
    api.open(spec)
    return
  }
  window.top?.location.assign(`/blog/?studio=${formatToolSpec(spec)}`)
}

export async function openStudioNote(
  open: { kind: 'new' } | WorkbenchNoteOpen,
  onError?: (message: string) => void,
  failed = '',
): Promise<void> {
  if (open.kind === 'new') {
    openStudioTool({ tool: 'editor', arg: 'new' })
    return
  }
  if (open.kind === 'doc') {
    openStudioTool({ tool: 'editor', arg: String(open.id) })
    return
  }
  try {
    const doc = await phantasiApi.getNoteDocForItem(open.id)
    openStudioTool({ tool: 'editor', arg: String(doc.id) })
  } catch (error) {
    onError?.(userFacingError(error, failed))
  }
}

/** 文章公开地址以后端 item.link 为准。在覆盖层里就让外层博客页去那里。 */
export async function openPublishedItem(
  itemId: number,
  onError?: (message: string) => void,
  failed = '',
): Promise<void> {
  try {
    const item = await phantasiApi.getItem(itemId)
    const link = item.link?.trim()
    if (!link) return
    openBlogPage(link)
  } catch (error) {
    onError?.(userFacingError(error, failed))
  }
}
