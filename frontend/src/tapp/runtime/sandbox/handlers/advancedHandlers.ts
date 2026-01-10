/**
 * 高级功能处理器
 *
 * Media, Component, Shortcut, Event, Background, Animation, DynamicContent
 */

import type { DynamicContentItem } from '../../../../services/DynamicContentProvider'
import type { TappInstance } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import type { AnimationConfigRef } from '../types'
import { getDynamicContentProvider } from '../../../../services/DynamicContentProvider'
import * as TappApiService from '../../../services/TappApiService'
import { getTappRuntime } from '../../TappRuntime'

/**
 * 注册 Media 处理器
 */
export function registerMediaHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  // 高频操作（seek、volume）不需要后端日志记录，直接本地处理
  const HIGH_FREQUENCY_ACTIONS = new Set(['seek', 'volume', 'mute', 'unmute'])

  bridge.registerHandler('media.control', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { action, value } = (params || {}) as { action?: string, value?: unknown }
    try {
      // 高频操作跳过后端 API，直接触发本地事件
      const isHighFrequency = HIGH_FREQUENCY_ACTIONS.has(action || '')

      if (!isHighFrequency) {
        // 非高频操作调用后端 API 记录日志
        await TappApiService.mediaControl({ tappId: tappInstance.id, action: (action || 'play') as 'play' | 'pause' | 'next' | 'prev' | 'seek' | 'volume' | 'mute' | 'unmute' | 'mode', value })
      }

      // 触发实际的播放器控制事件
      switch (action) {
        case 'play':
          // 只有在不是播放状态时才触发播放
          {
            const globalState = (window as { __musicPlayerState?: Record<string, unknown> }).__musicPlayerState
            if (!globalState?.isPlaying) {
              window.dispatchEvent(new CustomEvent('toggle-play-pause'))
            }
          }
          break
        case 'pause':
          // 只有在播放状态时才触发暂停
          {
            const globalState = (window as { __musicPlayerState?: Record<string, unknown> }).__musicPlayerState
            if (globalState?.isPlaying) {
              window.dispatchEvent(new CustomEvent('toggle-play-pause'))
            }
          }
          break
        case 'next':
          window.dispatchEvent(new CustomEvent('music-player-next'))
          break
        case 'prev':
          window.dispatchEvent(new CustomEvent('music-player-prev'))
          break
        case 'seek':
          window.dispatchEvent(new CustomEvent('music-player-seek', { detail: { position: value } }))
          break
        case 'volume':
          window.dispatchEvent(new CustomEvent('music-player-volume', { detail: { volume: value } }))
          break
        case 'mute':
          window.dispatchEvent(new CustomEvent('music-player-mute', { detail: { muted: true } }))
          break
        case 'unmute':
          window.dispatchEvent(new CustomEvent('music-player-mute', { detail: { muted: false } }))
          break
        case 'mode':
          window.dispatchEvent(new CustomEvent('music-player-mode', { detail: { mode: value } }))
          break
      }

      return { success: true, data: { action, value } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('media.getStatus', async () => {
    const globalState = (window as { __musicPlayerState?: Record<string, unknown> }).__musicPlayerState
    if (globalState) {
      const currentSong = globalState.currentSong as Record<string, unknown> | null
      const currentTime = (globalState.currentTime as number) || 0
      const audioDuration = (globalState.audioDuration as number) || (currentSong?.duration as number) || 0
      const volume = (globalState.volume as number) || 0.7
      const playMode = (globalState.playMode as string) || 'loop'
      const lyrics = (globalState.lyrics as Array<{ time: number, text: string }>) || []
      const currentLyricIndex = (globalState.currentLyricIndex as number) ?? -1
      const musicColor = (globalState.musicColor as string) || '#fc3c44'
      const musicColors = globalState.musicColors as { primary: string, secondary: string, accent: string, light: string, dark: string } | null

      // 将内部 playMode 映射为 API 模式
      const modeMap: Record<string, string> = {
        loop: 'loop',
        single: 'single',
        shuffle: 'shuffle',
      }
      const apiMode = modeMap[playMode] || 'sequence'

      return {
        success: true,
        data: {
          isPlaying: globalState.isPlaying || false,
          isPaused: !globalState.isPlaying && currentSong !== null,
          currentTrack: currentSong
            ? {
                id: currentSong.id || '',
                title: currentSong.name || currentSong.title || '',
                artist: currentSong.artist || '',
                album: currentSong.album || '',
                cover: currentSong.cover || '',
                duration: currentSong.duration || 0,
                source: currentSong.source || 'unknown',
              }
            : null,
          progress: {
            current: currentTime,
            duration: audioDuration,
            percentage: audioDuration > 0 ? (currentTime / audioDuration) * 100 : 0,
          },
          playlist: globalState.playlist ? { id: 'current', name: 'Current Playlist', tracks: (globalState.playlistLength as number) || (globalState.playlist as unknown[]).length || 0 } : null,
          mode: apiMode,
          volume: Math.round(volume * 100), // 转换为 0-100
          muted: volume === 0,
          // 歌词信息
          lyrics,
          currentLyricIndex,
          // 动态主题色 - 完整颜色
          primaryColor: musicColor,
          secondaryColor: musicColors?.secondary || musicColor,
          accentColor: musicColors?.accent || musicColor,
          lightColor: musicColors?.light || '#ffffff',
          darkColor: musicColors?.dark || '#000000',
        },
      }
    }
    return { success: true, data: { isPlaying: false, isPaused: false, currentTrack: null, progress: { current: 0, duration: 0, percentage: 0 }, playlist: null, mode: 'sequence', volume: 70, muted: false, lyrics: [], currentLyricIndex: -1, primaryColor: '#fc3c44', secondaryColor: '#fc3c44', accentColor: '#fc3c44', lightColor: '#ffffff', darkColor: '#000000' } }
  })

  bridge.registerHandler('media.getPlaylist', async () => {
    const globalState = (window as { __musicPlayerState?: Record<string, unknown> }).__musicPlayerState
    if (globalState?.playlist) {
      const playlist = globalState.playlist as Array<Record<string, unknown>>
      const tracks = playlist.map((song, index) => ({
        id: song.id || String(index),
        index,
        title: song.name || song.title || 'Unknown',
        artist: song.artist || 'Unknown',
        album: song.album || '',
        cover: song.cover || '',
        duration: song.duration || 0,
        source: song.source || 'unknown',
        isCurrent: index === globalState.currentSongIndex,
      }))
      return { success: true, data: { tracks, currentIndex: globalState.currentSongIndex || 0, total: tracks.length } }
    }
    return { success: true, data: { tracks: [], currentIndex: 0, total: 0 } }
  })

  // 频谱数据缓存 - 避免高频调用时重复计算
  let spectrumCache: { data: unknown, timestamp: number } | null = null
  const SPECTRUM_CACHE_TTL = 16 // ~60fps, 缓存16ms

  bridge.registerHandler('media.getSpectrum', async () => {
    const now = Date.now()

    // 检查缓存是否有效
    if (spectrumCache && (now - spectrumCache.timestamp) < SPECTRUM_CACHE_TTL) {
      return { success: true, data: spectrumCache.data }
    }

    // 从Myriad的audioManager获取频谱数据
    const audioManager = (window as { audioManager?: { getSpectrumData: () => number[] } }).audioManager
    if (audioManager && typeof audioManager.getSpectrumData === 'function') {
      const spectrum = audioManager.getSpectrumData()
      // 计算能量值（低频平均）
      const energy = spectrum.length >= 4
        ? (spectrum[0] + spectrum[1] + spectrum[2] + spectrum[3]) * 0.25 // 乘法比除法快
        : 0
      const result = {
        spectrum, // 完整频谱数据 (0-1 范围)
        energy, // 能量值 (0-1 范围)
        bass: spectrum[0] || 0, // 低频
        mid: spectrum[2] || 0, // 中频
        high: spectrum[5] || 0, // 高频
      }
      // 更新缓存
      spectrumCache = { data: result, timestamp: now }
      return { success: true, data: result }
    }
    return { success: true, data: { spectrum: [], energy: 0, bass: 0, mid: 0, high: 0 } }
  })

  bridge.registerHandler('media.playTrack', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { trackId, trackIndex } = (params || {}) as { trackId?: string, trackIndex?: number }
    const globalState = (window as { __musicPlayerState?: Record<string, unknown> }).__musicPlayerState
    if (globalState?.playlist) {
      const playlist = globalState.playlist as Array<Record<string, unknown>>
      let targetSong: Record<string, unknown> | null = null
      let targetIndex = -1
      if (typeof trackIndex === 'number' && trackIndex >= 0 && trackIndex < playlist.length) {
        targetSong = playlist[trackIndex]
        targetIndex = trackIndex
      }
      else if (trackId) {
        targetIndex = playlist.findIndex(s => s.id === trackId)
        if (targetIndex >= 0)
          targetSong = playlist[targetIndex]
      }
      if (targetSong) {
        window.dispatchEvent(new CustomEvent('play-song-at-index', { detail: { index: targetIndex, song: targetSong } }))
        return { success: true, data: { index: targetIndex, track: { id: targetSong.id, title: targetSong.name || targetSong.title, artist: targetSong.artist, duration: targetSong.duration, cover: targetSong.cover } } }
      }
    }
    return { success: false, error: 'Track not found' }
  })

  // 在当前播放列表中跳转到指定索引（不触发临时播放）
  bridge.registerHandler('media.jumpToIndex', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { index } = (params || {}) as { index?: number }
    const globalState = (window as { __musicPlayerState?: Record<string, unknown> }).__musicPlayerState
    if (globalState?.playlist && typeof index === 'number') {
      const playlist = globalState.playlist as Array<Record<string, unknown>>
      if (index >= 0 && index < playlist.length) {
        const targetSong = playlist[index]
        // 使用新事件 jump-to-index，不触发临时播放
        window.dispatchEvent(new CustomEvent('jump-to-index', { detail: { index, song: targetSong } }))
        return { success: true, data: { index, track: { id: targetSong.id, title: targetSong.name || targetSong.title, artist: targetSong.artist, duration: targetSong.duration, cover: targetSong.cover } } }
      }
    }
    return { success: false, error: 'Invalid index or playlist not available' }
  })

  // 加载网易云歌单
  bridge.registerHandler('media.loadNeteasePlaylist', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { playlistId } = (params || {}) as { playlistId?: string }

    if (!playlistId) {
      return { success: false, error: 'Playlist ID required' }
    }

    // 检查权限
    if (!tappInstance.grantedPermissions?.includes('media:control')) {
      return { success: false, error: 'Permission denied: media:control required' }
    }

    try {
      // 触发加载歌单事件
      window.dispatchEvent(new CustomEvent('music-player-load-playlist', {
        detail: {
          playlistId,
          source: 'netease',
        },
      }))

      return { success: true, data: { playlistId, source: 'netease', loading: true } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed to load playlist' }
    }
  })
}

