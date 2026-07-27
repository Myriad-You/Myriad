/**
 * 移动端阅读器底栏组件
 * 在小屏设备上替代左右两侧的控制栏
 */

import type { MobileReaderBarProps } from './types'

import {
  LuAlignJustify as AlignJustify,
  LuArrowRight as ArrowRight,
  LuChevronLeft as ChevronLeft,
  LuChevronRight as ChevronRight,
  LuChevronUp as ChevronUp,
  LuCloud as Cloud,
  LuExternalLink as ExternalLink,
  LuEye as Eye,
  LuEyeOff as EyeOff,
  LuList as List,
  LuMessageSquare as MessageSquare,
  LuMic as Mic,
  LuMinus as Minus,
  LuMonitor as Monitor,
  LuPalette as Palette,
  LuPause as Pause,
  LuPlay as Play,
  LuPlus as Plus,
  LuRefreshCw as RefreshCw,
  LuSkipBack as SkipBack,
  LuSkipForward as SkipForward,
  LuSparkles as Sparkles,
  LuSquare as Square,
  LuStar as Star,
  LuType as Type,
  LuX as X,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useState } from 'react'
import * as brewliaApi from '../../../services/brewliaApi'

import { Spinner } from '../../Spinner'
import { STYLE_MAX_HEIGHT_60VH, THEMES } from './constants'

