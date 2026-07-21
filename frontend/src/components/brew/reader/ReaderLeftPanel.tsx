/**
 * 阅读器左侧控制栏组件
 * 包含: 返回、来源、进度、目录、收藏、AI注释、AI播客、外部链接
 */

import type { MouseEvent } from 'react'

import type { ReaderLeftPanelProps } from './types'
import {
  LuArrowRight as ArrowRight,
  LuChevronLeft as ChevronLeft,
  LuChevronRight as ChevronRight,
  LuCloud as Cloud,
  LuExternalLink as ExternalLink,
  LuEye as Eye,
  LuEyeOff as EyeOff,
  LuList as List,
  LuMic as Mic,
  LuMonitor as Monitor,
  LuPause as Pause,
  LuPlay as Play,
  LuRefreshCw as RefreshCw,
  LuSettings as Settings,
  LuSkipBack as SkipBack,
  LuSkipForward as SkipForward,
  LuSparkles as Sparkles,
  LuSquare as Square,
  LuStar as Star,
  LuTrash2 as Trash2,
  LuVolume2 as Volume2,
  LuX as X,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useMemo, useRef } from 'react'

import * as brewliaApi from '../../../services/brewliaApi'
import { Spinner } from '../../Spinner'
import { STYLE_MAX_HEIGHT_320 } from './constants'