/**
 * 注册 Background 处理器
 */
export function registerBackgroundHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  const validRequirements = ['widget', 'media', 'sync', 'notification', 'scheduler', 'event-listener', 'realtime']

  bridge.registerHandler('background.require', async (message) => {
    const [requirement, reason] = (message.payload as { args: unknown[] }).args || []
    if (!requirement)
      return { success: false, error: 'Requirement required' }
    if (!validRequirements.includes(requirement as string)) {
      return { success: false, error: `Invalid requirement. Valid: ${validRequirements.join(', ')}` }
    }
    try {
      const runtime = getTappRuntime()
      runtime.registerBackgroundRequirement(tappInstance.id, requirement as 'widget' | 'notification' | 'sync' | 'media' | 'scheduler' | 'event-listener' | 'realtime')
      console.log(`[Sandbox] ${tappInstance.id} background: ${requirement}${reason ? ` (${reason})` : ''}`)
      return { success: true, data: { requirement, registered: true } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('background.release', async (message) => {
    const [requirement] = (message.payload as { args: unknown[] }).args || []
    if (!requirement)
      return { success: false, error: 'Requirement required' }
    try {
      const runtime = getTappRuntime()
      runtime.unregisterBackgroundRequirement(tappInstance.id, requirement as 'widget' | 'notification' | 'sync' | 'media' | 'scheduler' | 'event-listener' | 'realtime')
      return { success: true, data: { requirement, released: true } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('background.list', async () => {
    try {
      const runtime = getTappRuntime()
      const requirements = runtime.getBackgroundRequirements(tappInstance.id)
      return { success: true, data: requirements }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('background.has', async (message) => {
    const [requirement] = (message.payload as { args: unknown[] }).args || []
    if (!requirement)
      return { success: false, error: 'Requirement required' }
    try {
      const runtime = getTappRuntime()
      const requirements = runtime.getBackgroundRequirements(tappInstance.id)
      return { success: true, data: requirements.includes(requirement as 'widget' | 'notification' | 'sync' | 'media' | 'scheduler' | 'event-listener' | 'realtime') }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })
}

/**
 * 注册 Animation 处理器
 */
export function registerAnimationHandlers(
  bridge: TappBridge,
  animationConfigRef?: React.RefObject<AnimationConfigRef>,
): void {
  bridge.registerHandler('animation.getLevel', async () => {
    return { success: true, data: animationConfigRef?.current?.level || 'standard' }
  })

  bridge.registerHandler('animation.shouldAnimate', async () => {
    return { success: true, data: (animationConfigRef?.current?.level || 'standard') !== 'none' }
  })

  bridge.registerHandler('animation.getConfig', async () => {
    const cfg = animationConfigRef?.current
    return {
      success: true,
      data: cfg || { level: 'standard', loop: true, spring: { tension: 280, friction: 20 }, durationScale: 1 },
    }
  })

  bridge.registerHandler('animation.getStaggerDelay', async (message) => {
    const [index, baseDelay = 50] = (message.payload as { args: unknown[] }).args || []
    if (typeof index !== 'number')
      return { success: false, error: 'Index required' }
    const cfg = animationConfigRef?.current
    if (!cfg)
      return { success: true, data: index * (baseDelay as number) }
    let delay = baseDelay as number
    if (cfg.level === 'none')
      delay = 0
    else if (cfg.level === 'light')
      delay = (baseDelay as number) * 0.5
    return { success: true, data: index * delay * cfg.durationScale }
  })
}

/**
 * 注册 DynamicContent 处理器
 */
export function registerDynamicContentHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('dynamicContent.set', async (message) => {
    const [config] = (message.payload as { args: unknown[] }).args || []
    const { icon, text, subtext, priority, showSubtext, expiresAt, i18n } = (config || {}) as {
      icon?: string
      text?: string
      subtext?: string
      priority?: number
      showSubtext?: boolean
      expiresAt?: number
      i18n?: unknown
    }
    if (!icon || !text)
      return { success: false, error: 'Icon and text required' }
    try {
      const provider = getDynamicContentProvider()
      const content: Omit<DynamicContentItem, 'sourceTappId'> = {
        type: `tapp-${tappInstance.id}`,
        icon,
        text,
        subtext,
        priority: priority ?? -1,
        showSubtext: showSubtext ?? !!subtext,
        onClick: 'expand',
        expiresAt,
        i18n: i18n as DynamicContentItem['i18n'],
      }
      provider.setTappContent(tappInstance.id, content)
      getTappRuntime().registerBackgroundRequirement(tappInstance.id, 'notification')
      return { success: true, data: { registered: true } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('dynamicContent.update', async (message) => {
    const [updates] = (message.payload as { args: unknown[] }).args || []
    if (!updates)
      return { success: false, error: 'Updates required' }
    try {
      const provider = getDynamicContentProvider()
      const existing = provider.getTappContent(tappInstance.id)
      if (!existing)
        return { success: false, error: 'No content found. Use set first.' }
      provider.setTappContent(tappInstance.id, { ...existing, ...(updates as Partial<DynamicContentItem>), type: existing.type })
      return { success: true, data: { updated: true } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('dynamicContent.remove', async () => {
    try {
      const provider = getDynamicContentProvider()
      provider.removeTappContent(tappInstance.id)
      getTappRuntime().unregisterBackgroundRequirement(tappInstance.id, 'notification')
      return { success: true, data: { removed: true } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('dynamicContent.get', async () => {
    try {
      const provider = getDynamicContentProvider()
      const content = provider.getTappContent(tappInstance.id)
      return { success: true, data: content || null }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })
}

/**
 * 注册 Component/Shortcut/Event 处理器
 */
export function registerAdvancedHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  // Component handlers
  bridge.registerHandler('component.registerTheme', async (message) => {
    const [config] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.registerComponent(tappInstance.id, 'theme', config as TappApiService.ComponentConfig)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('component.registerAgent', async (message) => {
    const [config] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.registerComponent(tappInstance.id, 'agent', config as TappApiService.ComponentConfig)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('component.unregister', async (message) => {
    const [type, id] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.unregisterComponent(tappInstance.id, type as TappApiService.ComponentType, id as string)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('component.list', async (message) => {
    const [type] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.listComponents(tappInstance.id, type as TappApiService.ComponentType | undefined)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  // Shortcut handlers
  bridge.registerHandler('shortcut.register', async (message) => {
    const [config] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.registerShortcut(tappInstance.id, config as TappApiService.ShortcutConfig)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('shortcut.unregister', async (message) => {
    const [id] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.unregisterShortcut(tappInstance.id, id as string)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('shortcut.list', async () => {
    try {
      const result = await TappApiService.listShortcuts(tappInstance.id)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  // Event handlers
  bridge.registerHandler('event.publish', async (message) => {
    const [eventType, payload, target] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.publishEvent({ tappId: tappInstance.id, eventType: eventType as string, payload, target: target as string })
      bridge.emit(`tapp:${eventType}`, { source: tappInstance.id, payload })
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('event.subscribe', async (message) => {
    const [eventTypes] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.updateEventSubscriptions(tappInstance.id, eventTypes as string[])
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('event.unsubscribe', async (message) => {
    const [eventTypes] = (message.payload as { args: unknown[] }).args || []
    try {
      const current = await TappApiService.getEventSubscriptions(tappInstance.id)
      const updated = ((current.subscriptions || []) as string[]).filter(t => !(eventTypes as string[]).includes(t))
      const result = await TappApiService.updateEventSubscriptions(tappInstance.id, updated)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })
}

/**
 * 注册 Context/Fetch/Data 处理器
 */
export function registerContextHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('context.getApp', async () => {
    try { return { success: true, data: await TappApiService.getContextApp() } }
    catch (error) { return { success: false, error: error instanceof Error ? error.message : 'Failed' } }
  })

  bridge.registerHandler('context.getUser', async () => {
    try { return { success: true, data: await TappApiService.getContextUser() } }
    catch (error) { return { success: false, error: error instanceof Error ? error.message : 'Failed' } }
  })

  bridge.registerHandler('context.getPlayer', async () => {
    try { return { success: true, data: await TappApiService.getContextPlayer() } }
    catch (error) { return { success: false, error: error instanceof Error ? error.message : 'Failed' } }
  })

  bridge.registerHandler('context.getNavigation', async () => {
    try { return { success: true, data: await TappApiService.getContextNavigation() } }
    catch (error) { return { success: false, error: error instanceof Error ? error.message : 'Failed' } }
  })

  bridge.registerHandler('context.getSystem', async () => {
    try { return { success: true, data: await TappApiService.getContextSystem() } }
    catch (error) { return { success: false, error: error instanceof Error ? error.message : 'Failed' } }
  })

  bridge.registerHandler('data.transform', async (message) => {
    const [request] = (message.payload as { args: unknown[] }).args || []
    const req = request as { input?: unknown, pipeline?: unknown, output?: unknown }
    if (!req?.input || !req?.pipeline)
      return { success: false, error: 'Input and pipeline required' }
    try {
      const response = await TappApiService.dataTransform({ tappId: tappInstance.id, input: req.input as TappApiService.DataInput, pipeline: req.pipeline as TappApiService.ProcessStep[], output: req.output as TappApiService.DataOutput | undefined })
      return { success: true, data: response }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  // ============ Tapp API 声明系统 ============

  // 执行 Tapp manifest 中声明的 API
  bridge.registerHandler('api.execute', async (message) => {
    const [apiName, params] = (message.payload as { args: unknown[] }).args || []
    if (!apiName || typeof apiName !== 'string') {
      return { success: false, error: 'API name required' }
    }

    try {
      const response = await TappApiService.executeTappApi(
        tappInstance.id,
        apiName,
        params as Record<string, unknown> | undefined,
      )
      return { success: response.success, data: response.data, error: response.error }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  // 列出 Tapp 可用的 API
  bridge.registerHandler('api.list', async () => {
    try {
      const apis = await TappApiService.listTappApis(tappInstance.id)
      return { success: true, data: apis }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  // 获取客户端地理位置
  bridge.registerHandler('context.getGeo', async () => {
    try {
      const geo = await TappApiService.getContextGeo()
      return { success: true, data: geo }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  // 检测用户是否在中国大陆（用于判断是否需要代理）
  bridge.registerHandler('context.isInChinaMainland', async () => {
    try {
      const isInChina = await TappApiService.isUserInChinaMainland()
      return { success: true, data: isInChina }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })
}
