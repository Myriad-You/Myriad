import type { NavigateFunction } from 'react-router-dom'
import type { WorkbenchNoteOpen } from '../phantasi/logic/workbench'
import * as phantasiApi from '../../services/phantasiApi'
import { userFacingError } from '../../utils/userFacingError'
import { openBlogPage } from './studioIsland'
import { studioNotePath } from './studioRoutes'

export async function openStudioNote(
  navigate: NavigateFunction,
  open: { kind: 'new' } | WorkbenchNoteOpen,
  onError?: (message: string) => void,
  failed = '',
): Promise<void> {
  if (open.kind === 'new') {
    navigate(studioNotePath('new'))
    return
  }
  if (open.kind === 'doc') {
    navigate(studioNotePath(open.id))
    return
  }
  try {
    const doc = await phantasiApi.getNoteDocForItem(open.id)
    navigate(studioNotePath(doc.id))
  } catch (error) {
    onError?.(userFacingError(error, failed))
  }
}

/** 文章公开地址以后端 item.link 为准。在创作台岛里就让外层博客页去那里。 */
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
