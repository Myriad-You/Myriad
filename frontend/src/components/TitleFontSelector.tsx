import type { FontOption } from '../hooks/useTitleFont'
import type { WidgetGlowMode, WidgetSurface } from '../hooks/useWidgetTheme'
import type { StylePanelPosition } from './titleFontSelectorPlacement'
import { FaCheck, FaPalette } from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../contexts/I18nContext'
import { useTappThemes } from '../hooks/useTappThemes'
import {
  AVAILABLE_COLORS,
  AVAILABLE_FONTS,
  FONT_SIZE_OPTIONS,
  getTitleColorCss,
  useTitleFont,
} from '../hooks/useTitleFont'
import {
  GLOW_OPTIONS,
  SURFACE_OPTIONS,
  useWidgetTheme,
} from '../hooks/useWidgetTheme'
import { usePrimaryColor } from '../utils/colorSubscriber'
import { useThemeMode } from '../utils/themeSubscriber'
import {
  placeStylePanel,
  STYLE_PANEL_FALLBACK_HEIGHT,
  STYLE_PANEL_WIDTH,
} from './titleFontSelectorPlacement'

interface TitleFontSelectorProps {
  csrfToken: string
  className?: string
  // 自由布局底栏直接用轨道按钮，不要外包一层再 display:contents。
  buttonClassName?: string
  showHeroOptions?: boolean
}

type TabType = 'font' | 'size' | 'color' | 'surface' | 'glow' | 'preset'