export default memo(
  ({
    item,
    onClose,
    isAuthenticated,
    isAdmin,
    isBrewlia,
    currentTheme,
    isDark,
    readingProgress,
    showPanels,
    toc,
    showToc,
    setShowToc,
    activeHeadingId,
    scrollToHeading,
    onToggleStar,
    annotations,
    annotationsLoading,
    showAnnotations,
    showBrewliaPanel,
    setShowBrewliaPanel,
    toggleAnnotations,
    loadAnnotations,
    regenerateAnnotations,
    annotationsError,
    selectedAnnotation,
    setSelectedAnnotation,
    scrollToAnnotation,
    podcastDialogues,
    podcastLoading,
    cloudTtsLoading,
    podcastState,
    showPodcastPlayer,
    setShowPodcastPlayer,
    loadPodcast,
    podcastCurrentIndex,
    ttsEngine,
    handleTtsEngineChange,
    cloudTtsAvailable,
    cloudTtsError,
    cloudTtsLoadProgress,
    voiceList,
    showVoiceSettings,
    setShowVoiceSettings,
    hostVoiceId,
    guestVoiceId,
    handleVoiceSelect,
    handleOpenSettings,
    articleCache,
    articleCacheLoading,
    clearingVoiceId,
    handleClearVoiceCache,
    handleSwitchToCachedVoice,
    reloadCloudTts,
    handlePlayPause,
    handleStop,
    handlePrevious,
    handleNext,
    handleDialogueClick,
    handleProgressPointerDown,
    handleProgressPointerUp,
    handleProgressPointerLeave,
    enableAnimations,
    sideButtonClass,
    onMouseEnter,
    onMouseLeave,
    t,
  }: ReaderLeftPanelProps) => {
    const podcastListRef = useRef<HTMLDivElement>(null)

    // 音色ID到名称的映射
    const voiceNameById = useMemo(() => {
      const map = new Map<number, string>()
      voiceList.forEach((v) => map.set(v.id, v.name))
      return map
    }, [voiceList])

    // 分组音色列表
    const groupedVoices = useMemo(() => {
      const ultra: typeof voiceList = []
      const llm: typeof voiceList = []
      const premium: typeof voiceList = []

      voiceList.forEach((voice) => {
        if (voice.voice_type === 'ultra_natural') {
          ultra.push(voice)
        } else if (voice.voice_type === 'llm') {
          llm.push(voice)
        } else {
          premium.push(voice)
        }
      })

      // 预分组男女音色
      const isMale = (v: (typeof voiceList)[0]) =>
        v.gender === '男' || v.gender === '男童'
      const isFemale = (v: (typeof voiceList)[0]) =>
        v.gender === '女' || v.gender === '女童'

      return {
        ultra,
        llm,
        premium,
        ultraMale: ultra.filter(isMale),
        ultraFemale: ultra.filter(isFemale),
        llmMale: llm.filter(isMale),
        llmFemale: llm.filter(isFemale),
        premiumMale: premium.filter((v) => v.gender === '男'),
        premiumFemale: premium.filter((v) => v.gender === '女'),
      }
    }, [voiceList])

    return (
      // 常驻 DOM 避免每次 showPanels 切换时 unmount/remount backdrop-blur 层（代价极高）
      // 改用 animate 控制 opacity/transform，pointerEvents 控制交互
      <>
        <motion.aside
          initial={{ opacity: 0, x: -24, scale: 0.92 }}
          animate={
            enableAnimations
              ? showPanels
                ? { opacity: 1, x: 0, scale: 1 }
                : { opacity: 0, x: -24, scale: 0.92 }
              : { opacity: showPanels ? 1 : 0 }
          }
          transition={
            enableAnimations
              ? { duration: 0.3, ease: [0.16, 1, 0.3, 1] }
              : { duration: 0 }
          }
          className="hidden sm:block sticky top-1/3 -translate-y-1/3 h-fit mr-4 z-20"
          style={{
            pointerEvents: showPanels ? 'auto' : 'none',
            willChange: 'transform, opacity',
          }}
          onClick={(e: MouseEvent) => e.stopPropagation()}
          onMouseEnter={onMouseEnter}
          onMouseLeave={onMouseLeave}
        >
          <div
            className={`flex flex-col items-center gap-2 p-2 rounded-2xl backdrop-blur-md border ${currentTheme.border} ${currentTheme.surface}`}
          >
            {/* 返回按钮 */}
            <button
              onClick={onClose}
              className={sideButtonClass}
              title={t.brew.backEsc}
            >
              <ChevronLeft className="w-5 h-5" />
            </button>

            {/* 分隔线 */}
            <div
              className={`w-6 h-px ${isDark ? 'bg-white/10' : 'bg-black/10'}`}
            />

            {/* 来源图标 */}
            {item.source_icon && (
              <div className="p-1">
                <img
                  src={item.source_icon || undefined}
                  alt=""
                  className="w-6 h-6 rounded-lg"
                  title={item.source_name || undefined}
                />
              </div>
            )}

            {/* 阅读进度 - 圆形进度（点击返回上一段落，长按返回顶部） */}
            <button
              className="relative w-10 h-10 flex items-center justify-center cursor-pointer select-none"
              title={t.brew.clickBackLongTop}
              onPointerDown={handleProgressPointerDown}
              onPointerUp={handleProgressPointerUp}
              onPointerLeave={handleProgressPointerLeave}
            >
              <svg className="w-10 h-10 -rotate-90">
                <circle
                  cx="20"
                  cy="20"
                  r="16"
                  fill="none"
                  stroke={isDark ? 'rgba(255,255,255,0.1)' : 'rgba(0,0,0,0.1)'}
                  strokeWidth="3"
                />
                <circle
                  cx="20"
                  cy="20"
                  r="16"
                  fill="none"
                  stroke={currentTheme.accent}
                  strokeWidth="3"
                  strokeLinecap="round"
                  strokeDasharray={`${readingProgress} 100`}
                  className="transition-all duration-300"
                />
              </svg>
              <span
                className={`absolute text-[10px] font-medium ${currentTheme.text} tabular-nums`}
              >
                {readingProgress}
              </span>
            </button>

            {/* 分隔线 */}
            <div
              className={`w-6 h-px ${isDark ? 'bg-white/10' : 'bg-black/10'}`}
            />

            {/* 目录按钮 */}
            {toc.length > 0 && (
              <button
                onClick={() => {
                  if (!showToc) {
                    setShowBrewliaPanel(false)
                    setShowPodcastPlayer(false)
                  }
                  setShowToc(!showToc)
                }}
                className={`${sideButtonClass} ${showToc ? (isDark ? 'bg-white/10' : 'bg-black/5') : ''}`}
                title={t.brew.tableOfContents}
              >
                <List className="w-5 h-5" />
              </button>
            )}

            {/* 收藏 - 仅登录用户可见 */}
            {isAuthenticated && (
              <button
                onClick={onToggleStar}
                className={`p-2.5 rounded-xl transition-all duration-200 ${
                  item.is_starred
                    ? 'text-amber-500 bg-amber-500/10'
                    : `${currentTheme.secondary} hover:text-amber-500 ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                }`}
                title={item.is_starred ? t.brew.unstar : t.brew.starred}
              >
                <Star
                  className={`w-5 h-5 ${item.is_starred ? 'fill-current' : ''}`}
                />
              </button>
            )}

            {/* Brewlia AI 词汇注释 - 仅 Brewlia 订阅可用，非管理员需要有缓存才显示 */}
            {isBrewlia && (isAdmin || item.has_ai_annotations) && (
              <button
                onClick={() => {
                  if (!showBrewliaPanel) {
                    setShowToc(false)
                    setShowPodcastPlayer(false)
                  }
                  setShowBrewliaPanel(!showBrewliaPanel)
                }}
                disabled={annotationsLoading}
                className={`p-2.5 rounded-xl transition-all duration-200 ${
                  showBrewliaPanel ||
                  (showAnnotations && annotations.length > 0)
                    ? 'text-purple-500 bg-purple-500/10'
                    : annotationsLoading
                      ? `${currentTheme.secondary} opacity-50`
                      : `${currentTheme.secondary} hover:text-purple-500 ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                }`}
                title={annotationsLoading ? `${t.brew.loading}...` : 'AI'}
              >
                {annotationsLoading ? (
                  <Spinner size="sm" color="current" />
                ) : (
                  <Sparkles
                    className={`w-5 h-5 ${showAnnotations && annotations.length > 0 ? 'fill-current' : ''}`}
                  />
                )}
              </button>
            )}

            {/* Brewlia AI 播客 - 仅 Brewlia 订阅可用，非管理员需要有缓存才显示 */}
            {isBrewlia && (isAdmin || item.has_ai_podcast) && (
              <button
                onClick={() => {
                  if (podcastDialogues.length === 0) {
                    setShowToc(false)
                    setShowBrewliaPanel(false)
                    loadPodcast()
                  } else {
                    if (!showPodcastPlayer) {
                      setShowToc(false)
                      setShowBrewliaPanel(false)
                    }
                    setShowPodcastPlayer(!showPodcastPlayer)
                  }
                }}
                disabled={podcastLoading || cloudTtsLoading}
                className={`p-2.5 rounded-xl transition-all duration-200 ${
                  podcastState === 'playing'
                    ? 'text-emerald-500 bg-emerald-500/10 animate-pulse'
                    : showPodcastPlayer && podcastDialogues.length > 0
                      ? 'text-emerald-500 bg-emerald-500/10'
                      : podcastLoading || cloudTtsLoading
                        ? `${currentTheme.secondary} opacity-50`
                        : `${currentTheme.secondary} hover:text-emerald-500 ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                }`}
                title={
                  podcastLoading
                    ? t.brew.generatingPodcast
                    : cloudTtsLoading
                      ? `${t.brew.loading}...`
                      : podcastDialogues.length > 0
                        ? showPodcastPlayer
                          ? t.brew.closePlayer
                          : t.brew.play
                        : 'AI'
                }
              >
                {podcastLoading || cloudTtsLoading ? (
                  <Spinner size="sm" color="current" />
                ) : (
                  <Mic
                    className={`w-5 h-5 ${podcastState === 'playing' || (showPodcastPlayer && podcastDialogues.length > 0) ? 'fill-current' : ''}`}
                  />
                )}
              </button>
            )}

            {/* 外部链接 */}
            <a
              href={item.link}
              target="_blank"
              rel="noopener noreferrer"
              className={sideButtonClass}
              title={t.brew.readOriginal}
            >
              <ExternalLink className="w-5 h-5" />
            </a>
          </div>

          {/* 目录面板 */}
          <AnimatePresence>
            {showToc && toc.length > 0 && (
              <motion.div
                initial={
                  enableAnimations ? { opacity: 0, x: -12, scale: 0.96 } : false
                }
                animate={
                  enableAnimations ? { opacity: 1, x: 0, scale: 1 } : undefined
                }
                exit={
                  enableAnimations
                    ? { opacity: 0, x: -12, scale: 0.96 }
                    : undefined
                }
                transition={
                  enableAnimations
                    ? { duration: 0.25, ease: [0.16, 1, 0.3, 1] }
                    : undefined
                }
                className={`absolute left-full top-0 ml-2 w-64 max-h-[50vh] overflow-y-auto rounded-2xl backdrop-blur-md border ${currentTheme.border} ${currentTheme.surface} p-3`}
              >
                <div
                  className={`text-xs font-medium ${currentTheme.secondary} mb-2 px-2`}
                >
                  {t.brew.tocTitle} ({toc.length})
                </div>
                <nav className="space-y-0.5">
                  {(() => {
                    // 预计算最小层级，避免在 map 内部重复计算 O(n²) -> O(n)
                    const minLevel =
                      toc.length > 0 ? Math.min(...toc.map((t) => t.level)) : 1
                    return toc.map((item) => {
                      const isActive = item.id === activeHeadingId
                      // 计算缩进，h1 不缩进，h2 缩进一级，以此类推
                      const indent = (item.level - minLevel) * 12

                      return (
                        <button
                          key={item.id}
                          onClick={() => scrollToHeading(item.id)}
                          className={`w-full text-left px-2 py-1.5 rounded-lg text-sm transition-all duration-200 ease-out truncate ${
                            isActive
                              ? `${isDark ? 'bg-white/10' : 'bg-black/5'} ${currentTheme.text} font-medium`
                              : `${currentTheme.secondary} hover:${currentTheme.text} ${isDark ? 'hover:bg-white/5' : 'hover:bg-black/3'}`
                          }`}
                          style={{ paddingLeft: `${8 + indent}px` }}
                          title={item.text}
                        >
                          {isActive && (
                            <ChevronRight className="w-3 h-3 inline-block mr-1 -ml-1" />
                          )}
                          {item.text}
                        </button>
                      )
                    })
                  })()}
                </nav>
              </motion.div>
            )}
          </AnimatePresence>

          {/* Brewlia 阅读注释面板 */}
          <AnimatePresence>
            {showBrewliaPanel && (
              <motion.div
                initial={
                  enableAnimations ? { opacity: 0, x: -12, scale: 0.96 } : false
                }
                animate={
                  enableAnimations ? { opacity: 1, x: 0, scale: 1 } : undefined
                }
                exit={
                  enableAnimations
                    ? { opacity: 0, x: -12, scale: 0.96 }
                    : undefined
                }
                transition={
                  enableAnimations
                    ? { duration: 0.25, ease: [0.16, 1, 0.3, 1] }
                    : undefined
                }
                className={`absolute left-full ${showToc && toc.length > 0 ? 'top-[calc(100%+0.5rem)]' : 'top-0'} ml-2 w-72 overflow-hidden rounded-2xl backdrop-blur-md border ${currentTheme.border} ${currentTheme.surface} flex flex-col`}
                style={STYLE_MAX_HEIGHT_320} /* 约4个注释的高度 */
              >
                {/* 头部 */}
                <div
                  className={`flex items-center justify-between px-3 py-2.5 border-b ${currentTheme.border} shrink-0`}
                >
                  <div className="flex items-center gap-2">
                    <Sparkles className="w-4 h-4 text-purple-500" />
                    <span
                      className={`text-sm font-medium ${currentTheme.text}`}
                    >
                      {t.brew.aiAnnotations}{' '}
                      {annotations.length > 0 && `(${annotations.length})`}
                    </span>
                  </div>
                  <div className="flex items-center gap-1">
                    {/* 显示/隐藏切换 */}
                    <button
                      onClick={toggleAnnotations}
                      className={`p-1.5 rounded-lg transition-colors ${
                        showAnnotations
                          ? 'text-purple-500 bg-purple-500/10'
                          : `${currentTheme.secondary} hover:${currentTheme.text}`
                      }`}
                      title={
                        showAnnotations
                          ? t.brew.hideHighlight
                          : t.brew.showAnnotations
                      }
                    >
                      {showAnnotations ? (
                        <Eye className="w-3.5 h-3.5" />
                      ) : (
                        <EyeOff className="w-3.5 h-3.5" />
                      )}
                    </button>
                    {/* 刷新按钮 - 仅管理员可见 */}
                    {isAdmin && (
                      <button
                        onClick={regenerateAnnotations}
                        disabled={annotationsLoading}
                        className={`p-1.5 rounded-lg transition-colors ${currentTheme.secondary} hover:${currentTheme.text} disabled:opacity-50`}
                        title={t.brew.regenerate}
                      >
                        {annotationsLoading ? (
                          <Spinner size="xs" color="current" />
                        ) : (
                          <RefreshCw className="w-3.5 h-3.5" />
                        )}
                      </button>
                    )}
                  </div>
                </div>

                {/* 注释列表 */}
                <div className="overflow-y-auto flex-1 p-2">
                  {annotations.length === 0 ? (
                    <div
                      className={`py-6 text-center ${currentTheme.secondary}`}
                    >
                      {annotationsLoading ? (
                        <div className="flex flex-col items-center gap-2">
                          <Spinner size="md" className="text-purple-500" />
                          <p className="text-xs">{t.brew.analyzing}</p>
                        </div>
                      ) : (
                        <div className="flex flex-col items-center gap-2">
                          <Sparkles className="w-6 h-6 opacity-30" />
                          <p className="text-xs">{t.brew.noAnnotations}</p>
                          {isAdmin && (
                            <button
                              onClick={loadAnnotations}
                              className="text-xs text-purple-500 hover:text-purple-600 font-medium"
                            >
                              {t.brew.regenerate}
                            </button>
                          )}
                        </div>
                      )}
                    </div>
                  ) : (
                    <div className="space-y-1.5">
                      {annotations.map((annotation, index) => {
                        const typeConfig =
                          brewliaApi.ANNOTATION_TYPE_CONFIG[annotation.type] ||
                          brewliaApi.ANNOTATION_TYPE_CONFIG.term
                        const isSelected =
                          selectedAnnotation?.term === annotation.term

                        return (
                          <button
                            key={annotation.id || index}
                            onClick={() => {
                              setSelectedAnnotation(
                                isSelected ? null : annotation,
                              )
                              scrollToAnnotation(annotation)
                            }}
                            className={`w-full text-left p-2.5 rounded-xl transition-all duration-200 ease-out group ${
                              isSelected
                                ? `${typeConfig.bgColor} ${currentTheme.text}`
                                : `${isDark ? 'hover:bg-white/5' : 'hover:bg-black/2'}`
                            }`}
                          >
                            <div className="flex items-center gap-2 min-w-0">
                              <span
                                className={`text-xs px-1 py-0.5 rounded ${typeConfig.bgColor} ${typeConfig.color} shrink-0 whitespace-nowrap`}
                              >
                                {typeConfig.label}
                              </span>
                              <span
                                className={`text-sm font-medium ${currentTheme.text} min-w-0 flex-1 truncate`}
                              >
                                {annotation.term}
                              </span>
                              <ArrowRight
                                className={`w-3 h-3 ${currentTheme.secondary} opacity-0 group-hover:opacity-100 transition-opacity shrink-0`}
                              />
                            </div>
                            <p
                              className={`text-xs ${currentTheme.secondary} mt-1 break-words [overflow-wrap:anywhere] ${isSelected ? '' : 'line-clamp-1'}`}
                            >
                              {annotation.explanation}
                            </p>
                          </button>
                        )
                      })}
                    </div>
                  )}
                </div>

                {/* 错误提示 */}
                {annotationsError && (
                  <div
                    className={`px-3 py-2 text-xs text-red-500 bg-red-500/10 border-t ${currentTheme.border}`}
                  >
                    {annotationsError}
                  </div>
                )}
              </motion.div>
            )}
          </AnimatePresence>

          {/* Brewlia AI 播客播放器面板 */}
          <AnimatePresence>
            {showPodcastPlayer && podcastDialogues.length > 0 && (
              <motion.div
                initial={
                  enableAnimations ? { opacity: 0, x: -12, scale: 0.96 } : false
                }
                animate={
                  enableAnimations ? { opacity: 1, x: 0, scale: 1 } : undefined
                }
                exit={
                  enableAnimations
                    ? { opacity: 0, x: -12, scale: 0.96 }
                    : undefined
                }
                transition={
                  enableAnimations
                    ? { duration: 0.25, ease: [0.16, 1, 0.3, 1] }
                    : undefined
                }
                className={`absolute left-full ${showBrewliaPanel || (showToc && toc.length > 0) ? 'top-[calc(100%+0.5rem)]' : 'top-0'} ml-2 w-80 overflow-hidden rounded-2xl backdrop-blur-md border ${currentTheme.border} ${currentTheme.surface} flex flex-col`}
                style={STYLE_MAX_HEIGHT_320}
              >
                {/* 头部 */}
                <div
                  className={`flex items-center justify-between px-3 py-2.5 border-b ${currentTheme.border} shrink-0`}
                >
                  <div className="flex items-center gap-2">
                    <Mic className="w-4 h-4 text-emerald-500" />
                    <span
                      className={`text-sm font-medium ${currentTheme.text}`}
                    >
                      {t.brew.aiPodcast}
                    </span>
                    <span className={`text-xs ${currentTheme.secondary}`}>
                      {podcastCurrentIndex + 1}/{podcastDialogues.length}
                    </span>
                  </div>
                  <div className="flex items-center gap-1">
                    {/* TTS 引擎切换 */}
                    <div className="flex items-center gap-0.5 mr-1">
                      <button
                        onClick={() => handleTtsEngineChange('system')}
                        disabled={cloudTtsLoading}
                        className={`p-1.5 rounded-lg transition-colors ${
                          ttsEngine === 'system'
                            ? 'bg-emerald-500/20 text-emerald-500'
                            : cloudTtsLoading
                              ? 'opacity-30 cursor-not-allowed'
                              : `${currentTheme.secondary} hover:${currentTheme.text}`
                        }`}
                        title={t.brew.systemTts}
                      >
                        <Monitor className="w-3.5 h-3.5" />
                      </button>
                      <button
                        onClick={() => handleTtsEngineChange('cloud')}
                        disabled={cloudTtsLoading}
                        className={`p-1.5 rounded-lg transition-colors ${
                          ttsEngine === 'cloud'
                            ? 'bg-emerald-500/20 text-emerald-500'
                            : cloudTtsLoading
                              ? 'opacity-30 cursor-not-allowed'
                              : cloudTtsAvailable
                                ? `${currentTheme.secondary} hover:${currentTheme.text}`
                                : `${currentTheme.secondary} hover:${currentTheme.text} opacity-60`
                        }`}
                        title={
                          cloudTtsLoading
                            ? `${t.brew.loading}...`
                            : cloudTtsAvailable
                              ? t.brew.cloudTts
                              : cloudTtsError || t.brew.cloudTtsUnavailable
                        }
                      >
                        {/* 加载环由下方带进度的状态行独担，按钮不再重复转圈 */}
                        <Cloud className="w-3.5 h-3.5" />
                      </button>
                    </div>
                    {/* 设置按钮 - 仅管理员可见 */}
                    {isAdmin && (
                      <button
                        onClick={handleOpenSettings}
                        className={`p-1.5 rounded-lg transition-colors mr-1 ${
                          showVoiceSettings
                            ? 'bg-emerald-500/20 text-emerald-500'
                            : `${currentTheme.secondary} hover:${currentTheme.text}`
                        }`}
                        title={t.brew.settings}
                      >
                        <Settings className="w-3.5 h-3.5" />
                      </button>
                    )}
                    <button
                      onClick={() => setShowPodcastPlayer(false)}
                      className={`p-1 rounded-lg transition-colors ${currentTheme.secondary} hover:${currentTheme.text}`}
                      title={t.brew.closePlayer}
                    >
                      <X className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>

                {/* 云端 TTS 加载状态 */}
                {cloudTtsLoading && (
                  <div
                    className={`px-3 py-2 text-xs ${currentTheme.secondary} bg-emerald-500/5 flex items-center gap-2 shrink-0`}
                  >
                    <Spinner size="xs" color="current" />
                    <span>
                      {t.brew.loadingCloudVoice} {cloudTtsLoadProgress.loaded}/
                      {cloudTtsLoadProgress.total}
                    </span>
                  </div>
                )}

                {/* 内容区域 - 设置 或 对话列表 切换显示 */}
                {showVoiceSettings ? (
                  /* 设置页面 */
                  <div className="px-3 py-2 flex-1 overflow-y-auto">
                    <div className="space-y-2">
                      {/* 返回按钮 */}
                      <div className="flex items-center justify-between">
                        <button
                          onClick={() => setShowVoiceSettings(false)}
                          className={`flex items-center gap-1 text-xs ${currentTheme.secondary} hover:${currentTheme.text} transition-colors`}
                        >
                          <ChevronLeft className="w-3.5 h-3.5" />
                          {t.brew.back}
                        </button>
                        <span className={`text-xs ${currentTheme.secondary}`}>
                          {t.brew.podcastSettings}
                        </span>
                      </div>

                      {/* 重新生成操作 - 仅管理员可见 */}
                      {isAdmin && (
                        <div
                          className={`p-2 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/2'}`}
                        >
                          <div
                            className={`text-xs ${currentTheme.secondary} mb-1.5`}
                          >
                            {t.brew.regenerateLabel}
                          </div>
                          <div className="flex gap-1.5">
                            <button
                              onClick={() => {
                                // regeneratePodcast needs to be passed
                                setShowVoiceSettings(false)
                              }}
                              disabled={podcastLoading}
                              className={`flex-1 px-2 py-1.5 text-xs rounded-md transition-colors flex items-center justify-center gap-1 ${isDark ? 'bg-white/10 hover:bg-white/15' : 'bg-black/5 hover:bg-black/10'} ${currentTheme.text} disabled:opacity-50`}
                            >
                              <RefreshCw className="w-3 h-3" />
                              {t.brew.podcastScript}
                            </button>
                            {cloudTtsAvailable && (
                              <button
                                onClick={() => {
                                  reloadCloudTts()
                                  setShowVoiceSettings(false)
                                }}
                                disabled={cloudTtsLoading}
                                className={`flex-1 px-2 py-1.5 text-xs rounded-md transition-colors flex items-center justify-center gap-1 ${isDark ? 'bg-white/10 hover:bg-white/15' : 'bg-black/5 hover:bg-black/10'} ${currentTheme.text} disabled:opacity-50`}
                              >
                                <Cloud className="w-3 h-3" />
                                {t.brew.voice}
                              </button>
                            )}
                          </div>
                        </div>
                      )}

                      {/* 云端音色设置 - 仅云端TTS显示 */}
                      {ttsEngine === 'cloud' &&
                        cloudTtsAvailable &&
                        voiceList.length > 0 && (
                          <>
                            {/* 主播音色 */}
                            <div
                              className={`p-2 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/2'}`}
                            >
                              <div
                                className={`text-xs ${currentTheme.secondary} mb-1.5 flex items-center justify-between`}
                              >
                                <span>{t.brew.hostAnchor}</span>
                                {hostVoiceId ? (
                                  <span className="text-emerald-500">
                                    {voiceNameById.get(hostVoiceId)}
                                  </span>
                                ) : (
                                  <span>{t.brew.defaultVoice}</span>
                                )}
                              </div>
                              <div className="flex flex-wrap gap-1">
                                <button
                                  onClick={() => handleVoiceSelect('host', 0)}
                                  className={`px-2 py-1 text-xs rounded-md transition-colors ${
                                    !hostVoiceId
                                      ? 'bg-emerald-500/20 text-emerald-500'
                                      : `${currentTheme.secondary} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                                  }`}
                                >
                                  {t.brew.defaultVoice}
                                </button>
                                {groupedVoices.ultraMale.map((voice) => (
                                  <button
                                    key={voice.id}
                                    onClick={() =>
                                      handleVoiceSelect('host', voice.id)
                                    }
                                    className={`px-2 py-1 text-xs rounded-md transition-colors ${
                                      hostVoiceId === voice.id
                                        ? 'bg-emerald-500/20 text-emerald-500'
                                        : `${currentTheme.secondary} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                                    }`}
                                    title={`${voice.description}${t.brew.voiceSuperNaturalSuffix}`}
                                  >
                                    {voice.name}
                                    <span className="ml-1 opacity-70">
                                      {t.brew.superNatural}
                                    </span>
                                  </button>
                                ))}
                                {groupedVoices.llmMale.map((voice) => (
                                  <button
                                    key={voice.id}
                                    onClick={() =>
                                      handleVoiceSelect('host', voice.id)
                                    }
                                    className={`px-2 py-1 text-xs rounded-md transition-colors ${
                                      hostVoiceId === voice.id
                                        ? 'bg-emerald-500/20 text-emerald-500'
                                        : `${currentTheme.secondary} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                                    }`}
                                    title={`${voice.description}${voice.emotion_support ? t.brew.voiceEmotionalSuffix : ''}`}
                                  >
                                    {voice.name}
                                    {voice.emotion_support && (
                                      <span className="ml-1 opacity-70">
                                        {t.brew.emotionalLabel}
                                      </span>
                                    )}
                                  </button>
                                ))}
                                {groupedVoices.premiumMale.map((voice) => (
                                  <button
                                    key={voice.id}
                                    onClick={() =>
                                      handleVoiceSelect('host', voice.id)
                                    }
                                    className={`px-2 py-1 text-xs rounded-md transition-colors ${
                                      hostVoiceId === voice.id
                                        ? 'bg-emerald-500/20 text-emerald-500'
                                        : `${currentTheme.secondary} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                                    }`}
                                    title={voice.description}
                                  >
                                    {voice.name}
                                  </button>
                                ))}
                              </div>
                            </div>

                            {/* 嘉宾音色 */}
                            <div
                              className={`p-2 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/2'}`}
                            >
                              <div
                                className={`text-xs ${currentTheme.secondary} mb-1.5 flex items-center justify-between`}
                              >
                                <span>{t.brew.guestLabel}</span>
                                {guestVoiceId ? (
                                  <span className="text-emerald-500">
                                    {voiceNameById.get(guestVoiceId)}
                                  </span>
                                ) : (
                                  <span>{t.brew.defaultVoice}</span>
                                )}
                              </div>
                              <div className="flex flex-wrap gap-1">
                                <button
                                  onClick={() => handleVoiceSelect('guest', 0)}
                                  className={`px-2 py-1 text-xs rounded-md transition-colors ${
                                    !guestVoiceId
                                      ? 'bg-emerald-500/20 text-emerald-500'
                                      : `${currentTheme.secondary} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                                  }`}
                                >
                                  {t.brew.defaultVoice}
                                </button>
                                {groupedVoices.ultraFemale.map((voice) => (
                                  <button
                                    key={voice.id}
                                    onClick={() =>
                                      handleVoiceSelect('guest', voice.id)
                                    }
                                    className={`px-2 py-1 text-xs rounded-md transition-colors ${
                                      guestVoiceId === voice.id
                                        ? 'bg-emerald-500/20 text-emerald-500'
                                        : `${currentTheme.secondary} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                                    }`}
                                    title={`${voice.description}${t.brew.voiceSuperNaturalSuffix}`}
                                  >
                                    {voice.name}
                                    <span className="ml-1 opacity-70">
                                      {t.brew.superNatural}
                                    </span>
                                  </button>
                                ))}
                                {groupedVoices.llmFemale.map((voice) => (
                                  <button
                                    key={voice.id}
                                    onClick={() =>
                                      handleVoiceSelect('guest', voice.id)
                                    }
                                    className={`px-2 py-1 text-xs rounded-md transition-colors ${
                                      guestVoiceId === voice.id
                                        ? 'bg-emerald-500/20 text-emerald-500'
                                        : `${currentTheme.secondary} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                                    }`}
                                    title={`${voice.description}${voice.emotion_support ? t.brew.voiceEmotionalSuffix : ''}`}
                                  >
                                    {voice.name}
                                    {voice.emotion_support && (
                                      <span className="ml-1 opacity-70">
                                        {t.brew.emotionalLabel}
                                      </span>
                                    )}
                                  </button>
                                ))}
                                {groupedVoices.premiumFemale.map((voice) => (
                                  <button
                                    key={voice.id}
                                    onClick={() =>
                                      handleVoiceSelect('guest', voice.id)
                                    }
                                    className={`px-2 py-1 text-xs rounded-md transition-colors ${
                                      guestVoiceId === voice.id
                                        ? 'bg-emerald-500/20 text-emerald-500'
                                        : `${currentTheme.secondary} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`
                                    }`}
                                    title={voice.description}
                                  >
                                    {voice.name}
                                  </button>
                                ))}
                              </div>
                            </div>
                          </>
                        )}

                      {/* 已缓存的音色 - 仅管理员可见 */}
                      {isAdmin &&
                        articleCache &&
                        articleCache.voices.length > 0 && (
                          <div
                            className={`p-2 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/2'}`}
                          >
                            <div
                              className={`text-xs ${currentTheme.secondary} mb-1.5`}
                            >
                              {t.brew.cachedVoices}
                            </div>
                            <div className="space-y-1">
                              {articleCache.voices.map((voice) => (
                                <div
                                  key={voice.voice_id}
                                  className={`flex items-center justify-between gap-2 px-2 py-1.5 rounded-md ${isDark ? 'bg-white/5' : 'bg-black/2'}`}
                                >
                                  <button
                                    onClick={() =>
                                      handleSwitchToCachedVoice(
                                        voice.voice_id,
                                        voice.role,
                                      )
                                    }
                                    className={`flex-1 text-left text-xs ${currentTheme.text} hover:text-emerald-500 transition-colors truncate`}
                                    title={t.brew.switchToThisVoice}
                                  >
                                    {voice.voice_name || voice.voice_id}
                                    <span
                                      className={`ml-1 ${currentTheme.secondary}`}
                                    >
                                      ({voice.file_count}
                                      {t.brew.fileCountSuffix})
                                    </span>
                                  </button>
                                  <button
                                    onClick={() =>
                                      handleClearVoiceCache(voice.voice_id)
                                    }
                                    disabled={
                                      clearingVoiceId === voice.voice_id
                                    }
                                    className="p-1 rounded text-red-500/70 hover:text-red-500 hover:bg-red-500/10 transition-all disabled:opacity-50"
                                    title={t.brew.clearCache}
                                  >
                                    {clearingVoiceId === voice.voice_id ? (
                                      <Spinner size="xs" color="current" />
                                    ) : (
                                      <Trash2 className="w-3 h-3" />
                                    )}
                                  </button>
                                </div>
                              ))}
                            </div>
                            {articleCacheLoading && (
                              <div
                                className={`flex items-center gap-1 mt-1.5 text-xs ${currentTheme.secondary}`}
                              >
                                <Spinner size="xs" color="current" />
                                {t.brew.loadingCache}
                              </div>
                            )}
                          </div>
                        )}
                    </div>
                  </div>
                ) : (
                  /* 对话列表 */
                  <>
                    {/* 对话列表 */}
                    <div
                      ref={podcastListRef}
                      className="overflow-y-auto flex-1 p-2 space-y-1.5"
                    >
                      {podcastDialogues.map((dialogue, index) => {
                        const isHostA = dialogue.speaker === 'host_a'
                        const isCurrent = index === podcastCurrentIndex

                        return (
                          <button
                            key={index}
                            data-podcast-index={index}
                            onClick={() => handleDialogueClick(index)}
                            className={`w-full text-left p-2.5 rounded-xl transition-all duration-200 ease-out ${
                              isCurrent
                                ? 'bg-emerald-500/15 ring-1 ring-emerald-500/30'
                                : `${isDark ? 'hover:bg-white/5' : 'hover:bg-black/2'}`
                            }`}
                          >
                            <div className="flex items-start gap-2">
                              <span
                                className={`text-xs px-1.5 py-0.5 rounded shrink-0 ${
                                  isHostA
                                    ? 'bg-blue-500/15 text-blue-500'
                                    : 'bg-pink-500/15 text-pink-500'
                                }`}
                              >
                                {isHostA ? t.brew.hostA : t.brew.hostB}
                              </span>
                              {isCurrent && podcastState === 'playing' && (
                                <Volume2 className="w-3.5 h-3.5 text-emerald-500 animate-pulse shrink-0" />
                              )}
                            </div>
                            <p
                              className={`text-sm ${currentTheme.text} mt-1.5 ${isCurrent ? '' : 'line-clamp-2'}`}
                            >
                              {dialogue.text}
                            </p>
                          </button>
                        )
                      })}
                    </div>

                    {/* 播放控制栏 */}
                    <div
                      className={`flex items-center justify-center gap-3 px-3 py-2.5 border-t ${currentTheme.border} shrink-0`}
                    >
                      <button
                        onClick={handlePrevious}
                        disabled={podcastCurrentIndex <= 0}
                        className={`p-1.5 rounded-lg transition-colors ${currentTheme.secondary} hover:${currentTheme.text} disabled:opacity-30`}
                        title={t.brew.previousSegment}
                      >
                        <SkipBack className="w-4 h-4" />
                      </button>
                      <button
                        onClick={handlePlayPause}
                        className={`p-2.5 rounded-xl transition-colors ${
                          podcastState === 'playing'
                            ? 'bg-emerald-500 text-white'
                            : `${isDark ? 'bg-white/10' : 'bg-black/5'} ${currentTheme.text}`
                        }`}
                        title={
                          podcastState === 'playing'
                            ? t.brew.pause
                            : t.brew.play
                        }
                      >
                        {podcastState === 'playing' ? (
                          <Pause className="w-5 h-5" />
                        ) : (
                          <Play className="w-5 h-5" />
                        )}
                      </button>
                      <button
                        onClick={handleNext}
                        disabled={
                          podcastCurrentIndex >= podcastDialogues.length - 1
                        }
                        className={`p-1.5 rounded-lg transition-colors ${currentTheme.secondary} hover:${currentTheme.text} disabled:opacity-30`}
                        title={t.brew.nextSegment}
                      >
                        <SkipForward className="w-4 h-4" />
                      </button>
                      <button
                        onClick={handleStop}
                        className={`p-1.5 rounded-lg transition-colors ${currentTheme.secondary} hover:${currentTheme.text}`}
                        title={t.brew.stop}
                      >
                        <Square className="w-4 h-4" />
                      </button>
                    </div>
                  </>
                )}
              </motion.div>
            )}
          </AnimatePresence>
        </motion.aside>
      </>
    )
  },
)
