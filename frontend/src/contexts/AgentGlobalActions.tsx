import type { FrontendAction, PageElementTarget } from '../services/agent'
import { useCallback, useEffect, useRef } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { currentCopy } from '../i18n/localeCopy'
import {
  registerActionHandler,
  unregisterActionHandler,
} from '../services/agent'
import { brewSubject } from '../utils/brewSubject'
import { useMusicPlayerControl } from './MusicPlayerContext'

function findElement(target: PageElementTarget): HTMLElement | null {
  if (target.testId) {
    const el = document.querySelector(
      `[data-testid="${CSS.escape(target.testId)}"]`,
    )
    if (el) return el as HTMLElement
  }

  if (target.selector) {
    try {
      const els = document.querySelectorAll(target.selector)
      if (els.length > 0) {
        const index = typeof target.index === 'number' ? target.index : 0
        const el = els[index]
        if (el) return el as HTMLElement
      }
    } catch {
      return null
    }
  }

  if (target.ariaLabel) {
    const el = document.querySelector(
      `[aria-label="${CSS.escape(target.ariaLabel)}"]`,
    )
    if (el) return el as HTMLElement
  }

  if (target.role) {
    const elements = document.querySelectorAll(
      `[role="${CSS.escape(target.role)}"]`,
    )
    if (target.text) {
      for (const el of elements) {
        if (el.textContent?.includes(target.text)) {
          return el as HTMLElement
        }
      }
    } else if (typeof target.index === 'number') {
      return elements[target.index] as HTMLElement
    } else if (elements.length > 0) {
      return elements[0] as HTMLElement
    }
  }

  if (target.text && !target.role) {
    const interactiveElements = document.querySelectorAll(
      'button, a, input, textarea, [contenteditable="true"], [role="button"], [role="link"], [role="tab"], [role="menuitem"]',
    )
    for (const el of interactiveElements) {
      if (el.textContent?.includes(target.text)) {
        return el as HTMLElement
      }
    }
  }

  return null
}

async function waitForElement(
  target: PageElementTarget,
  timeout = 5000,
  signal?: AbortSignal,
): Promise<HTMLElement | null> {
  const startTime = Date.now()

  while (Date.now() - startTime < timeout) {
    if (signal?.aborted) return null
    const el = findElement(target)
    if (el) return el

    await new Promise((resolve) => setTimeout(resolve, 100))
  }

  return null
}

async function executeInteraction(
  element: HTMLElement,
  action: string,
  value?: FrontendAction['value'],
  options?: FrontendAction['scrollOptions'],
): Promise<boolean> {
  try {
    switch (action) {
      case 'click':
        element.click()
        break

      case 'hover':
        element.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }))
        element.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }))
        break

      case 'focus':
        element.focus()
        break

      case 'blur':
        element.blur()
        break

      case 'scroll':
        if (options) {
          const scrollBehavior = options.smooth ? 'smooth' : 'auto'
          if (options.direction === 'top') {
            element.scrollTo({ top: 0, behavior: scrollBehavior })
          } else if (options.direction === 'bottom') {
            element.scrollTo({
              top: element.scrollHeight,
              behavior: scrollBehavior,
            })
          } else if (options.direction === 'left') {
            element.scrollBy({
              left: -(options.offset ?? 200),
              behavior: scrollBehavior,
            })
          } else if (options.direction === 'right') {
            element.scrollBy({
              left: options.offset ?? 200,
              behavior: scrollBehavior,
            })
          } else if (options.offset !== undefined) {
            element.scrollBy({ top: options.offset, behavior: scrollBehavior })
          }
        } else {
          element.scrollIntoView({ behavior: 'smooth', block: 'center' })
        }
        break

      case 'scrollIntoView':
        element.scrollIntoView({ behavior: 'smooth', block: 'center' })
        break

      case 'select':
        if (element instanceof HTMLInputElement) {
          if (element.type === 'checkbox' || element.type === 'radio') {
            element.checked =
              typeof value === 'boolean' ? value : value !== 'false'
            element.dispatchEvent(new Event('change', { bubbles: true }))
          }
        } else if (element instanceof HTMLSelectElement) {
          if (value !== undefined) element.value = String(value)
          element.dispatchEvent(new Event('change', { bubbles: true }))
        }
        break

      case 'toggle':
        if (element instanceof HTMLInputElement) {
          if (element.type === 'checkbox') {
            element.checked = !element.checked
            element.dispatchEvent(new Event('change', { bubbles: true }))
          }
        } else {
          element.click()
        }
        break

      case 'expand':
        if (element.getAttribute('aria-expanded') === 'false') {
          element.click()
        }
        break

      case 'collapse':
        if (element.getAttribute('aria-expanded') === 'true') {
          element.click()
        }
        break

      case 'type':
      case 'input': {
        const text = value === undefined ? '' : String(value)
        if (
          element instanceof HTMLInputElement ||
          element instanceof HTMLTextAreaElement
        ) {
          element.focus()
          element.value = text
          element.dispatchEvent(new Event('input', { bubbles: true }))
          element.dispatchEvent(new Event('change', { bubbles: true }))
        } else if (element.isContentEditable) {
          element.focus()
          element.textContent = text
          element.dispatchEvent(new Event('input', { bubbles: true }))
        } else {
          return false
        }
        break
      }

      default:
        console.warn(
          `[AgentGlobalActions] Unknown interaction action: ${action}`,
        )
        return false
    }

    return true
  } catch (error) {
    console.error(`[AgentGlobalActions] Interaction failed:`, error)
    return false
  }
}

