/**
 * 阅读器右侧控制栏组件
 * 包含: 评论、主题、字体、布局、字号、行高
 */

import type { MouseEvent } from 'react'

import type { CommentItem } from '../../../services/brewApi'
import type { ReaderRightPanelProps } from './types'
import {
  LuAlignJustify as AlignJustify,
  LuMessageSquare as MessageSquare,
  LuMinus as Minus,
  LuPalette as Palette,
  LuPlus as Plus,
} from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import { memo } from 'react'
import { THEMES } from './constants'

interface ExtendedReaderRightPanelProps extends ReaderRightPanelProps {
  // 评论
  isAuthenticated: boolean
  hasComments: boolean
  comments: CommentItem[]
  showCommentsPanel: boolean
  setShowCommentsPanel: (show: boolean) => void
}

export default memo(
  ({
    theme,
    currentTheme,
    isDark,
    showPanels,
    cycleTheme,
    cycleFont,
    cycleLayout,
    fontSize,
    adjustFontSize,
    lineHeight,
    adjustLineHeight,
    currentFont,
    currentLayout,
    enableAnimations,
    sideButtonClass,
    onMouseEnter,
    onMouseLeave,
    t,
    // Extended props
    isAuthenticated,
    hasComments,
    comments,
    showCommentsPanel,
    setShowCommentsPanel,
  }: ExtendedReaderRightPanelProps) => {
    const layout = currentLayout.id

    return (
      <>
        <motion.aside
          initial={{ opacity: 0, x: 24, scale: 0.92 }}
          animate={
            enableAnimations
              ? showPanels
                ? { opacity: 1, x: 0, scale: 1 }
                : { opacity: 0, x: 24, scale: 0.92 }
              : { opacity: showPanels ? 1 : 0 }
          }
          transition={
            enableAnimations
              ? { duration: 0.3, ease: [0.16, 1, 0.3, 1] }
              : { duration: 0 }
          }
          className="hidden sm:block sticky top-1/3 -translate-y-1/3 h-fit ml-4 z-20"
          style={{
            pointerEvents: showPanels ? 'auto' : 'none',
            willChange: 'transform, opacity',
          }}
          onClick={(e: MouseEvent) => e.stopPropagation()}
          onMouseEnter={onMouseEnter}
          onMouseLeave={onMouseLeave}
        >
          <div
            className={`flex flex-col items-center gap-2 p-2 rounded-2xl border ${currentTheme.border} ${currentTheme.surface}`}
          >
            {/* 用户评论指示器 - 仅登录用户可见 */}
            {isAuthenticated && (
              <>
                <button
                  onClick={() => setShowCommentsPanel(!showCommentsPanel)}
                  className={`${sideButtonClass} relative`}
                  title={
                    hasComments
                      ? `${t.brew.viewComments} (${comments.length})`
                      : t.brew.selectTextToComment
                  }
                >
                  <MessageSquare className="w-5 h-5" />
                  {hasComments && (
                    <span className="absolute -top-1 -right-1 w-4 h-4 bg-amber-500 text-white text-[10px] font-bold rounded-full flex items-center justify-center">
                      {comments.length > 9 ? '9+' : comments.length}
                    </span>
                  )}
                </button>
                <div
                  className={`w-6 h-px ${isDark ? 'bg-white/10' : 'bg-black/10'}`}
                />
              </>
            )}

            {/* 主题切换 */}
            <button
              onClick={cycleTheme}
              className={sideButtonClass}
              title={`${t.brew.switchTheme} (${THEMES[theme].icon})`}
            >
              <Palette className="w-5 h-5" />
            </button>

            {/* 字体切换 */}
            <button
              onClick={cycleFont}
              className={`${sideButtonClass} text-xs font-bold w-10 h-10 flex items-center justify-center`}
              title={
                (t.brew as Record<string, string>)[currentFont.labelKey] ||
                currentFont.labelKey
              }
              style={{ fontFamily: currentFont.family }}
            >
              {t.brew.fontLabel}
            </button>

            {/* 布局宽度切换 */}
            <button
              onClick={cycleLayout}
              className={sideButtonClass}
              title={
                (t.brew as Record<string, string>)[currentLayout.labelKey] ||
                currentLayout.labelKey
              }
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
                {layout === 'narrow' ? (
                  // 窄版图标 - 居中矩形
                  <>
                    <rect x="6" y="4" width="12" height="16" rx="1" />
                  </>
                ) : (
                  // 宽版图标 - 更宽的矩形
                  <>
                    <rect x="3" y="4" width="18" height="16" rx="1" />
                  </>
                )}
              </svg>
            </button>

            {/* 分隔线 */}
            <div
              className={`w-6 h-px ${isDark ? 'bg-white/10' : 'bg-black/10'}`}
            />

            {/* 字号调整 */}
            <div
              className={`flex flex-col items-center gap-1 p-1 rounded-xl ${isDark ? 'bg-white/5' : 'bg-black/5'}`}
            >
              <button
                onClick={() => adjustFontSize(1)}
                className={`p-1.5 rounded-lg ${currentTheme.secondary} hover:${currentTheme.text} transition-colors`}
                title={t.brew.increaseFontSize}
              >
                <Plus className="w-4 h-4" />
              </button>
              <span
                className={`text-[10px] ${currentTheme.secondary} tabular-nums`}
              >
                {fontSize}
              </span>
              <button
                onClick={() => adjustFontSize(-1)}
                className={`p-1.5 rounded-lg ${currentTheme.secondary} hover:${currentTheme.text} transition-colors`}
                title={t.brew.decreaseFontSize}
              >
                <Minus className="w-4 h-4" />
              </button>
            </div>

            {/* 行高调整 */}
            <div
              className={`flex flex-col items-center gap-1 p-1 rounded-xl ${isDark ? 'bg-white/5' : 'bg-black/5'}`}
            >
              <button
                onClick={() => adjustLineHeight(0.1)}
                className={`p-1.5 rounded-lg ${currentTheme.secondary} hover:${currentTheme.text} transition-colors`}
                title={t.brew.increaseLineHeight}
              >
                <AlignJustify className="w-4 h-4" />
              </button>
              <span
                className={`text-[10px] ${currentTheme.secondary} tabular-nums`}
              >
                {lineHeight.toFixed(1)}
              </span>
              <button
                onClick={() => adjustLineHeight(-0.1)}
                className={`p-1.5 rounded-lg ${currentTheme.secondary} hover:${currentTheme.text} transition-colors`}
                title={t.brew.decreaseLineHeight}
              >
                <AlignJustify className="w-4 h-4 opacity-50" />
              </button>
            </div>
          </div>
        </motion.aside>
      </>
    )
  },
)
