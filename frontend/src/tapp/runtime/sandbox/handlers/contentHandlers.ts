import type { TappBridge, TappInstance } from '@myriad/tapp-runtime'
import type { TappListInstallRequestInput } from '../../../utils/tappListInstallRequest'
import { getDefaultLocale } from '../../../../i18n'
import { userFacingError } from '../../../../utils/userFacingError'
import * as TappApiService from '../../../services/TappApiService'
import { resolveManifestText } from '../../../utils/manifestLocale'
import {
  resolveTappListInstallRequest,

} from '../../../utils/tappListInstallRequest'
import { openHostExternalTab } from './baseHandlers'

/** 只接受宿主数据里的 http(s) 绝对地址。不接受调用方拼出来的 URL。 */
function hostArticleHref(link: unknown): string | null {
  if (typeof link !== 'string') return null
  const value = link.trim()
  if (!value || value.length > 2048) return null
  if (/[\u0000-\u001F\u007F]/.test(value) || /\s/.test(value)) return null
  let url: URL
  try {
    url = new URL(value)
  } catch {
    return null
  }
  if (url.username || url.password) return null
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return null
  if (!url.hostname) return null
  return url.href
}

function fail(error: unknown) {
  return {
    success: false,
    error: userFacingError(error),
  }
}

function getArgs(message: { payload: unknown }): unknown[] {
  return (message.payload as { args?: unknown[] }).args || []
}

/**
 * Lifecycle goes through the host runtime, never the raw API: uninstalling
 * must stop instances, drop widgets/platforms/caches and emit events so open
 * windows and background cores of that app are torn down.
 */
async function runtimeKnowing(tappId: string) {
  const { getTappRuntime } = await import('../../TappRuntime')
  const runtime = getTappRuntime()
  if (!runtime.getTapp(tappId)) await runtime.syncFromBackend(true)
  return runtime
}

async function syncRuntime() {
  const { getTappRuntime } = await import('../../TappRuntime')
  await getTappRuntime().syncFromBackend(true)
}