/** Must live inside BrowserRouter (useNavigate). */
export function AgentGlobalActions() {
  const navigate = useNavigate()
  const location = useLocation()
  const musicPlayer = useMusicPlayerControl()
  const isNavigatingRef = useRef(false)
  const playAudioRef = useRef<{ audio: HTMLAudioElement; url: string; unbindAbort?: () => void } | null>(
    null,
  )

  const releasePlayAudio = useCallback(() => {
    const current = playAudioRef.current
    if (!current) return
    playAudioRef.current = null
    current.unbindAbort?.()
    current.audio.pause()
    current.audio.removeAttribute('src')
    current.audio.load()
    URL.revokeObjectURL(current.url)
  }, [])

  const handleNavigate = useCallback(
    async (action: FrontendAction): Promise<boolean> => {
      if (action.type !== 'navigate') return false

      if (isNavigatingRef.current) {
        console.warn('[AgentGlobalActions] Navigation already in progress')
        return false
      }

      try {
        isNavigatingRef.current = true

        let targetPath = action.fullPath || action.path

        if (!targetPath) {
          console.warn('[AgentGlobalActions] Navigate action without path')
          return false
        }

        if (
          action.query &&
          Object.keys(action.query).length > 0 &&
          !action.fullPath
        ) {
          const searchParams = new URLSearchParams()
          for (const [key, value] of Object.entries(action.query)) {
            if (value !== undefined && value !== null) {
              searchParams.set(key, String(value))
            }
          }
          const queryString = searchParams.toString()
          if (queryString) {
            targetPath = `${targetPath}?${queryString}`
          }
        }

        if (
          location.pathname === targetPath ||
          `${location.pathname}${location.search}` === targetPath
        ) {
          console.log(
            '[AgentGlobalActions] Already at target path:',
            targetPath,
          )
          return true
        }

        console.log(
          '[AgentGlobalActions] Navigating to:',
          targetPath,
          action.replace ? '(replace)' : '',
        )

        if (action.replace) {
          navigate(targetPath, { replace: true, state: action.params })
        } else {
          navigate(targetPath, { state: action.params })
        }

        return true
      } finally {
        // Unlock after navigation can finish.
        setTimeout(() => {
          isNavigatingRef.current = false
        }, 100)
      }
    },
    [navigate, location],
  )

  const handlePageInteract = useCallback(
    async (action: FrontendAction, signal?: AbortSignal): Promise<boolean> => {
      if (action.type !== 'page_interact') return false

      const target = action.target as PageElementTarget | undefined
      if (!target) {
        console.warn('[AgentGlobalActions] page_interact without target')
        return false
      }

      const waitTimeout = action.waitFor?.timeout || 5000
      const element = await waitForElement(target, waitTimeout, signal)
      if (signal?.aborted) return false

      if (!element) {
        console.warn('[AgentGlobalActions] Element not found:', target)
        return false
      }

      if (action.waitFor?.visible) {
        const isVisible = element.offsetWidth > 0 && element.offsetHeight > 0
        if (!isVisible) {
          console.warn('[AgentGlobalActions] Element not visible:', target)
          return false
        }
      }

      const interactionAction = action.action || 'click'
      return executeInteraction(
        element,
        interactionAction,
        action.value,
        action.scrollOptions,
      )
    },
    [],
  )

  const handleBrewOpenArticle = useCallback(
    async (action: FrontendAction): Promise<boolean> => {
      if (action.type !== 'brew_open_article') return false

      const params = action.params as
        | {
            articleId?: string
            articleLink?: string
            openLatest?: boolean
            openReader?: boolean
          }
        | undefined

      console.log('[AgentGlobalActions] Opening brew article:', params)

      const isOnBrewPage =
        location.pathname === '/brew' || location.pathname.startsWith('/brew/')

      if (!isOnBrewPage) {
        console.log(
          '[AgentGlobalActions] Not on Brew page, navigating first...',
        )

        // Park in sessionStorage so Brew can run it after mount (avoids a pre-mount race).
        const pendingAction = {
          subjectKey: brewSubject.capture().key,
          subjectGeneration: brewSubject.capture().generation,
          articleId: params?.articleId,
          articleLink: params?.articleLink,
          openLatest: params?.openLatest ?? true,
          timestamp: Date.now(),
        }
        sessionStorage.setItem(
          'brew_pending_open_article',
          JSON.stringify(pendingAction),
        )
        console.log(
          '[AgentGlobalActions] Stored pending action to sessionStorage:',
          pendingAction,
        )

        navigate('/brew')
      } else {
        const event = new CustomEvent('agent:open-brew-article', {
          detail: {
            articleId: params?.articleId,
            articleLink: params?.articleLink,
            openLatest: params?.openLatest ?? true,
          },
        })
        window.dispatchEvent(event)
      }

      return true
    },
    [location.pathname, navigate],
  )

  const handleMusicControl = useCallback(
    async (frontendAction: FrontendAction): Promise<boolean> => {
      console.log('[AgentGlobalActions] handleMusicControl:', frontendAction)

      const action = frontendAction.action as string
      const value = frontendAction.value as number | undefined

      switch (action) {
        case 'play':
          if (!musicPlayer.isPlaying) {
            window.dispatchEvent(new CustomEvent('toggle-play-pause'))
          }
          break
        case 'pause':
          if (musicPlayer.isPlaying) {
            window.dispatchEvent(new CustomEvent('toggle-play-pause'))
          }
          break
        case 'toggle':
        case 'toggle-play-pause':
          window.dispatchEvent(new CustomEvent('toggle-play-pause'))
          break

        case 'next':
          window.dispatchEvent(new CustomEvent('music-player-next'))
          break

        case 'previous':
        case 'prev':
          window.dispatchEvent(new CustomEvent('music-player-prev'))
          break

        case 'volume':
          if (value !== undefined) {
            // Volume is 0–1 (backend already converted).
            window.dispatchEvent(
              new CustomEvent('music-player-volume', {
                detail: { volume: value },
              }),
            )
          }
          break

        case 'mute':
          window.dispatchEvent(
            new CustomEvent('music-player-mute', {
              detail: { muted: frontendAction.value !== false },
            }),
          )
          break

        case 'seek':
          if (value !== undefined) {
            window.dispatchEvent(
              new CustomEvent('music-player-seek', {
                detail: { position: value },
              }),
            )
          }
          break

        default:
          console.warn('[AgentGlobalActions] Unknown music action:', action)
          return false
      }

      return true
    },
    [musicPlayer.isPlaying],
  )

  const handleMusicLoadPlaylist = useCallback(
    async (frontendAction: FrontendAction): Promise<boolean> => {
      console.log(
        '[AgentGlobalActions] handleMusicLoadPlaylist:',
        frontendAction,
      )

      // playlistId/source/autoPlay may be top-level or under data.
      const playlistId =
        frontendAction.playlistId ||
        (frontendAction.data?.playlistId as string) ||
        ''
      const source =
        frontendAction.source ||
        (frontendAction.data?.source as string) ||
        'netease'
      const autoPlay =
        frontendAction.autoPlay ??
        (frontendAction.data?.autoPlay as boolean) ??
        true

      if (!playlistId) {
        console.error(
          '[AgentGlobalActions] Missing playlistId for music_load_playlist',
        )
        return false
      }

      console.log(
        `[AgentGlobalActions] Loading playlist: ${playlistId} from ${source}, autoPlay: ${autoPlay}`,
      )

      window.dispatchEvent(
        new CustomEvent('music-player-load-playlist', {
          detail: {
            playlistId,
            source,
            autoPlay,
            timestamp: Date.now(),
          },
        }),
      )

      return true
    },
    [],
  )

  const handleMusicGetStatus = useCallback(async (): Promise<unknown> => {
    return {
      isEnabled: musicPlayer.isEnabled,
      isPlaying: musicPlayer.isPlaying,
      currentSong: musicPlayer.currentSong,
      currentSongIndex: musicPlayer.currentSongIndex,
      playlistLength: musicPlayer.playlistLength,
      currentLyricIndex: musicPlayer.currentLyricIndex,
    }
  }, [
    musicPlayer.isEnabled,
    musicPlayer.isPlaying,
    musicPlayer.currentSong,
    musicPlayer.currentSongIndex,
    musicPlayer.playlistLength,
    musicPlayer.currentLyricIndex,
  ])

  const handleReadingList = useCallback(
    async (frontendAction: FrontendAction): Promise<boolean> => {
      console.log('[AgentGlobalActions] handleReadingList:', frontendAction)

      const payload = frontendAction.payload as
        | {
            items?: Array<{
              id: number
              title: string
              author?: string
              sourceName?: string
              publishedAt?: string
              summary?: string
              relevanceReason?: string
              link?: string
              content?: string
              fromWebSearch?: boolean
            }>
            name?: string
          }
        | undefined

      if (!payload?.items || payload.items.length === 0) {
        console.warn('[AgentGlobalActions] Reading list is empty')
        return false
      }

      console.log(
        `[AgentGlobalActions] Setting reading list: ${payload.name} with ${payload.items.length} items`,
      )

      const hasWebSearchItems = payload.items.some((item) => item.fromWebSearch)
      if (hasWebSearchItems) {
        console.log(
          '[AgentGlobalActions] Reading list contains web search results',
        )
      }

      const readingListData = {
        id: `reading_list_${Date.now()}`,
        name: payload.name || currentCopy().brew.smartReadingList,
        criteria: frontendAction.criteria || '',
        items: payload.items,
        createdAt: new Date().toISOString(),
      }

      const firstItem = payload.items[0]
      const firstItemData = firstItem?.fromWebSearch
        ? {
            ...firstItem,
            isWebSearchArticle: true,
          }
        : null

      const isOnBrewPage =
        location.pathname === '/brew' || location.pathname.startsWith('/brew/')

      if (!isOnBrewPage) {
        // Park in sessionStorage so Brew can run it after mount (avoids a pre-mount race).
        const pendingAction = {
          subjectKey: brewSubject.capture().key,
          subjectGeneration: brewSubject.capture().generation,
          readingList: readingListData,
          articleId:
            payload.items[0]?.id != null
              ? String(payload.items[0].id)
              : undefined,
          webSearchArticle: firstItemData, // Pass full payload for web-search items.
          timestamp: Date.now(),
        }
        sessionStorage.setItem(
          'brew_pending_reading_list',
          JSON.stringify(pendingAction),
        )
        console.log(
          '[AgentGlobalActions] Stored pending reading list to sessionStorage:',
          pendingAction,
        )

        navigate('/brew')
      } else {
        window.dispatchEvent(
          new CustomEvent('agent:set-reading-list', {
            detail: readingListData,
          }),
        )

        if (firstItem) {
          // Wait until the reading list is applied.
          setTimeout(() => {
            window.dispatchEvent(
              new CustomEvent('agent:open-brew-article', {
                detail: {
                  articleId: firstItem.id.toString(),
                  openLatest: false,
                  // Pass full payload for web-search items.
                  webSearchArticle: firstItemData,
                },
              }),
            )
          }, 100)
        }
      }

      return true
    },
    [location.pathname, navigate],
  )

  const handleShowNotification = useCallback(
    async (action: FrontendAction, signal?: AbortSignal): Promise<boolean> => {
      if (action.type !== 'show_notification') return false
      const params = action.params as
        | { title?: string; message?: string; content?: string }
        | undefined
      const title = typeof params?.title === 'string' ? params.title : undefined
      const message =
        (typeof params?.message === 'string' && params.message) ||
        (typeof params?.content === 'string' && params.content) ||
        (typeof action.value === 'string' && action.value) ||
        title
      if (!message) return false
      const { showToast } = await import('../utils/toastManager')
      if (signal?.aborted) return false
      showToast({
        title: title && title !== message ? title : undefined,
        message,
        type: 'success',
      })
      return true
    },
    [],
  )

  const handleCopyClipboard = useCallback(
    async (action: FrontendAction): Promise<boolean> => {
      if (action.type !== 'copy_clipboard') return false
      const text =
        (typeof action.value === 'string' && action.value) ||
        (typeof action.params?.content === 'string'
          ? String(action.params.content)
          : '')
      if (!text) return false
      try {
        await navigator.clipboard.writeText(text)
      } catch (error) {
        console.warn('[AgentGlobalActions] clipboard write failed:', error)
        return false
      }
      return true
    },
    [],
  )

  const handlePlayAudio = useCallback(
    async (action: FrontendAction, signal?: AbortSignal): Promise<boolean> => {
      if (action.type !== 'play_audio') return false
      const params = action.params as
        | { audioBase64?: string; codec?: string }
        | undefined
      const audioBase64 =
        (typeof params?.audioBase64 === 'string' && params.audioBase64) ||
        (typeof action.value === 'string' && action.value) ||
        ''
      if (!audioBase64) return false
      const codec = typeof params?.codec === 'string' ? params.codec : 'mp3'
      const mime =
        codec === 'wav' || codec === 'pcm' ? 'audio/wav' : 'audio/mpeg'
      const { base64ToAudioUrl } = await import('../services/speechApi')
      if (signal?.aborted) return false
      releasePlayAudio()
      const url = base64ToAudioUrl(audioBase64, mime)
      const audio = new Audio(url)
      playAudioRef.current = { audio, url }
      const release = () => {
        signal?.removeEventListener('abort', release)
        if (playAudioRef.current?.url !== url) return
        releasePlayAudio()
      }
      audio.addEventListener('ended', release, { once: true })
      audio.addEventListener('error', release, { once: true })
      signal?.addEventListener('abort', release, { once: true })
      playAudioRef.current.unbindAbort = () => signal?.removeEventListener('abort', release)
      try {
        await audio.play()
      } catch (error) {
        release()
        console.warn('[AgentGlobalActions] play_audio failed:', error)
        return false
      }
      return true
    },
    [releasePlayAudio],
  )

  const handleShowData = useCallback(
    async (action: FrontendAction): Promise<unknown> => {
      if (action.type !== 'show_data') return false
      const params = action.params as
        | {
            count?: number
            title?: string
            message?: string
            preview?: unknown
          }
        | undefined
      const count = typeof params?.count === 'number' ? params.count : undefined
      if (
        count === undefined &&
        params?.preview === undefined &&
        typeof params?.message !== 'string' &&
        typeof params?.title !== 'string'
      ) {
        return false
      }
      return {
        title: typeof params?.title === 'string' ? params.title : undefined,
        count,
        message: typeof params?.message === 'string' ? params.message : undefined,
        preview: params?.preview,
      }
    },
    [],
  )

  const handleDownloadFile = useCallback(
    async (action: FrontendAction): Promise<boolean> => {
      if (action.type !== 'download_file') return false
      const params = action.params as
        | { content?: string; filename?: string; format?: string }
        | undefined
      const content = typeof params?.content === 'string' ? params.content : ''
      if (!content) return false
      const filename =
        (typeof params?.filename === 'string' && params.filename) || 'download'
      const format = typeof params?.format === 'string' ? params.format : 'json'
      const mime =
        format === 'csv'
          ? 'text/csv'
          : format === 'markdown' || format === 'md'
            ? 'text/markdown'
            : 'application/json'
      const blob = new Blob([content], { type: mime })
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = url
      link.download = filename
      link.click()
      URL.revokeObjectURL(url)
      return true
    },
    [],
  )

  const handleShowReport = useCallback(
    async (action: FrontendAction): Promise<unknown> => {
      if (action.type !== 'show_report') return false
      const params = action.params as
        | {
            title?: string
            reportId?: string
            format?: string
            content?: string
          }
        | undefined
      const title =
        (typeof params?.title === 'string' && params.title) || 'Report'
      const content =
        typeof params?.content === 'string' ? params.content : undefined
      if (!content && typeof params?.reportId !== 'string') {
        return false
      }
      return {
        title,
        reportId:
          typeof params?.reportId === 'string' ? params.reportId : undefined,
        format: typeof params?.format === 'string' ? params.format : undefined,
        content,
      }
    },
    [],
  )

  useEffect(() => {
    console.log('[AgentGlobalActions] Registering global action handlers')

    registerActionHandler('navigate', handleNavigate)
    registerActionHandler('page_interact', handlePageInteract)
    registerActionHandler('brew_open_article', handleBrewOpenArticle)
    registerActionHandler('music_control', handleMusicControl)
    registerActionHandler('music_get_status', handleMusicGetStatus)
    registerActionHandler('music_load_playlist', handleMusicLoadPlaylist)
    registerActionHandler('reading_list', handleReadingList)
    registerActionHandler('show_notification', handleShowNotification)
    registerActionHandler('copy_clipboard', handleCopyClipboard)
    registerActionHandler('play_audio', handlePlayAudio)
    registerActionHandler('show_data', handleShowData)
    registerActionHandler('download_file', handleDownloadFile)
    registerActionHandler('show_report', handleShowReport)

    return () => {
      console.log('[AgentGlobalActions] Unregistering global action handlers')
      releasePlayAudio()
      unregisterActionHandler('navigate')
      unregisterActionHandler('page_interact')
      unregisterActionHandler('brew_open_article')
      unregisterActionHandler('music_control')
      unregisterActionHandler('music_get_status')
      unregisterActionHandler('music_load_playlist')
      unregisterActionHandler('reading_list')
      unregisterActionHandler('show_notification')
      unregisterActionHandler('copy_clipboard')
      unregisterActionHandler('play_audio')
      unregisterActionHandler('show_data')
      unregisterActionHandler('download_file')
      unregisterActionHandler('show_report')
    }
  }, [
    handleNavigate,
    handlePageInteract,
    handleBrewOpenArticle,
    handleMusicControl,
    handleMusicGetStatus,
    handleMusicLoadPlaylist,
    handleReadingList,
    handleShowNotification,
    handleCopyClipboard,
    handlePlayAudio,
    handleShowData,
    handleDownloadFile,
    handleShowReport,
    releasePlayAudio,
  ])

  return null
}

export default AgentGlobalActions
