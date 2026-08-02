/**
 * 内容列表处理器 — Tapp 管理与 Brew 全功能
 */

import type { TappInstance } from '../../../types'
import type { TappListInstallRequestInput } from '../../../utils/tappListInstallRequest'
import type { TappBridge } from '../../TappBridge'
import { getDefaultLocale } from '../../../../i18n'
import * as TappApiService from '../../../services/TappApiService'
import { resolveManifestText } from '../../../utils/manifestLocale'
import {
  resolveTappListInstallRequest,

} from '../../../utils/tappListInstallRequest'

// 统一错误返回
function fail(error: unknown) {
  return {
    success: false,
    error: error instanceof Error ? error.message : 'Failed',
  }
}

// 从 message payload 取参数
function getArgs(message: { payload: unknown }): unknown[] {
  return (message.payload as { args?: unknown[] }).args || []
}

// ============ Tapp 列表处理器 ============

export function registerTappListHandlers(
  bridge: TappBridge,
  _tappInstance: TappInstance,
): void {
  // 列出所有已安装 Tapp
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

  // 获取单个 Tapp 详情
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

  // 获取最近使用的 Tapp
  bridge.registerHandler('tappList.getRecent', async (message) => {
    const [limit] = getArgs(message) as [number?]
    try {
      const items = await TappApiService.getRecentTapps(limit || 10)
      return { success: true, data: items }
    } catch (error) {
      return fail(error)
    }
  })

  // 安装 Tapp — source: 'store' | 'direct' | http(s) catalog URL
  // Shape resolution: utils/tappListInstallRequest.ts (shared with docs gating tests).
  // Correct store install: { source: 'store', storeSource: id|url, tappId }
  // or { source: 'https://…/index.json', tappId }. Bare source:"1" alone fails.
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
          code: resolved.code,
          styles: resolved.styles,
          pageTemplate: resolved.pageTemplate,
          widgetTemplates: resolved.widgetTemplates,
          widgetCss: resolved.widgetCss,
          pageCss: resolved.pageCss,
          i18n: resolved.i18n,
          pageModules: resolved.pageModules,
          assets: resolved.assets,
          permissions: resolved.permissions,
        })
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
      return {
        success: true,
        data: { id: result.id, name: result.name, status: result.status },
      }
    } catch (error) {
      return fail(error)
    }
  })

  // Resolve portable catalog URL for an installed/store Tapp (for chat share).
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

  // Build a shareable direct-install package from an installed Tapp.
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

  // 卸载 Tapp
  bridge.registerHandler('tappList.uninstall', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await TappApiService.uninstallTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  // 启动 Tapp
  bridge.registerHandler('tappList.start', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await TappApiService.startTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  // 停止 Tapp
  bridge.registerHandler('tappList.stop', async (message) => {
    const [tappId] = getArgs(message) as [string]
    try {
      await TappApiService.stopTapp(tappId)
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  // 导出 Tapp
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

// ============ Brew 处理器 ============

export function registerBrewListHandlers(
  bridge: TappBridge,
  _tappInstance: TappInstance,
): void {
  // --- Brew 读取 ---

  // 文章列表
  bridge.registerHandler('brewList.list', async (message) => {
    const [options = {}] = getArgs(message) as [Record<string, unknown>?]
    try {
      const { getItems } = await import('../../../../services/brewApi')
      const data = await getItems(
        {
          per_page: (options.limit as number) || 30,
          page: (options.page as number) || 1,
          filter: (options.filter as 'all' | 'unread' | 'starred') || 'all',
          source_id: options.source_id as number | undefined,
        },
        await bridge.hostAttributionHeaders(),
      )
      return {
        success: true,
        data: {
          items: data.items.map((item) => ({
            id: item.id,
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
        },
      }
    } catch (error) {
      return fail(error)
    }
  })

  // 单篇文章详情
  bridge.registerHandler('brewList.get', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { getItem } = await import('../../../../services/brewApi')
      const item = await getItem(id, await bridge.hostAttributionHeaders())
      return {
        success: true,
        data: {
          id: item.id,
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

  // 订阅源列表
  bridge.registerHandler('brewList.sources', async () => {
    try {
      const { getSources } = await import('../../../../services/brewApi')
      const sources = await getSources(await bridge.hostAttributionHeaders())
      return {
        success: true,
        data: sources.map((s) => ({
          id: s.id,
          name: s.name,
          url: s.url,
          icon: s.icon || '',
          description: s.description || '',
          category: s.category || '',
          item_count: s.item_count,
          unread_count: s.unread_count,
        })),
      }
    } catch (error) {
      return fail(error)
    }
  })

  // 分类列表
  bridge.registerHandler('brewList.categories', async () => {
    try {
      const { getCategories } = await import('../../../../services/brewApi')
      const categories = await getCategories(
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: categories }
    } catch (error) {
      return fail(error)
    }
  })

  // 统计信息
  bridge.registerHandler('brewList.stats', async () => {
    try {
      const { getStats } = await import('../../../../services/brewApi')
      const stats = await getStats(await bridge.hostAttributionHeaders())
      return { success: true, data: stats }
    } catch (error) {
      return fail(error)
    }
  })

  // 发现 RSS 源
  bridge.registerHandler('brewList.discover', async (message) => {
    const [url] = getArgs(message) as [string]
    try {
      const { discoverSource } = await import('../../../../services/brewApi')
      const result = await discoverSource(
        url,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  // 导出 OPML
  bridge.registerHandler('brewList.exportOpml', async () => {
    try {
      const { exportOpml } = await import('../../../../services/brewApi')
      const opml = await exportOpml(await bridge.hostAttributionHeaders())
      return { success: true, data: opml }
    } catch (error) {
      return fail(error)
    }
  })

  // --- Brew 写入 ---

  bridge.registerHandler('brewList.markRead', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { markRead } = await import('../../../../services/brewApi')
      await markRead(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.markUnread', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { markUnread } = await import('../../../../services/brewApi')
      await markUnread(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.star', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { starItem } = await import('../../../../services/brewApi')
      await starItem(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.unstar', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { unstarItem } = await import('../../../../services/brewApi')
      await unstarItem(itemId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.markAllRead', async (message) => {
    const [options] = getArgs(message) as [
      { source_id?: number; category?: string; before?: number }?,
    ]
    try {
      const { markAllRead } = await import('../../../../services/brewApi')
      const count = await markAllRead(
        options,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: { count } }
    } catch (error) {
      return fail(error)
    }
  })

  // --- Brew 评论 ---

  bridge.registerHandler('brewList.getComments', async (message) => {
    const [itemId] = getArgs(message) as [number]
    try {
      const { getComments } = await import('../../../../services/brewApi')
      const result = await getComments(
        itemId,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.createComment', async (message) => {
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
      const { createComment } = await import('../../../../services/brewApi')
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

  bridge.registerHandler('brewList.updateComment', async (message) => {
    const [commentId, req] = getArgs(message) as [
      number,
      { comment?: string; color?: string },
    ]
    try {
      const { updateComment } = await import('../../../../services/brewApi')
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

  bridge.registerHandler('brewList.deleteComment', async (message) => {
    const [commentId] = getArgs(message) as [number]
    try {
      const { deleteComment } = await import('../../../../services/brewApi')
      await deleteComment(commentId, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.getReplies', async (message) => {
    const [commentId] = getArgs(message) as [number]
    try {
      const { getCommentReplies } = await import('../../../../services/brewApi')
      const result = await getCommentReplies(
        commentId,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.createReply', async (message) => {
    const [itemId, parentId, content] = getArgs(message) as [
      number,
      number,
      string,
    ]
    try {
      const { createReply } = await import('../../../../services/brewApi')
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

  // --- Brew 管理 ---

  bridge.registerHandler('brewList.addSource', async (message) => {
    const [req] = getArgs(message) as [{ url: string; category?: string }]
    try {
      const { addSource } = await import('../../../../services/brewApi')
      const source = await addSource(req, await bridge.hostAttributionHeaders())
      return {
        success: true,
        data: { id: source.id, name: source.name, url: source.url },
      }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.updateSource', async (message) => {
    const [id, req] = getArgs(message) as [
      number,
      { name?: string; category?: string },
    ]
    try {
      const { updateSource } = await import('../../../../services/brewApi')
      const source = await updateSource(
        id,
        req,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: { id: source.id, name: source.name } }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.deleteSource', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { deleteSource } = await import('../../../../services/brewApi')
      await deleteSource(id, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.refreshSource', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { refreshSource } = await import('../../../../services/brewApi')
      const count = await refreshSource(
        id,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: { new_items: count } }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.importOpml', async (message) => {
    const [opml] = getArgs(message) as [string]
    try {
      const { importOpml } = await import('../../../../services/brewApi')
      const result = await importOpml(
        opml,
        await bridge.hostAttributionHeaders(),
      )
      return { success: true, data: result }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.createCategory', async (message) => {
    const [req] = getArgs(message) as [{ name: string }]
    try {
      const { createCategory } = await import('../../../../services/brewApi')
      await createCategory(req, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })

  bridge.registerHandler('brewList.deleteCategory', async (message) => {
    const [id] = getArgs(message) as [number]
    try {
      const { deleteCategory } = await import('../../../../services/brewApi')
      await deleteCategory(id, await bridge.hostAttributionHeaders())
      return { success: true }
    } catch (error) {
      return fail(error)
    }
  })
}