export const MobileReaderBar = memo(
  ({
    item,
    onClose,
    onToggleStar,
    isAuthenticated,
    isAdmin,
    isBrewlia,
    theme,
    currentTheme,
    isDark,
    readingProgress,
    showPanels,
    showMobileControls,
    setShowMobileControls,
    toc,
    activeHeadingId,
    scrollToHeading,
    comments,
    hasComments,
    showCommentsPanel,
    setShowCommentsPanel,
    annotations,
    annotationsLoading,
    showAnnotations,
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
    loadPodcast,
    podcastCurrentIndex,
    ttsEngine,
    handleTtsEngineChange,
    cloudTtsAvailable,
    cloudTtsLoadProgress,
    handlePlayPause,
    handleStop,
    handlePrevious,
    handleNext,
    handleDialogueClick,
    cycleTheme,
    cycleFont,
    fontSize,
    adjustFontSize,
    lineHeight,
    adjustLineHeight,
    currentFont,
    handleShare,
    enableAnimations,
    onTouchStart,
    onTouchEnd,
    t,
  }: MobileReaderBarProps) => {
    // 当前激活的全屏面板
    const [activePanel, setActivePanel] = useState<
      'toc' | 'annotations' | 'podcast' | 'comments' | null
    >(null)

    // 打开面板
    const openPanel = (
      panel: 'toc' | 'annotations' | 'podcast' | 'comments',
    ) => {
      setActivePanel(panel)
      setShowMobileControls(false)
    }

    // 关闭面板
    const closePanel = () => {
      setActivePanel(null)
    }

    // 计算最小目录层级
    const minTocLevel =
      toc.length > 0 ? Math.min(...toc.map((t) => t.level)) : 1

    return (
      <>
        {/* 移动端底栏控制条 */}
        <div
          className="sm:hidden fixed bottom-0 left-0 right-0 z-30"
          onTouchStart={onTouchStart}
          onTouchEnd={onTouchEnd}
        >
          <AnimatePresence>
            {showPanels && (
              <motion.div
                initial={enableAnimations ? { opacity: 0, y: 24 } : false}
                animate={enableAnimations ? { opacity: 1, y: 0 } : undefined}
                exit={enableAnimations ? { opacity: 0, y: 24 } : undefined}
                transition={
                  enableAnimations
                    ? { duration: 0.3, ease: [0.16, 1, 0.3, 1] }
                    : undefined
                }
                className={`flex flex-col-reverse backdrop-blur-md border-t ${currentTheme.border} ${currentTheme.surface}`}
              >
                {/* 主控制栏 - 始终显示（在底部） */}
                <div className="flex items-center justify-between px-3 py-2 safe-area-inset-bottom">
                  {/* 左侧：返回 + 来源 */}
                  <div className="flex items-center gap-2">
                    <button
                      onClick={onClose}
                      className={`p-2 rounded-xl ${currentTheme.secondary} hover:${currentTheme.text} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`}
                      title={t.brew.backEsc}
                    >
                      <ChevronLeft className="w-5 h-5" />
                    </button>
                    {item.source_icon && (
                      <img
                        src={item.source_icon || undefined}
                        alt=""
                        className="w-6 h-6 rounded-lg"
                      />
                    )}
                    <span
                      className={`text-sm font-medium ${currentTheme.text} truncate max-w-25`}
                    >
                      {item.source_name}
                    </span>
                  </div>

                  {/* 右侧：进度 + 展开按钮 */}
                  <div className="flex items-center gap-2">
                    <span
                      className={`text-xs font-medium ${currentTheme.secondary} tabular-nums`}
                    >
                      {readingProgress}%
                    </span>
                    <button
                      onClick={() => setShowMobileControls(!showMobileControls)}
                      className={`p-2 rounded-xl ${showMobileControls ? currentTheme.text : currentTheme.secondary} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`}
                      title={t.brew.settings || '更多选项'}
                    >
                      <ChevronUp
                        className={`w-5 h-5 transition-transform ${showMobileControls ? 'rotate-180' : ''}`}
                      />
                    </button>
                  </div>
                </div>

                {/* 展开的控制面板（在主控制栏上方） */}
                <AnimatePresence>
                  {showMobileControls && (
                    <motion.div
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: 'auto', opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={{ duration: 0.25, ease: [0.16, 1, 0.3, 1] }}
                      className={`overflow-hidden border-b ${currentTheme.border}`}
                    >
                      <div className="px-3 py-3 space-y-3">
                        {/* 第一行：导航与功能按钮 */}
                        <div className="flex items-center justify-between">
                          <div className="flex items-center gap-1">
                            {/* 目录 */}
                            {toc.length > 0 && (
                              <button
                                onClick={() => openPanel('toc')}
                                className={`p-2 rounded-xl ${activePanel === 'toc' ? (isDark ? 'bg-white/10' : 'bg-black/5') : ''} ${currentTheme.secondary} hover:${currentTheme.text}`}
                                title={t.brew.tableOfContents}
                              >
                                <List className="w-5 h-5" />
                              </button>
                            )}

                            {/* 收藏 */}
                            {isAuthenticated && (
                              <button
                                onClick={onToggleStar}
                                className={`p-2 rounded-xl ${item.is_starred ? 'text-amber-500 bg-amber-500/10' : `${currentTheme.secondary} hover:text-amber-500`}`}
                                title={
                                  item.is_starred
                                    ? t.brew.unstar
                                    : t.brew.starred
                                }
                              >
                                <Star
                                  className={`w-5 h-5 ${item.is_starred ? 'fill-current' : ''}`}
                                />
                              </button>
                            )}

                            {/* 评论 */}
                            {isAuthenticated && (
                              <button
                                onClick={() =>
                                  setShowCommentsPanel(!showCommentsPanel)
                                }
                                className={`p-2 rounded-xl relative ${currentTheme.secondary} hover:${currentTheme.text}`}
                                title={
                                  hasComments
                                    ? `${t.brew.viewComments} (${comments.length})`
                                    : t.brew.selectTextToComment
                                }
                              >
                                <MessageSquare className="w-5 h-5" />
                                {hasComments && (
                                  <span className="absolute -top-0.5 -right-0.5 w-4 h-4 bg-amber-500 text-white text-[10px] font-bold rounded-full flex items-center justify-center">
                                    {comments.length > 9
                                      ? '9+'
                                      : comments.length}
                                  </span>
                                )}
                              </button>
                            )}

                            {/* AI 注释 */}
                            {isBrewlia &&
                              (isAdmin || item.has_ai_annotations) && (
                                <button
                                  onClick={() => openPanel('annotations')}
                                  disabled={annotationsLoading}
                                  className={`p-2 rounded-xl ${
                                    activePanel === 'annotations' ||
                                    (showAnnotations && annotations.length > 0)
                                      ? 'text-purple-500 bg-purple-500/10'
                                      : `${currentTheme.secondary} hover:text-purple-500`
                                  }`}
                                  title={
                                    annotationsLoading
                                      ? `${t.brew.loading}...`
                                      : t.brew.aiAnnotations
                                  }
                                >
                                  {annotationsLoading ? (
                                    <Spinner size="sm" color="current" />
                                  ) : (
                                    <Sparkles className="w-5 h-5" />
                                  )}
                                </button>
                              )}

                            {/* AI 播客 */}
                            {isBrewlia && (isAdmin || item.has_ai_podcast) && (
                              <button
                                onClick={() => {
                                  if (podcastDialogues.length === 0) {
                                    loadPodcast()
                                  }
                                  openPanel('podcast')
                                }}
                                disabled={podcastLoading || cloudTtsLoading}
                                className={`p-2 rounded-xl ${
                                  podcastState === 'playing'
                                    ? 'text-emerald-500 bg-emerald-500/10 animate-pulse'
                                    : activePanel === 'podcast'
                                      ? 'text-emerald-500 bg-emerald-500/10'
                                      : `${currentTheme.secondary} hover:text-emerald-500`
                                }`}
                                title={
                                  podcastLoading
                                    ? t.brew.generatingPodcast
                                    : t.brew.aiPodcast
                                }
                              >
                                {podcastLoading || cloudTtsLoading ? (
                                  <Spinner size="sm" color="current" />
                                ) : (
                                  <Mic className="w-5 h-5" />
                                )}
                              </button>
                            )}

                            {/* 外部链接 */}
                            <a
                              href={item.link}
                              target="_blank"
                              rel="noopener noreferrer"
                              className={`p-2 rounded-xl ${currentTheme.secondary} hover:${currentTheme.text}`}
                              title={t.brew.readOriginal}
                            >
                              <ExternalLink className="w-5 h-5" />
                            </a>
                          </div>

                          {/* 分享 */}
                          <button
                            onClick={handleShare}
                            className={`p-2 rounded-xl ${currentTheme.secondary} hover:${currentTheme.text}`}
                            title={t.brew.share || '分享'}
                          >
                            <svg
                              className="w-5 h-5"
                              viewBox="0 0 24 24"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth="2"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            >
                              <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8" />
                              <polyline points="16 6 12 2 8 6" />
                              <line x1="12" x2="12" y1="2" y2="15" />
                            </svg>
                          </button>
                        </div>

                        {/* 第二行：阅读设置 */}
                        <div className="flex items-center justify-between">
                          {/* 主题 */}
                          <button
                            onClick={cycleTheme}
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/5'} ${currentTheme.secondary}`}
                            title={t.brew.switchTheme}
                          >
                            <Palette className="w-4 h-4" />
                            <span className="text-xs">
                              {THEMES[theme].icon}
                            </span>
                          </button>

                          {/* 字体 */}
                          <button
                            onClick={cycleFont}
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/5'} ${currentTheme.secondary}`}
                            style={{ fontFamily: currentFont.family }}
                            title={
                              (t.brew as Record<string, string>)[
                                currentFont.labelKey
                              ]
                            }
                          >
                            <Type className="w-4 h-4" />
                            <span className="text-xs">{t.brew.fontLabel}</span>
                          </button>

                          {/* 字号调整 */}
                          <div
                            className={`flex items-center gap-1 px-2 py-1 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/5'}`}
                          >
                            <button
                              onClick={() => adjustFontSize(-1)}
                              className={`p-1 rounded ${currentTheme.secondary} hover:${currentTheme.text}`}
                              title={t.brew.decreaseFontSize}
                            >
                              <Minus className="w-4 h-4" />
                            </button>
                            <span
                              className={`text-xs ${currentTheme.secondary} tabular-nums min-w-6 text-center`}
                            >
                              {fontSize}
                            </span>
                            <button
                              onClick={() => adjustFontSize(1)}
                              className={`p-1 rounded ${currentTheme.secondary} hover:${currentTheme.text}`}
                              title={t.brew.increaseFontSize}
                            >
                              <Plus className="w-4 h-4" />
                            </button>
                          </div>

                          {/* 行高调整 */}
                          <div
                            className={`flex items-center gap-1 px-2 py-1 rounded-lg ${isDark ? 'bg-white/5' : 'bg-black/5'}`}
                          >
                            <button
                              onClick={() => adjustLineHeight(-0.1)}
                              className={`p-1 rounded ${currentTheme.secondary} hover:${currentTheme.text}`}
                              title={t.brew.decreaseLineHeight}
                            >
                              <AlignJustify className="w-4 h-4 opacity-50" />
                            </button>
                            <span
                              className={`text-xs ${currentTheme.secondary} tabular-nums min-w-7 text-center`}
                            >
                              {lineHeight.toFixed(1)}
                            </span>
                            <button
                              onClick={() => adjustLineHeight(0.1)}
                              className={`p-1 rounded ${currentTheme.secondary} hover:${currentTheme.text}`}
                              title={t.brew.increaseLineHeight}
                            >
                              <AlignJustify className="w-4 h-4" />
                            </button>
                          </div>
                        </div>
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        {/* 移动端全屏面板 */}
        <AnimatePresence>
          {activePanel && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.2, ease: 'easeOut' }}
              className="sm:hidden fixed inset-0 z-40 flex flex-col"
            >
              {/* 背景遮罩 */}
              <div
                className="absolute inset-0 bg-black/50 backdrop-blur-sm"
                onClick={closePanel}
              />

              {/* 面板内容 - 从底部滑出 */}
              <motion.div
                initial={{ y: '100%' }}
                animate={{ y: 0 }}
                exit={{ y: '100%' }}
                transition={{ duration: 0.35, ease: [0.16, 1, 0.3, 1] }}
                className={`absolute bottom-0 left-0 right-0 rounded-t-2xl ${currentTheme.surfaceSolid} border-t ${currentTheme.border} overflow-hidden`}
                style={STYLE_MAX_HEIGHT_60VH}
              >
                {/* 面板头部 */}
                <div
                  className={`flex items-center justify-between px-4 py-3 border-b ${currentTheme.border}`}
                >
                  <div className="flex items-center gap-2">
                    {activePanel === 'toc' && (
                      <>
                        <List className="w-5 h-5 text-amber-500" />
                        <span className={`font-medium ${currentTheme.text}`}>
                          {t.brew.tocTitle}
                        </span>
                        <span className={`text-sm ${currentTheme.secondary}`}>
                          ({toc.length})
                        </span>
                      </>
                    )}
                    {activePanel === 'annotations' && (
                      <>
                        <Sparkles className="w-5 h-5 text-purple-500" />
                        <span className={`font-medium ${currentTheme.text}`}>
                          {t.brew.aiAnnotations}
                        </span>
                        {annotations.length > 0 && (
                          <span className={`text-sm ${currentTheme.secondary}`}>
                            ({annotations.length})
                          </span>
                        )}
                      </>
                    )}
                    {activePanel === 'podcast' && (
                      <>
                        <Mic className="w-5 h-5 text-emerald-500" />
                        <span className={`font-medium ${currentTheme.text}`}>
                          {t.brew.aiPodcast}
                        </span>
                        {podcastDialogues.length > 0 && (
                          <span className={`text-sm ${currentTheme.secondary}`}>
                            {podcastCurrentIndex + 1}/{podcastDialogues.length}
                          </span>
                        )}
                      </>
                    )}
                  </div>
                  <button
                    onClick={closePanel}
                    className={`p-2 rounded-xl ${currentTheme.secondary} hover:${currentTheme.text} ${isDark ? 'hover:bg-white/10' : 'hover:bg-black/5'}`}
                    title={t.brew.close || '关闭'}
                  >
                    <X className="w-5 h-5" />
                  </button>
                </div>

                {/* 面板内容区 */}
                <div
                  className="overflow-y-auto"
                  style={{ maxHeight: 'calc(60vh - 60px)' }}
                >
                  {/* 目录面板 */}
                  {activePanel === 'toc' && (
                    <nav className="p-3 space-y-0.5">
                      {toc.map((item) => {
                        const isActive = item.id === activeHeadingId
                        const indent = (item.level - minTocLevel) * 12

                        return (
                          <button
                            key={item.id}
                            onClick={() => {
                              scrollToHeading(item.id)
                              closePanel()
                            }}
                            className={`w-full text-left px-3 py-2 rounded-xl text-sm transition-all duration-200 ease-out ${
                              isActive
                                ? `${isDark ? 'bg-white/10' : 'bg-black/5'} ${currentTheme.text} font-medium`
                                : `${currentTheme.secondary} hover:${currentTheme.text} ${isDark ? 'hover:bg-white/5' : 'hover:bg-black/3'}`
                            }`}
                            style={{ paddingLeft: `${12 + indent}px` }}
                          >
                            {isActive && (
                              <ChevronRight className="w-3 h-3 inline-block mr-1 -ml-1" />
                            )}
                            {item.text}
                          </button>
                        )
                      })}
                    </nav>
                  )}

                  {/* AI 注释面板 */}
                  {activePanel === 'annotations' && (
                    <div className="p-3">
                      {/* 操作栏 */}
                      <div className="flex items-center justify-between mb-3">
                        <button
                          onClick={toggleAnnotations}
                          className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-all duration-200 ease-out ${
                            showAnnotations
                              ? 'text-purple-500 bg-purple-500/10'
                              : `${currentTheme.secondary} ${isDark ? 'bg-white/5' : 'bg-black/5'}`
                          }`}
                        >
                          {showAnnotations ? (
                            <Eye className="w-4 h-4" />
                          ) : (
                            <EyeOff className="w-4 h-4" />
                          )}
                          {showAnnotations
                            ? t.brew.hideHighlight
                            : t.brew.showAnnotations}
                        </button>
                        {isAdmin && (
                          <button
                            onClick={regenerateAnnotations}
                            disabled={annotationsLoading}
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm ${currentTheme.secondary} ${isDark ? 'bg-white/5' : 'bg-black/5'} disabled:opacity-50`}
                          >
                            {annotationsLoading ? (
                              <Spinner size="sm" color="current" />
                            ) : (
                              <RefreshCw className="w-4 h-4" />
                            )}
                            {t.brew.regenerate}
                          </button>
                        )}
                      </div>

                      {/* 注释列表 */}
                      {annotations.length === 0 ? (
                        <div
                          className={`py-8 text-center ${currentTheme.secondary}`}
                        >
                          {annotationsLoading ? (
                            <div className="flex flex-col items-center gap-2">
                              <Spinner size="lg" className="text-purple-500" />
                              <p className="text-sm">{t.brew.analyzing}</p>
                            </div>
                          ) : (
                            <div className="flex flex-col items-center gap-2">
                              <Sparkles className="w-8 h-8 opacity-30" />
                              <p className="text-sm">{t.brew.noAnnotations}</p>
                              {isAdmin && (
                                <button
                                  onClick={loadAnnotations}
                                  className="text-sm text-purple-500 hover:text-purple-600 font-medium"
                                >
                                  {t.brew.regenerate}
                                </button>
                              )}
                            </div>
                          )}
                        </div>
                      ) : (
                        <div className="space-y-2">
                          {annotations.map((annotation, index) => {
                            const typeConfig =
                              brewliaApi.ANNOTATION_TYPE_CONFIG[
                                annotation.type
                              ] || brewliaApi.ANNOTATION_TYPE_CONFIG.term
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
                                  closePanel()
                                }}
                                className={`w-full text-left p-3 rounded-xl transition-all duration-200 ease-out ${
                                  isSelected
                                    ? `${typeConfig.bgColor} ${currentTheme.text}`
                                    : `${isDark ? 'bg-white/5 hover:bg-white/10' : 'bg-black/2 hover:bg-black/5'}`
                                }`}
                              >
                                <div className="flex items-center gap-2 mb-1 min-w-0">
                                  <span
                                    className={`text-xs px-1.5 py-0.5 rounded ${typeConfig.bgColor} ${typeConfig.color} shrink-0 whitespace-nowrap`}
                                  >
                                    {typeConfig.label}
                                  </span>
                                  <span
                                    className={`text-sm font-medium ${currentTheme.text} min-w-0 flex-1 truncate`}
                                  >
                                    {annotation.term}
                                  </span>
                                  <ArrowRight
                                    className={`w-3 h-3 ${currentTheme.secondary} shrink-0`}
                                  />
                                </div>
                                <p
                                  className={`text-xs ${currentTheme.secondary} leading-relaxed break-words [overflow-wrap:anywhere] ${isSelected ? '' : 'line-clamp-2'}`}
                                >
                                  {annotation.explanation}
                                </p>
                              </button>
                            )
                          })}
                        </div>
                      )}

                      {/* 错误提示 */}
                      {annotationsError && (
                        <div className="mt-3 px-3 py-2 text-sm text-red-500 bg-red-500/10 rounded-lg">
                          {annotationsError}
                        </div>
                      )}
                    </div>
                  )}

                  {/* AI 播客面板 */}
                  {activePanel === 'podcast' && (
                    <div className="p-3">
                      {/* TTS 引擎切换 */}
                      <div className="flex items-center justify-between mb-3">
                        <div className="flex items-center gap-2">
                          <button
                            onClick={() => handleTtsEngineChange('system')}
                            disabled={cloudTtsLoading}
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-colors ${
                              ttsEngine === 'system'
                                ? 'bg-emerald-500/20 text-emerald-500'
                                : `${currentTheme.secondary} ${isDark ? 'bg-white/5' : 'bg-black/5'}`
                            } disabled:opacity-50`}
                          >
                            <Monitor className="w-4 h-4" />
                            {t.brew.systemTts}
                          </button>
                          <button
                            onClick={() => handleTtsEngineChange('cloud')}
                            disabled={cloudTtsLoading}
                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-colors ${
                              ttsEngine === 'cloud'
                                ? 'bg-emerald-500/20 text-emerald-500'
                                : cloudTtsAvailable
                                  ? `${currentTheme.secondary} ${isDark ? 'bg-white/5' : 'bg-black/5'}`
                                  : `${currentTheme.secondary} ${isDark ? 'bg-white/5' : 'bg-black/5'} opacity-60`
                            } disabled:opacity-50`}
                          >
                            {/* 加载环由下方带进度的状态行独担，按钮不再重复转圈 */}
                            <Cloud className="w-4 h-4" />
                            {t.brew.cloudTts}
                          </button>
                        </div>
                      </div>

                      {/* 云端 TTS 加载状态 */}
                      {cloudTtsLoading && (
                        <div
                          className={`mb-3 px-3 py-2 text-sm ${currentTheme.secondary} bg-emerald-500/10 rounded-lg flex items-center gap-2`}
                        >
                          <Spinner size="sm" color="current" />
                          <span>
                            {t.brew.loadingCloudVoice}{' '}
                            {cloudTtsLoadProgress.loaded}/
                            {cloudTtsLoadProgress.total}
                          </span>
                        </div>
                      )}

                      {/* 播放控制 */}
                      {podcastDialogues.length > 0 && (
                        <div
                          className={`flex items-center justify-center gap-4 mb-4 p-3 rounded-xl ${isDark ? 'bg-white/5' : 'bg-black/2'}`}
                        >
                          <button
                            onClick={handlePrevious}
                            disabled={podcastCurrentIndex === 0}
                            className={`p-2 rounded-xl ${currentTheme.secondary} hover:${currentTheme.text} disabled:opacity-30`}
                            title={t.brew.previousDialogue || '上一条'}
                          >
                            <SkipBack className="w-5 h-5" />
                          </button>
                          <button
                            onClick={handlePlayPause}
                            className={`p-4 rounded-full ${podcastState === 'playing' ? 'bg-emerald-500 text-white' : `${isDark ? 'bg-white/10' : 'bg-black/5'} ${currentTheme.text}`}`}
                            title={
                              podcastState === 'playing'
                                ? t.brew.pause
                                : t.brew.play
                            }
                          >
                            {podcastState === 'playing' ? (
                              <Pause className="w-6 h-6" />
                            ) : (
                              <Play className="w-6 h-6" />
                            )}
                          </button>
                          <button
                            onClick={handleNext}
                            disabled={
                              podcastCurrentIndex >= podcastDialogues.length - 1
                            }
                            className={`p-2 rounded-xl ${currentTheme.secondary} hover:${currentTheme.text} disabled:opacity-30`}
                            title={t.brew.nextDialogue || '下一条'}
                          >
                            <SkipForward className="w-5 h-5" />
                          </button>
                          <button
                            onClick={handleStop}
                            className={`p-2 rounded-xl ${currentTheme.secondary} hover:${currentTheme.text}`}
                            title={t.brew.stop || '停止'}
                          >
                            <Square className="w-5 h-5" />
                          </button>
                        </div>
                      )}

                      {/* 对话列表 */}
                      {podcastDialogues.length === 0 ? (
                        <div
                          className={`py-8 text-center ${currentTheme.secondary}`}
                        >
                          {podcastLoading ? (
                            <div
                              className="flex flex-col items-center gap-2"
                              role="status"
                            >
                              <Spinner size="lg" className="text-emerald-500" />
                            </div>
                          ) : (
                            <div className="flex flex-col items-center gap-2">
                              <Mic className="w-8 h-8 opacity-30" />
                              <p className="text-sm">
                                {t.brew.noPodcast || '暂无播客内容'}
                              </p>
                              {isAdmin && (
                                <button
                                  onClick={loadPodcast}
                                  className="text-sm text-emerald-500 hover:text-emerald-600 font-medium"
                                >
                                  {t.brew.generate || '生成播客'}
                                </button>
                              )}
                            </div>
                          )}
                        </div>
                      ) : (
                        <div className="space-y-2">
                          {podcastDialogues.map((dialogue, index) => {
                            const isHost = dialogue.speaker === 'host_a'
                            const isCurrent = index === podcastCurrentIndex
                            const isPlaying =
                              isCurrent && podcastState === 'playing'

                            return (
                              <button
                                key={index}
                                onClick={() => handleDialogueClick(index)}
                                className={`w-full text-left p-3 rounded-xl transition-all duration-200 ease-out ${
                                  isCurrent
                                    ? isPlaying
                                      ? 'bg-emerald-500/20 ring-2 ring-emerald-500/50'
                                      : `${isDark ? 'bg-white/10' : 'bg-black/5'}`
                                    : `${isDark ? 'hover:bg-white/5' : 'hover:bg-black/2'}`
                                }`}
                              >
                                <div className="flex items-start gap-2">
                                  <span
                                    className={`shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-xs font-medium ${
                                      isHost
                                        ? 'bg-blue-500/20 text-blue-500'
                                        : 'bg-pink-500/20 text-pink-500'
                                    }`}
                                  >
                                    {isHost
                                      ? t.brew.podcastHostLabel || '主'
                                      : t.brew.podcastGuestLabel || '嘉'}
                                  </span>
                                  <p
                                    className={`text-sm ${isCurrent ? currentTheme.text : currentTheme.secondary} leading-relaxed`}
                                  >
                                    {dialogue.text}
                                  </p>
                                </div>
                              </button>
                            )
                          })}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              </motion.div>
            </motion.div>
          )}
        </AnimatePresence>
      </>
    )
  },
)
