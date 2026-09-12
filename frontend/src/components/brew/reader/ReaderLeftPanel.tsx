import type { MouseEvent } from 'react'

import type { ReaderToolPanel } from './readerPanels'
import type { ReaderLeftPanelProps } from './types'
import {
  LuArrowRight as ArrowRight,
  LuChevronLeft as ChevronLeft,
  LuChevronRight as ChevronRight,
  LuCloud as Cloud,
  LuEdit3 as Edit3,
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

import {
  isFemaleVoice,
  isMaleVoice,
  localizedVoiceDescription,
} from '../../../services/speechApi'
import { Spinner } from '../../Spinner'
import { annotationChrome } from './annotationChrome'
import {
  READER_ANNOTATIONS_PANEL_ID,
  READER_ANNOTATIONS_TITLE_ID,
  READER_PODCAST_PANEL_ID,
  READER_PODCAST_TITLE_ID,
  READER_TOC_PANEL_ID,
  READER_TOC_TITLE_ID,
  STYLE_MAX_HEIGHT_320,
} from './constants'
import {
  nextExclusivePanel,
  readerPanelFlags,
  readerPopupTrigger,
} from './readerPanels'
import { ReaderProgressRing } from './ReaderProgress'

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
    onEditNote,
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
    const openToolPanel = (panel: ReaderToolPanel) => {
      const current = showToc
        ? 'toc'
        : showBrewliaPanel
          ? 'annotations'
          : showPodcastPlayer
            ? 'podcast'
            : null
      const next = readerPanelFlags(nextExclusivePanel(current, panel))
      setShowToc(next.toc)
      setShowBrewliaPanel(next.brewlia)
      setShowPodcastPlayer(next.podcast)
    }

    const voiceNameById = useMemo(() => {
      const map = new Map<number, string>()
      voiceList.forEach((v) => map.set(v.id, v.name))
      return map
    }, [voiceList])

    const groupedVoices = useMemo(() => {
      const byBucket = Object.groupBy(voiceList, (voice) =>
        voice.voice_type === 'ultra_natural'
          ? 'ultra'
          : voice.voice_type === 'llm'
            ? 'llm'
            : 'premium',
      )
      const ultra = byBucket.ultra ?? []
      const llm = byBucket.llm ?? []
      const premium = byBucket.premium ?? []

      return {
        ultra,
        llm,
        premium,
        ultraMale: ultra.filter((v) => isMaleVoice(v.gender)),
        ultraFemale: ultra.filter((v) => isFemaleVoice(v.gender)),
        llmMale: llm.filter((v) => isMaleVoice(v.gender)),
        llmFemale: llm.filter((v) => isFemaleVoice(v.gender)),
        premiumMale: premium.filter((v) => isMaleVoice(v.gender)),
        premiumFemale: premium.filter((v) => isFemaleVoice(v.gender)),
      }
    }, [voiceList])

    const voiceTip = (voice: { id: number; description: string }) =>
      localizedVoiceDescription(t.brew, voice)

    return (
      // 常驻 DOM，避免切换时重挂 backdrop-blur。用 animate + pointerEvents，不卸载。
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
          className="hidden sm:flex sticky top-0 h-dvh items-center mr-4 z-20 pointer-events-none"
          style={{ willChange: 'transform, opacity' }}
        >
          {/* 胶囊相对视口垂直居中，不用 sticky。弹层 absolute 挂在 relative h-fit 上。 */}
          <div
            className="relative h-fit"
            style={{ pointerEvents: showPanels ? 'auto' : 'none' }}
            onClick={(e: MouseEvent) => e.stopPropagation()}
            onMouseEnter={onMouseEnter}
            onMouseLeave={onMouseLeave}
          >
            <div
              className={`flex flex-col items-center gap-2 p-2 rounded-2xl border ${currentTheme.border} ${currentTheme.surface}`}
            >
              <button
                onClick={onClose}
                className={sideButtonClass}
                title={t.brew.backEsc}
              >
                <ChevronLeft className="w-5 h-5" />
              </button>

              <div
                className={`w-6 h-px ${isDark ? 'bg-white/10' : 'bg-black/10'}`}
              />

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

              <button
                className="relative w-10 h-10 flex items-center justify-center cursor-pointer select-none"
                title={t.brew.clickBackLongTop}
                onPointerDown={handleProgressPointerDown}
                onPointerUp={handleProgressPointerUp}
                onPointerLeave={handleProgressPointerLeave}
              >
                <ReaderProgressRing
                  progress={readingProgress}
                  accent={currentTheme.accent}
                  track={isDark ? 'rgba(255,255,255,0.1)' : 'rgba(0,0,0,0.1)'}
                  labelClass={currentTheme.text}
                />
              </button>

              <div
                className={`w-6 h-px ${isDark ? 'bg-white/10' : 'bg-black/10'}`}
              />

              {toc.length > 0 && (
                <button
                  onClick={() => openToolPanel('toc')}
                  className={`${sideButtonClass} ${showToc ? (isDark ? 'bg-white/10' : 'bg-black/5') : ''}`}
                  title={t.brew.tableOfContents}
                  {...readerPopupTrigger(showToc, READER_TOC_PANEL_ID)}
                >
                  <List className="w-5 h-5" />
                </button>
              )}

              {isAuthenticated && (
                <button
                  onClick={onToggleStar}
                  className={`${sideButtonClass}${item.is_starred ? ' is-star' : ''}`}
                  title={item.is_starred ? t.brew.unstar : t.brew.starred}
                >
                  <Star
                    className={`w-5 h-5 ${item.is_starred ? 'fill-current' : ''}`}
                  />
                </button>
              )}

              {isBrewlia && (isAdmin || item.has_ai_annotations) && (
                <button
                  onClick={() => openToolPanel('annotations')}
                  {...readerPopupTrigger(
                    showBrewliaPanel,
                    READER_ANNOTATIONS_PANEL_ID,
                  )}
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

              {isBrewlia && (isAdmin || item.has_ai_podcast) && (
                <button
                  onClick={() => {
                    if (podcastDialogues.length === 0) loadPodcast()
                    openToolPanel('podcast')
                  }}
                  {...readerPopupTrigger(
                    showPodcastPlayer,
                    READER_PODCAST_PANEL_ID,
                  )}
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

              {onEditNote && (
                <button
                  onClick={onEditNote}
                  className={sideButtonClass}
                  title={t.brew.noteEdit}
                  aria-label={t.brew.noteEdit}
                >
                  <Edit3 className="w-5 h-5" />
                </button>
              )}

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

            <AnimatePresence>
              {showToc && toc.length > 0 && (
                <motion.div
                  initial={
                    enableAnimations
                      ? { opacity: 0, x: -12, scale: 0.96 }
                      : false
                  }
                  animate={
                    enableAnimations
                      ? { opacity: 1, x: 0, scale: 1 }
                      : undefined
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
                  id={READER_TOC_PANEL_ID}
                  role="region"
                  aria-labelledby={READER_TOC_TITLE_ID}
                  className={`absolute left-full top-0 ml-2 w-64 max-h-[50vh] overflow-y-auto rounded-2xl border ${currentTheme.border} ${currentTheme.surface} p-3`}
                >
                  <div
                    id={READER_TOC_TITLE_ID}
                    className={`text-xs font-medium ${currentTheme.secondary} mb-2 px-2`}
                  >
                    {t.brew.tocTitle} ({toc.length})
                  </div>
                  <nav className="space-y-0.5">
                    {(() => {
                      const minLevel =
                        toc.length > 0
                          ? Math.min(...toc.map((t) => t.level))
                          : 1
                      return toc.map((item) => {
                        const isActive = item.id === activeHeadingId
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

            <AnimatePresence>
              {showBrewliaPanel && (
                <motion.div
                  initial={
                    enableAnimations
                      ? { opacity: 0, x: -12, scale: 0.96 }
                      : false
                  }
                  animate={
                    enableAnimations
                      ? { opacity: 1, x: 0, scale: 1 }
                      : undefined
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
                  id={READER_ANNOTATIONS_PANEL_ID}
                  role="region"
                  aria-labelledby={READER_ANNOTATIONS_TITLE_ID}
                  className={`absolute left-full ${showToc && toc.length > 0 ? 'top-[calc(100%+0.5rem)]' : 'top-0'} ml-2 w-72 overflow-hidden rounded-2xl border ${currentTheme.border} ${currentTheme.surface} flex flex-col`}
                  style={STYLE_MAX_HEIGHT_320} /* ~4 条注释高 */
                >
                  <div
                    className={`flex items-center justify-between px-3 py-2.5 border-b ${currentTheme.border} shrink-0`}
                  >
                    <div className="flex items-center gap-2">
                      <Sparkles className="w-4 h-4 text-purple-500" />
                      <span
                        id={READER_ANNOTATIONS_TITLE_ID}
                        className={`text-sm font-medium ${currentTheme.text}`}
                      >
                        {t.brew.aiAnnotations}{' '}
                        {annotations.length > 0 && `(${annotations.length})`}
                      </span>
                    </div>
                    <div className="flex items-center gap-1">
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
                          const typeConfig = annotationChrome(annotation.type)
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

            <AnimatePresence>
              {showPodcastPlayer && podcastDialogues.length > 0 && (
                <motion.div
                  initial={
                    enableAnimations
                      ? { opacity: 0, x: -12, scale: 0.96 }
                      : false
                  }
                  animate={
                    enableAnimations
                      ? { opacity: 1, x: 0, scale: 1 }
                      : undefined
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
                  id={READER_PODCAST_PANEL_ID}
                  role="region"
                  aria-labelledby={READER_PODCAST_TITLE_ID}
                  className={`absolute left-full ${showBrewliaPanel || (showToc && toc.length > 0) ? 'top-[calc(100%+0.5rem)]' : 'top-0'} ml-2 w-80 overflow-hidden rounded-2xl border ${currentTheme.border} ${currentTheme.surface} flex flex-col`}
                  style={STYLE_MAX_HEIGHT_320}
                >
                  <div
                    className={`flex items-center justify-between px-3 py-2.5 border-b ${currentTheme.border} shrink-0`}
                  >
                    <div className="flex items-center gap-2">
                      <Mic className="w-4 h-4 text-emerald-500" />
                      <span
                        id={READER_PODCAST_TITLE_ID}
                        className={`text-sm font-medium ${currentTheme.text}`}
                      >
                        {t.brew.aiPodcast}
                      </span>
                      <span className={`text-xs ${currentTheme.secondary}`}>
                        {podcastCurrentIndex + 1}/{podcastDialogues.length}
                      </span>
                    </div>
                    <div className="flex items-center gap-1">
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
                          {/* 加载环由下方状态行独担，按钮不再转圈。 */}
                          <Cloud className="w-3.5 h-3.5" />
                        </button>
                      </div>
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

                  {cloudTtsLoading && (
                    <div
                      className={`px-3 py-2 text-xs ${currentTheme.secondary} bg-emerald-500/5 flex items-center gap-2 shrink-0`}
                    >
                      <Spinner size="xs" color="current" />
                      <span>
                        {t.brew.loadingCloudVoice} {cloudTtsLoadProgress.loaded}
                        /{cloudTtsLoadProgress.total}
                      </span>
                    </div>
                  )}

                  {showVoiceSettings ? (
                    <div className="px-3 py-2 flex-1 overflow-y-auto">
                      <div className="space-y-2">
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

                        {ttsEngine === 'cloud' &&
                          cloudTtsAvailable &&
                          voiceList.length > 0 && (
                            <>
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
                                      title={`${voiceTip(voice)}${t.brew.voiceSuperNaturalSuffix}`}
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
                                      title={`${voiceTip(voice)}${voice.emotion_support ? t.brew.voiceEmotionalSuffix : ''}`}
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
                                      title={voiceTip(voice)}
                                    >
                                      {voice.name}
                                    </button>
                                  ))}
                                </div>
                              </div>

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
                                    onClick={() =>
                                      handleVoiceSelect('guest', 0)
                                    }
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
                                      title={`${voiceTip(voice)}${t.brew.voiceSuperNaturalSuffix}`}
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
                                      title={`${voiceTip(voice)}${voice.emotion_support ? t.brew.voiceEmotionalSuffix : ''}`}
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
                                      title={voiceTip(voice)}
                                    >
                                      {voice.name}
                                    </button>
                                  ))}
                                </div>
                              </div>
                            </>
                          )}

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
                                      title={t.brew.clearVoiceCache}
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
                                  {t.brew.loadingCloudCache}
                                </div>
                              )}
                            </div>
                          )}
                      </div>
                    </div>
                  ) : (
                    <>
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

                      <div
                        className={`flex items-center justify-center gap-3 px-3 py-2.5 border-t ${currentTheme.border} shrink-0`}
                      >
                        <button
                          onClick={handlePrevious}
                          disabled={podcastCurrentIndex <= 0}
                          className={`p-1.5 rounded-lg transition-colors ${currentTheme.secondary} hover:${currentTheme.text} disabled:opacity-30`}
                          title={t.brew.prevSegment}
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
          </div>
        </motion.aside>
      </>
    )
  },
)