export function registerTappListHandlers(
  bridge: TappBridge,
  _tappInstance: TappInstance,
): void {
  bridge.registerHandler('tappList.list', async () => {
    try {
      const tapps = await TappApiService.listTapps()
      const locale = getDefaultLocale()
      return {
        success: true,
        data: tapps.map((t) => {
          const text = resolveManifestText(t, locale)
          return {
            id: t.id,
            name: text.name,
            version: t.version,
            description: text.description || '',
            icon: t.icon || '',
            iconSvg: t.iconSvg || '',
            status: t.status,
          }
        }),
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.get', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      const detail = await TappApiService.getTapp(tappId)
      const text = resolveManifestText(detail.manifest, getDefaultLocale())
      return {
        success: true,
        data: {
          id: detail.id,
          name: text.name,
          version: detail.manifest.version,
          description: text.description || '',
          icon: detail.icon || '',
          status: detail.status,
          installed_at: detail.installed_at,
          last_run_at: detail.last_run_at,
        },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.getRecent', async (message) => {
    const [limit] = getArgs(message) as [number?]
    try {
      const items = await TappApiService.getRecentTapps(limit || 10)
      return { success: true, data: items }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.install', async (message) => {
    const [request] = getArgs(message) as [TappListInstallRequestInput]
    try {
      const resolved = resolveTappListInstallRequest(request)
      if (resolved.kind === 'error') {
        return { success: false, error: resolved.error }
      }
      if (resolved.kind === 'direct') {
        const result = await TappApiService.installDirect({
          manifest: resolved.manifest as Parameters<
            typeof TappApiService.installDirect
          >[0]['manifest'],
          modules: resolved.modules,
          coreStyles: resolved.coreStyles,
          pageStyles: resolved.pageStyles,
          widgetStyles: resolved.widgetStyles,
          pageTemplate: resolved.pageTemplate,
          widgetTemplates: resolved.widgetTemplates,
          widgetCss: resolved.widgetCss,
          pageCss: resolved.pageCss,
          i18n: resolved.i18n,
          assets: resolved.assets,
          permissions: resolved.permissions,
        })
        await syncRuntime()
        return {
          success: true,
          data: { id: result.id, name: result.name, status: result.status },
        }
      }
      const result = await TappApiService.installFromStore({
        source: resolved.catalogRef,
        tappId: resolved.tappId,
        permissions: resolved.permissions,
      })
      await syncRuntime()
      return {
        success: true,
        data: { id: result.id, name: result.name, status: result.status },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.resolveStoreSource', async (message) => {
    const [tappId] = getArgs(message) as [string]
    if (!tappId || typeof tappId !== 'string') {
      return { success: false, error: 'tappId is required' }
    }
    try {
      const result = await TappApiService.resolveStoreSourceForTapp(tappId)
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.getInstallPackage', async (message) => {
    const [tappId, opts] = getArgs(message) as [
      string,
      { maxBytes?: number } | undefined,
    ]
    if (!tappId || typeof tappId !== 'string') {
      return { success: false, error: 'tappId is required' }
    }
    try {
      const result = await TappApiService.buildInstallPackageFromInstalled(
        tappId,
        { maxBytes: opts?.maxBytes },
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.uninstall', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await (await runtimeKnowing(tappId)).uninstallTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.start', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await (await runtimeKnowing(tappId)).startTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.stop', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await (await runtimeKnowing(tappId)).stopTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('tappList.export', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await TappApiService.exportTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })
}

/** 源写入只转发编辑字段。extra_config 里的令牌不进沙箱请求。 */
function sourceWrite(raw: Record<string, unknown> | undefined) {
  const req: {
    url?: string
    name?: string
    category?: string
    description?: string
    icon?: string
    site_url?: string
    update_interval?: number
    enabled?: boolean
  } = {}
  if (!raw) return req
  for (const key of ['url', 'name', 'category', 'description', 'icon', 'site_url'] as const) {
    if (typeof raw[key] === 'string') req[key] = raw[key]
  }
  if (typeof raw.update_interval === 'number' && Number.isFinite(raw.update_interval)) {
    req.update_interval = raw.update_interval
  }
  if (typeof raw.enabled === 'boolean') req.enabled = raw.enabled
  return req
}

export function registerPhantasiListHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('phantasiList.list', async (message) => {
    const [options = {}] = getArgs(message) as [Record<string, unknown>?]
    try {
      const { getItemPreviews } = await import('../../../../services/phantasiApi')
      const cursor =
        typeof options.cursor === 'string' && options.cursor.trim()
          ? options.cursor.trim()
          : undefined
      const data = await getItemPreviews(
        {
          per_page: (options.limit as number) || (options.per_page as number) || 30,
          page: cursor ? undefined : (options.page as number) || 1,
          cursor,
          filter: (options.filter as 'all' | 'unread' | 'starred') || 'all',
          source_id: options.source_id as number | undefined,
          category:
            typeof options.category === 'string' && options.category.trim()
              ? options.category.trim()
              : undefined,
          sort_order:
            options.sort_order === 'asc' || options.sort_order === 'desc'
              ? options.sort_order
              : undefined,
        },
        await bridge.hostAttributionHeaders(),
      )
      return {
        success: true,
        data: {
          items: data.items.map((item) => ({
            id: item.id,
            source_id: item.source_id,
            title: item.title,
            link: item.link,
            summary: item.summary || '',
            image: item.image || '',
            author: item.author || '',
            source_name: item.source_name || '',
            source_icon: item.source_icon || '',
            published_at: item.published_at,
            is_read: item.is_read,
            is_starred: item.is_starred,
          })),
          total: data.total,
          page: data.page,
          per_page: data.per_page,
          next_cursor: data.next_cursor ?? null,
        },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.get', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { getItem } = await import('../../../../services/phantasiApi')
      const item = await getItem(id, await bridge.hostAttributionHeaders())
      return {
        success: true,
        data: {
          id: item.id,
          source_id: item.source_id,
          title: item.title,
          link: item.link,
          summary: item.summary || '',
          content: item.content || '',
          image: item.image || '',
          author: item.author || '',
          source_name: item.source_name || '',
          source_icon: item.source_icon || '',
          published_at: item.published_at,
          is_read: item.is_read,
          is_starred: item.is_starred,
        },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.openOriginal', async (message) => {
    if (!tappInstance.grantedPermissions?.includes('phantasi:read')) {
      return { success: false, error: 'Missing permission: phantasi:read' }
    }
    const [rawId] = getArgs(message) as [unknown]
    const itemId = typeof rawId === 'number' ? rawId : Number.NaN
    if (!Number.isInteger(itemId) || itemId <= 0) {
      return { success: false, error: 'article id is required' }
    }
    try {
      const { getItem } = await import('../../../../services/phantasiApi')
      const item = await getItem(
        itemId,
        await bridge.hostAttributionHeaders(),
      )
      const href = hostArticleHref(item.link)
      if (!href) {
        return { success: false, error: 'article link cannot be opened' }
      }
      const opened = openHostExternalTab(tappInstance.id, href)
      if (!opened.ok) {
        return { success: false, error: opened.error }
      }
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.sources', async (message) => {
    const [options = {}] = getArgs(message) as [Record<string, unknown>?]
    try {
      const { getSources } = await import('../../../../services/phantasiApi')
      // 缺省保持 view=catalog（源行，没有未读覆盖）。阅读器要真实未读数时传
      // board: "feeds" | "notes" | "sites"，那条路径才会做未读聚合。
      const board = options.board
      const sourceQuery: {
        view?: 'catalog'
        category?: string
        board?: 'feeds' | 'notes' | 'sites'
      } = {}
      if (board === 'feeds' || board === 'notes' || board === 'sites') {
        sourceQuery.board = board
      }
      if (typeof options.category === 'string' && options.category.trim()) {
        sourceQuery.category = options.category.trim()
      }
      if (
        options.view === 'catalog' ||
        (sourceQuery.board == null && options.view == null)
      ) {
        sourceQuery.view = 'catalog'
      }
      const sources = await getSources(
        await bridge.hostAttributionHeaders(),
        sourceQuery,
      )
      return {
        success: true,
        data: sources.map((s) => ({
          id: s.id,
          name: s.name,
          url: s.url,
          icon: s.icon || '',
          description: s.description || '',
          category: s.category || '',
          source_type: s.source_type || '',
          feed_type: s.feed_type || '',
          site_url: s.site_url || '',
          update_interval: s.update_interval,
          enabled: s.enabled,
          item_count: s.item_count,
          unread_count: s.unread_count,
          last_error: s.last_error || '',
          last_fetched_at: s.last_fetched_at ?? null,
          last_success_at: s.last_success_at ?? null,
          error_count: s.error_count ?? 0,
        })),
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.categories', async () => {
    try {
      const { getCategories } = await import('../../../../services/phantasiApi')
      const categories = await getCategories(
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: categories }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.stats', async () => {
    try {
      const { getStats } = await import('../../../../services/phantasiApi')
      const stats = await getStats(await bridge.hostAttributionHeaders())
      return { success: true, data: stats }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.discover', async (message) => {
    const [url] = getArgs(message) as [string]
    try {
      const { discoverSource } = await import('../../../../services/phantasiApi')
      const result = await discoverSource(
        url,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.exportOpml', async () => {
    try {
      const { exportOpml } = await import('../../../../services/phantasiApi')
      const opml = await exportOpml(await bridge.hostAttributionHeaders())
      return { success: true, data: opml }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.markRead', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { markRead } = await import('../../../../services/phantasiApi')
      await markRead(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.markUnread', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { markUnread } = await import('../../../../services/phantasiApi')
      await markUnread(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.star', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { starItem } = await import('../../../../services/phantasiApi')
      await starItem(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.unstar', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { unstarItem } = await import('../../../../services/phantasiApi')
      await unstarItem(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.markAllRead', async (message) => {
    const [options] = getArgs(message) as [
      { source_id?: number; category?: string; before?: number }?,
    ]
    try {
      const { markAllRead } = await import('../../../../services/phantasiApi')
      const count = await markAllRead(
        options,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: { count } }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.getComments', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { getComments } = await import('../../../../services/phantasiApi')
      const result = await getComments(
        itemId,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.createComment', async (message) => {
    const [itemId, req] = getArgs(message) as [
      number,
      {
        selected_text: string
        comment: string
        start_offset?: number
        end_offset?: number
        color?: string
        is_public?: boolean
        parent_id?: number
      },
    ]
    try {
      const { createComment } = await import('../../../../services/phantasiApi')
      const result = await createComment(
        itemId,
        req,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.updateComment', async (message) => {
    const [commentId, req] = getArgs(message) as [
      number,
      { comment?: string; color?: string },
    ]
    try {
      const { updateComment } = await import('../../../../services/phantasiApi')
      const result = await updateComment(
        commentId,
        req,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.deleteComment', async (message) => {
    const [commentId] = getArgs(message) as [number]
    try {
      const { deleteComment } = await import('../../../../services/phantasiApi')
      await deleteComment(commentId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.getReplies', async (message) => {
    const [commentId] = getArgs(message) as [number]
    try {
      const { getCommentReplies } = await import('../../../../services/phantasiApi')
      const result = await getCommentReplies(
        commentId,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.createReply', async (message) => {
    const [itemId, parentId, content] = getArgs(message) as [
      number,
      number,
      string,
    ]
    try {
      const { createReply } = await import('../../../../services/phantasiApi')
      const result = await createReply(
        itemId,
        parentId,
        content,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.addSource', async (message) => {
    const [req] = getArgs(message) as [Record<string, unknown>]
    try {
      const { addSource } = await import('../../../../services/phantasiApi')
      const written = sourceWrite(req)
      const source = await addSource(
        { ...written, url: written.url || '' },
        await bridge.hostAttributionHeaders(),
      )
      return {
        success: true,
        data: { id: source.id, name: source.name, url: source.url },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.updateSource', async (message) => {
    const [id, req] = getArgs(message) as [number, Record<string, unknown>]
    try {
      const { updateSource } = await import('../../../../services/phantasiApi')
      const source = await updateSource(
        id,
        sourceWrite(req),
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: { id: source.id, name: source.name } }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.deleteSource', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { deleteSource } = await import('../../../../services/phantasiApi')
      await deleteSource(id, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.refreshSource', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { refreshSource } = await import('../../../../services/phantasiApi')
      const count = await refreshSource(
        id,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: { new_items: count } }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.importOpml', async (message) => {
    const [opml] = getArgs(message) as [string]
    try {
      const { importOpml } = await import('../../../../services/phantasiApi')
      const result = await importOpml(
        opml,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.updateCategory', async (message) => {
    const [id, req] = getArgs(message) as [
      number,
      { name?: string; icon?: string; color?: string; sort_order?: number },
    ]
    try {
      const { updateCategory } = await import('../../../../services/phantasiApi')
      const category = await updateCategory(
        id,
        req,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: { id: category.id, name: category.name } }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.createCategory', async (message) => {
    const [req] = getArgs(message) as [{ name: string }]
    try {
      const { createCategory } = await import('../../../../services/phantasiApi')
      await createCategory(req, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('phantasiList.deleteCategory', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { deleteCategory } = await import('../../../../services/phantasiApi')
      await deleteCategory(id, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })
}