export const TitleFontSelector: React.FC<TitleFontSelectorProps> = React.memo(
  ({
      csrfToken,
      className = '',
      buttonClassName,
      showHeroOptions = true,
    }) => {
    const { t } = useI18n()

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

    const { surface, glow, setSurface, setGlowMode } = useWidgetTheme()

    const [isOpen, setIsOpen] = useState(false)

    const { themes: tappThemes } = useTappThemes(isOpen)

    const TABS = useMemo<{ id: TabType; label: string }[]>(() => {
      const tabs: { id: TabType; label: string }[] = []
      if (showHeroOptions) {
        tabs.push(
          { id: 'font', label: t.titleStyle.tabFont },
          { id: 'size', label: t.titleStyle.tabSize },
        )
      }
      tabs.push(
        { id: 'color', label: t.titleStyle.tabColor },
        { id: 'surface', label: t.titleStyle.tabSurface },
        { id: 'glow', label: t.titleStyle.tabGlow },
      )
      if (tappThemes.length > 0) {
        tabs.push({ id: 'preset', label: t.titleStyle.tabPreset })
      }
      return tabs
    }, [showHeroOptions, t, tappThemes.length])
    const [activeTab, setActiveTab] = useState<TabType>(() =>
      showHeroOptions ? 'font' : 'surface',
    )
    useEffect(() => {
      if (!TABS.some((tab) => tab.id === activeTab)) {
        setActiveTab(TABS[0]?.id ?? 'surface')
      }
    }, [TABS, activeTab])
    const [panelPosition, setPanelPosition] = useState<StylePanelPosition>({
      top: 0,
      left: 0,
      placement: 'below',
    })
    const isDark = useThemeMode()
    const primaryColor = usePrimaryColor()

    const buttonRef = useRef<HTMLButtonElement>(null)
    const panelRef = useRef<HTMLDivElement>(null)

    const adaptivePreviewColor = useMemo(() => {
      void primaryColor
      return getTitleColorCss('adaptive', isDark)
    }, [isDark, primaryColor])

    useLayoutEffect(() => {
      if (!isOpen || !buttonRef.current) return

      const update = () => {
        if (!buttonRef.current) return
        const button = buttonRef.current.getBoundingClientRect()
        const panel = panelRef.current
        setPanelPosition(
          placeStylePanel(
            button,
            panel?.offsetWidth || STYLE_PANEL_WIDTH,
            panel?.offsetHeight || STYLE_PANEL_FALLBACK_HEIGHT,
          ),
        )
      }

      update()
      const frame = window.requestAnimationFrame(update)
      const panel = panelRef.current
      const observer =
        panel && typeof ResizeObserver !== 'undefined'
          ? new ResizeObserver(update)
          : null
      if (panel) observer?.observe(panel)
      window.addEventListener('resize', update)
      return () => {
        window.cancelAnimationFrame(frame)
        observer?.disconnect()
        window.removeEventListener('resize', update)
      }
    }, [isOpen, activeTab])

    useEffect(() => {
      if (!isOpen) return

      const handleClickOutside = (event: MouseEvent) => {
        const target = event.target as Node
        if (
          buttonRef.current &&
          !buttonRef.current.contains(target) &&
          panelRef.current &&
          !panelRef.current.contains(target)
        ) {
          setIsOpen(false)
        }
      }

      document.addEventListener('mousedown', handleClickOutside)
      return () => document.removeEventListener('mousedown', handleClickOutside)
    }, [isOpen])

    const handleOpen = useCallback(async () => {
      const willOpen = !isOpen
      if (willOpen && buttonRef.current) {
        setPanelPosition(
          placeStylePanel(
            buttonRef.current.getBoundingClientRect(),
            STYLE_PANEL_WIDTH,
            panelRef.current?.offsetHeight || STYLE_PANEL_FALLBACK_HEIGHT,
          ),
        )
      }
      setIsOpen(willOpen)
      if (willOpen && showHeroOptions) {
        preloadAllFonts()
      }
    }, [isOpen, preloadAllFonts, showHeroOptions])

    const handleSelectFont = useCallback(
      (font: FontOption) => {
        setTitleFont(font.id, csrfToken)
      },
      [setTitleFont, csrfToken],
    )

    const handleSelectSize = useCallback(
      (size: number) => {
        setTitleFontSize(size, csrfToken)
      },
      [setTitleFontSize, csrfToken],
    )

    const handleSelectColor = useCallback(
      (colorId: string) => {
        setTitleColor(colorId, csrfToken)
      },
      [setTitleColor, csrfToken],
    )

    const handleSelectSurface = useCallback(
      (id: WidgetSurface) => {
        setSurface(id, csrfToken)
      },
      [setSurface, csrfToken],
    )

    const handleSelectGlow = useCallback(
      (id: WidgetGlowMode) => {
        setGlowMode(id, csrfToken)
      },
      [setGlowMode, csrfToken],
    )

    const handleApplyPreset = useCallback(
      (preset: { surface?: WidgetSurface; glow?: WidgetGlowMode }) => {
        if (preset.surface) setSurface(preset.surface, csrfToken)
        if (preset.glow) setGlowMode(preset.glow, csrfToken)
      },
      [setSurface, setGlowMode, csrfToken],
    )

    const handleTabChange = useCallback((tab: TabType) => {
      setActiveTab(tab)
    }, [])

    const renderTabs = useMemo(
      () => (
        <div className="flex border-b border-gray-200 dark:border-white/10">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              onClick={() => handleTabChange(tab.id)}
              className={`flex-1 px-1.5 py-2 text-xs font-medium transition-colors ${
                activeTab === tab.id
                  ? 'text-gray-800 dark:text-gray-200 border-b-2'
                  : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
              }`}
              style={{
                borderColor:
                  activeTab === tab.id ? 'var(--color-primary)' : 'transparent',
              }}
            >
              {tab.label}
            </button>
          ))}
        </div>
      ),
      [activeTab, handleTabChange, TABS],
    )

    const renderFontList = useMemo(
      () => (
        <div className="space-y-1 max-h-56 overflow-y-auto">
          {AVAILABLE_FONTS.map((font) => (
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
                <FaCheck
                  className="w-3.5 h-3.5 shrink-0"
                  style={{ color: 'var(--color-primary)' }}
                />
              )}
            </button>
          ))}
        </div>
      ),
      [titleFont, isLoading, handleSelectFont],
    )

    const renderSizeList = useMemo(
      () => (
        <div className="space-y-1">
          {FONT_SIZE_OPTIONS.map((size) => (
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
                <FaCheck
                  className="w-3.5 h-3.5 shrink-0"
                  style={{ color: 'var(--color-primary)' }}
                />
              )}
            </button>
          ))}
        </div>
      ),
      [titleFontSize, handleSelectSize, t],
    )

    const renderColorList = useMemo(
      () => (
        <div className="space-y-1">
          {AVAILABLE_COLORS.map((color) => (
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
                    background:
                      color.id === 'adaptive'
                        ? adaptivePreviewColor
                        : color.value,
                  }}
                />
                <span className="text-sm text-gray-600 dark:text-gray-400">
                  {t.titleStyle[color.nameKey as keyof typeof t.titleStyle]}
                </span>
              </div>
              {titleColor === color.id && (
                <FaCheck
                  className="w-3.5 h-3.5 shrink-0"
                  style={{ color: 'var(--color-primary)' }}
                />
              )}
            </button>
          ))}
        </div>
      ),
      [titleColor, adaptivePreviewColor, handleSelectColor, t],
    )

    const renderSurfaceList = useMemo(
      () => (
        <div className="space-y-1">
          {SURFACE_OPTIONS.map((opt) => (
            <button
              key={opt.id}
              onClick={() => handleSelectSurface(opt.id)}
              className={`
            w-full px-3 py-2 rounded-lg text-left transition-colors flex items-center justify-between
            ${surface === opt.id ? 'bg-black/10 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/5'}
          `}
            >
              <div className="flex items-center gap-3 min-w-0">
                <div
                  className={`w-6 h-6 rounded-md shrink-0 ${opt.className}`}
                  aria-hidden
                />
                <span className="text-sm text-gray-600 dark:text-gray-400">
                  {t.titleStyle[opt.nameKey as keyof typeof t.titleStyle]}
                </span>
                {opt.id === 'liquid' && (
                  <span className="shrink-0 rounded-full border border-red-500/40 bg-red-500/15 px-1.5 py-0.5 text-[10px] font-semibold leading-none text-red-600 dark:border-red-400/40 dark:text-red-400">
                    {t.titleStyle.surfaceLiquidLoadWarning}
                  </span>
                )}
              </div>
              {surface === opt.id && (
                <FaCheck
                  className="w-3.5 h-3.5 shrink-0"
                  style={{ color: 'var(--color-primary)' }}
                />
              )}
            </button>
          ))}
        </div>
      ),
      [surface, handleSelectSurface, t],
    )

    const renderGlowList = useMemo(
      () => (
        <div className="space-y-1">
          {GLOW_OPTIONS.map((opt) => (
            <button
              key={opt.id}
              onClick={() => handleSelectGlow(opt.id)}
              className={`
            w-full px-3 py-2 rounded-lg text-left transition-colors flex items-center justify-between
            ${glow === opt.id ? 'bg-black/10 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/5'}
          `}
            >
              <div className="flex items-center gap-3">
                <div
                  className="w-6 h-6 rounded-md border border-gray-300 dark:border-gray-600 overflow-hidden relative"
                  aria-hidden
                >
                  {opt.id !== 'none' && (
                    <div
                      className="absolute -right-1 -top-1 w-4 h-4 rounded-full blur-[6px]"
                      style={{
                        background:
                          opt.id === 'primary'
                            ? 'var(--color-primary)'
                            : 'linear-gradient(135deg, #fb7299, #00A1D6)',
                        opacity: 0.7,
                      }}
                    />
                  )}
                </div>
                <span className="text-sm text-gray-600 dark:text-gray-400">
                  {t.titleStyle[opt.nameKey as keyof typeof t.titleStyle]}
                </span>
              </div>
              {glow === opt.id && (
                <FaCheck
                  className="w-3.5 h-3.5 shrink-0"
                  style={{ color: 'var(--color-primary)' }}
                />
              )}
            </button>
          ))}
        </div>
      ),
      [glow, handleSelectGlow, t],
    )

    const renderPresetList = useMemo(() => {
      const surfaceLabel = (id?: WidgetSurface) => {
        const opt = SURFACE_OPTIONS.find((o) => o.id === id)
        return opt
          ? t.titleStyle[opt.nameKey as keyof typeof t.titleStyle]
          : null
      }
      const glowLabel = (id?: WidgetGlowMode) => {
        const opt = GLOW_OPTIONS.find((o) => o.id === id)
        return opt
          ? t.titleStyle[opt.nameKey as keyof typeof t.titleStyle]
          : null
      }

      return (
        <div className="space-y-1 max-h-56 overflow-y-auto">
          {tappThemes.map((preset) => {
            const active =
              (!preset.surface || preset.surface === surface) &&
              (!preset.glow || preset.glow === glow)
            const summary = [
              surfaceLabel(preset.surface),
              glowLabel(preset.glow),
            ]
              .filter(Boolean)
              .join(' · ')
            return (
              <button
                key={preset.id}
                onClick={() => handleApplyPreset(preset)}
                className={`
            w-full px-3 py-2 rounded-lg text-left transition-colors flex items-center justify-between
            ${active ? 'bg-black/10 dark:bg-white/10' : 'hover:bg-black/5 dark:hover:bg-white/5'}
          `}
              >
                <div className="min-w-0">
                  <div className="text-sm text-gray-800 dark:text-gray-200 truncate">
                    {preset.name}
                  </div>
                  {summary && (
                    <div className="text-[10px] text-gray-500 dark:text-gray-400 truncate">
                      {summary}
                    </div>
                  )}
                </div>
                {active && (
                  <FaCheck
                    className="w-3.5 h-3.5 shrink-0 ml-2"
                    style={{ color: 'var(--color-primary)' }}
                  />
                )}
              </button>
            )
          })}
        </div>
      )
    }, [tappThemes, surface, glow, handleApplyPreset, t])

    const renderTabContent = useMemo(() => {
      switch (activeTab) {
        case 'font':
          return renderFontList
        case 'size':
          return renderSizeList
        case 'color':
          return renderColorList
        case 'surface':
          return renderSurfaceList
        case 'glow':
          return renderGlowList
        case 'preset':
          return renderPresetList
      }
    }, [
      activeTab,
      renderFontList,
      renderSizeList,
      renderColorList,
      renderSurfaceList,
      renderGlowList,
      renderPresetList,
    ])

    const trigger = (
      <button
        ref={buttonRef}
        onClick={handleOpen}
        className={
          buttonClassName ||
          'px-3 py-1.5 rounded-lg text-xs font-bold flex items-center gap-1.5 transition-colors bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10'
        }
        style={buttonClassName ? undefined : { color: 'var(--color-primary)' }}
        title={t.titleStyle.title}
      >
        <FaPalette className="w-3 h-3" />
        {t.titleStyle.style}
      </button>
    )

    const panel = createPortal(
      <AnimatePresence>
        {isOpen && (
          <motion.div
            ref={panelRef}
            data-library-dock-chrome=""
            initial={{
              opacity: 0,
              y: panelPosition.placement === 'above' ? 8 : -8,
              scale: 0.95,
            }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{
              opacity: 0,
              y: panelPosition.placement === 'above' ? 8 : -8,
              scale: 0.95,
            }}
            transition={{ duration: 0.15 }}
            className="title-font-selector-panel fixed w-80 glass rounded-xl shadow-lg overflow-hidden"
            style={{
              zIndex: 99999,
              top: panelPosition.top,
              left: panelPosition.left,
              transformOrigin:
                panelPosition.placement === 'above'
                  ? 'bottom left'
                  : 'top left',
            }}
          >
            {renderTabs}
            <div className="p-2">{renderTabContent}</div>
          </motion.div>
        )}
      </AnimatePresence>,
      document.body,
    )

    if (className) {
      return (
        <div className={className}>
          {trigger}
          {panel}
        </div>
      )
    }

    return (
      <>
        {trigger}
        {panel}
      </>
    )
  },
)

TitleFontSelector.displayName = 'TitleFontSelector'

export default TitleFontSelector
