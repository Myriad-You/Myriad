/**
 * 标题样式选择器组件
 * 用于在编辑模式下选择信息条标题的装饰字体、大小和颜色
 *
 * 性能优化：
 * - useCallback 缓存事件处理函数
 * - useMemo 缓存计算结果
 * - 懒加载字体预览
 */

import type { FontOption } from '../hooks/useTitleFont'
import { FaCheck, FaPalette } from '@lib/icons'
import { AnimatePresenceShim as AnimatePresence, motionShim as motion } from '@lib/motionShim'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../contexts/I18nContext'
import {
  AVAILABLE_COLORS,
  AVAILABLE_FONTS,
  FONT_SIZE_OPTIONS,

  useTitleFont,
} from '../hooks/useTitleFont'

interface TitleFontSelectorProps {
  csrfToken: string
  className?: string
}

type TabType = 'font' | 'size' | 'color'

export const TitleFontSelector: React.FC<TitleFontSelectorProps> = React.memo(({
  csrfToken,
  className = '',
}) => {
  const { t } = useI18n()

  // 标签配置 - 使用 i18n
  const TABS: { id: TabType, label: string }[] = [
    { id: 'font', label: t.titleStyle.tabFont },
    { id: 'size', label: t.titleStyle.tabSize },
    { id: 'color', label: t.titleStyle.tabColor },
  ]

  const {
    titleFont,
    titleFontSize,
    titleColor,
    setTitleFont,
    setTitleFontSize,
    setTitleColor,
    isLoading,
    preloadAllFonts,
  } = useTitleFont()

  const [isOpen, setIsOpen] = useState(false)
  const [activeTab, setActiveTab] = useState<TabType>('font')
  const [panelPosition, setPanelPosition] = useState({ top: 0, left: 0 })
  const [isDark, setIsDark] = useState(false)

  const buttonRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)

  // 检测深色模式
  useEffect(() => {
    const checkDarkMode = () => {
      setIsDark(document.documentElement.classList.contains('dark'))
    }
    checkDarkMode()

    const observer = new MutationObserver(checkDarkMode)
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['class'],
    })

    return () => observer.disconnect()
  }, [])

  // 计算面板位置
  useEffect(() => {
    if (isOpen && buttonRef.current) {
      const rect = buttonRef.current.getBoundingClientRect()
      setPanelPosition({
        top: rect.bottom + 8,
        left: Math.max(8, rect.left), // 确保不会超出左边界
      })
    }
  }, [isOpen])

  // 点击外部关闭
  useEffect(() => {
    if (!isOpen)
      return

    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node
      if (
        buttonRef.current && !buttonRef.current.contains(target)
        && panelRef.current && !panelRef.current.contains(target)
      ) {
        setIsOpen(false)
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [isOpen])

  // 打开选择器
  const handleOpen = useCallback(async () => {
    const willOpen = !isOpen
    setIsOpen(willOpen)
    if (willOpen) {
      preloadAllFonts()
    }
  }, [isOpen, preloadAllFonts])

  // 选择字体
  const handleSelectFont = useCallback((font: FontOption) => {
    setTitleFont(font.id, csrfToken)
  }, [setTitleFont, csrfToken])

  // 选择字体大小
  const handleSelectSize = useCallback((size: number) => {
    setTitleFontSize(size, csrfToken)
  }, [setTitleFontSize, csrfToken])

  // 选择颜色
  const handleSelectColor = useCallback((colorId: string) => {
    setTitleColor(colorId, csrfToken)
  }, [setTitleColor, csrfToken])

  // 切换标签
  const handleTabChange = useCallback((tab: TabType) => {
    setActiveTab(tab)
  }, [])

  // 渲染标签按钮
  const renderTabs = useMemo(() => (
    <div className="flex border-b border-gray-200 dark:border-white/10">
      {TABS.map(tab => (
        <button
          key={tab.id}
          onClick={() => handleTabChange(tab.id)}
          className={`flex-1 px-3 py-2 text-xs font-medium transition-colors ${
            activeTab === tab.id
              ? 'text-gray-800 dark:text-gray-200 border-b-2'
              : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
          }`}
          style={{
            borderColor: activeTab === tab.id ? 'var(--color-primary)' : 'transparent',
          }}
        >
          {tab.label}
        </button>
      ))}
    </div>
  ), [activeTab, handleTabChange])

  // 渲染字体列表
  const renderFontList = useMemo(() => (
    <div className="space-y-1 max-h-56 overflow-y-auto">
      {AVAILABLE_FONTS.map(font => (
        <button
          key={font.id}
          onClick={() => handleSelectFont(font)}
          disabled={isLoading}
          className={`
            w-full px-3 py-2 rounded-lg text-left transition-colors flex items-center justify-between
            ${titleFont === font.id ? 'bg-black/10 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/5'}
            ${isLoading ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}
          `}
        >
          <span
            className="text-base text-gray-800 dark:text-gray-200"
            style={{ fontFamily: font.family }}
          >
            {font.name}
          </span>
          {titleFont === font.id && (
            <FaCheck className="w-3.5 h-3.5 flex-shrink-0" style={{ color: 'var(--color-primary)' }} />
          )}
        </button>
      ))}
    </div>
  ), [titleFont, isLoading, handleSelectFont])

  // 渲染大小列表
  const renderSizeList = useMemo(() => (
    <div className="space-y-1">
      {FONT_SIZE_OPTIONS.map(size => (
        <button
          key={size.id}
          onClick={() => handleSelectSize(size.value)}
          className={`
            w-full px-3 py-2 rounded-lg text-left transition-colors flex items-center justify-between
            ${titleFontSize === size.value ? 'bg-black/10 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/5'}
          `}
        >
          <div className="flex items-center gap-3">
            <span
              className="text-gray-800 dark:text-gray-200 w-8"
              style={{ fontSize: `${14 * size.value}px` }}
            >
              Aa
            </span>
            <span className="text-sm text-gray-600 dark:text-gray-400">
              {t.titleStyle[size.nameKey as keyof typeof t.titleStyle]}
            </span>
          </div>
          {titleFontSize === size.value && (
            <FaCheck className="w-3.5 h-3.5 flex-shrink-0" style={{ color: 'var(--color-primary)' }} />
          )}
        </button>
      ))}
    </div>
  ), [titleFontSize, handleSelectSize, t])

  // 渲染颜色列表
  const renderColorList = useMemo(() => (
    <div className="space-y-1">
      {AVAILABLE_COLORS.map(color => (
        <button
          key={color.id}
          onClick={() => handleSelectColor(color.id)}
          className={`
            w-full px-3 py-2 rounded-lg text-left transition-colors flex items-center justify-between
            ${titleColor === color.id ? 'bg-black/10 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/5'}
          `}
        >
          <div className="flex items-center gap-3">
            <div
              className="w-5 h-5 rounded-full border border-gray-300 dark:border-gray-600"
              style={{
                background: color.id === 'adaptive'
                  ? `linear-gradient(135deg, ${isDark ? '#fff' : '#000'} 0%, var(--color-primary) 100%)`
                  : color.value,
              }}
            />
            <span className="text-sm text-gray-600 dark:text-gray-400">
              {t.titleStyle[color.nameKey as keyof typeof t.titleStyle]}
            </span>
          </div>
          {titleColor === color.id && (
            <FaCheck className="w-3.5 h-3.5 flex-shrink-0" style={{ color: 'var(--color-primary)' }} />
          )}
        </button>
      ))}
    </div>
  ), [titleColor, isDark, handleSelectColor, t])

  // 渲染当前标签内容
  const renderTabContent = useMemo(() => {
    switch (activeTab) {
      case 'font': return renderFontList
      case 'size': return renderSizeList
      case 'color': return renderColorList
    }
  }, [activeTab, renderFontList, renderSizeList, renderColorList])

  return (
    <div className={className}>
      {/* 样式选择按钮 */}
      <button
        ref={buttonRef}
        onClick={handleOpen}
        className="px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-colors bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10"
        style={{ color: 'var(--color-primary)' }}
        title={t.titleStyle.title}
      >
        <FaPalette className="w-3 h-3" />
        {t.titleStyle.style}
      </button>

      {/* 样式选择面板 - Portal 渲染 */}
      {createPortal(
        <AnimatePresence>
          {isOpen && (
            <motion.div
              ref={panelRef}
              initial={{ opacity: 0, y: -10, scale: 0.95 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: -10, scale: 0.95 }}
              transition={{ duration: 0.15 }}
              className="fixed w-72 glass rounded-xl shadow-lg overflow-hidden"
              style={{
                zIndex: 99999,
                top: panelPosition.top,
                left: panelPosition.left,
              }}
            >
              {renderTabs}
              <div className="p-2">
                {renderTabContent}
              </div>
            </motion.div>
          )}
        </AnimatePresence>,
        document.body,
      )}
    </div>
  )
})

TitleFontSelector.displayName = 'TitleFontSelector'

export default TitleFontSelector
