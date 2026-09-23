import type {
  WidgetConfig,
  WidgetGridHandle,
  WidgetType,
} from '../widgetGridTypes'
import { FaChevronLeft, FaChevronRight } from '@lib/faChromeIcons'
import React, {
  lazy,
  memo,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useVisibilityInterval } from '../../hooks/animation'
import { useEditModeEscape } from '../../hooks/useEditModeEscape'
import { useTappWidgets } from '../../hooks/useTappWidgets'
import { emitAppEvent } from '../../utils/appEvents'
import WidgetGrid, { startGridLibraryDrag } from '../WidgetGrid'
import { getBuiltinWidgets } from '../widgets/builtinWidgets'
import {
  PANEL_MORPH_BASE_MS,
  PANEL_SETTLE_SLACK_MS,
} from './panelTransition'
import { useWidgetGestures } from './useWidgetGestures'
import { useWidgetLayout } from './useWidgetLayout'
import {
  isHoverCapablePointer,
  shouldAutoAdvanceWidgets,
} from './widgetCarousel'
import { saveControlPanelLayout } from './widgetLayoutPersistence'
import {
  mergeVisibleWithHidden,
  packControlPanelWidgets,
} from './widgetReflow'
import './ControlPanelWidgets.css'

const WidgetLibraryIsland = lazy(() => import('../WidgetLibraryIsland'))

interface ControlPanelWidgetsProps {
  isAdmin?: boolean
  // 收起后仍挂载（content-visibility:hidden）；不接 panelVisible 的话轮播会在看不见时继续翻页。
  panelVisible?: boolean
}

export const ControlPanelWidgets: React.FC<ControlPanelWidgetsProps> = memo(
  ({ isAdmin = false, panelVisible = true }) => {
    const { t } = useI18n()

    const BUILTIN_WIDGETS: WidgetType[] = useMemo(
      () => getBuiltinWidgets(t.widgets, 'control-panel'),
      [t.widgets],
    )

    const { tappWidgets, isLoading: isTappWidgetsLoading } = useTappWidgets()

    const CONTROL_PANEL_WIDGETS: WidgetType[] = useMemo(
      () => [...BUILTIN_WIDGETS, ...tappWidgets],
      [BUILTIN_WIDGETS, tappWidgets],
    )

    const { widgets, gridRows, editLayout } = useWidgetLayout(t.errors.controlPanelLoadFailed)
    const [isEditMode, setIsEditMode] = useState(false)
    const gridRef = useRef<WidgetGridHandle>(null)
    useEditModeEscape(isEditMode, () => setIsEditMode(false))
    const onLibraryDragStart = useCallback(
      (
        event: Parameters<typeof startGridLibraryDrag>[1],
        widgetTypeId: string,
      ) => {
        startGridLibraryDrag(gridRef.current, event, widgetTypeId)
      },
      [],
    )
    const [currentPage, setCurrentPage] = useState(0)
    const [isHovering, setIsHovering] = useState(false)
    // overflow 用 ref，避免编辑回调随布局重建。
    const hiddenWidgetsRef = useRef<WidgetConfig[]>([])
    const heightBeforeRowsRef = useRef<number | null>(null)
    const containerRef = useRef<HTMLDivElement>(null)

    const saveToBackend = useCallback((layout: WidgetConfig[], rows: number) => {
      if (isAdmin) saveControlPanelLayout(layout, rows)
    }, [isAdmin])

    const handleWidgetsChange = useCallback(
      (newWidgets: WidgetConfig[]) => {
        // Tapp 目录未加载完不要过滤未注册类型，否则会把已存 Tapp 格子 POST 成空。
        const registeredIds = new Set(CONTROL_PANEL_WIDGETS.map((w) => w.id))
        const validWidgets = isTappWidgetsLoading
          ? newWidgets
          : Iterator.from(newWidgets)
              .filter((w) => registeredIds.has(w.type))
              .toArray()
        // WidgetGrid 只回可见项；行数装不下的必须并回去，否则 1 行模式一拖就删。
        const merged = mergeVisibleWithHidden(
          validWidgets,
          hiddenWidgetsRef.current,
        )
        editLayout(merged)
        if (!isTappWidgetsLoading) {
          saveToBackend(merged, gridRows)
        }
      },
      [gridRows, saveToBackend, CONTROL_PANEL_WIDGETS, isTappWidgetsLoading, editLayout],
    )

    const handleRowsChange = useCallback(
      (rows: number) => {
        // 必须在 setState 之前同步读高度，之后 DOM 已是新布局。
        const el = containerRef.current
        heightBeforeRowsRef.current = el
          ? el.getBoundingClientRect().height
          : null

        let updatedWidgets: WidgetConfig[]
        if (rows === 1) {
          // 只改能压成 4x1 的尺寸；绝不能 filter，否则改行数会把格子从存盘删掉。
          updatedWidgets = widgets.map((w) => {
            const widgetType = CONTROL_PANEL_WIDGETS.find(
              (wt) => wt.id === w.type,
            )
            if (widgetType?.supportedSizes?.includes('4x1')) {
              return { ...w, size: '4x1' as const }
            }
            return w
          })
        } else {
          updatedWidgets = widgets.map((w) => {
            if (w.size === '4x1') {
              const widgetType = CONTROL_PANEL_WIDGETS.find(
                (wt) => wt.id === w.type,
              )
              const defaultSize = widgetType?.defaultSize || '2x2'
              return { ...w, size: defaultSize }
            }
            return w
          })
        }

        // 只改 size，不做打包/裁剪；1 行装不下的必须留在数据里。
        editLayout(updatedWidgets, rows)
        // Tapp 目录未就绪时不要把过滤后的布局写盘。
        if (!isTappWidgetsLoading) {
          saveToBackend(updatedWidgets, rows)
        }
      },
      [widgets, saveToBackend, CONTROL_PANEL_WIDGETS, isTappWidgetsLoading, editLayout],
    )

    // 行数切换用 FLIP 钉显式高度，并同帧 immediate 重测；auto 不能 transition，外壳节流会露白或裁按钮。
    useLayoutEffect(() => {
      const el = containerRef.current
      const from = heightBeforeRowsRef.current
      heightBeforeRowsRef.current = null
      if (!el || from == null) return

      const to = el.getBoundingClientRect().height
      if (Math.abs(to - from) < 1) return

      // 先让外壳读 auto 下的 scrollHeight，再钉高度。
      emitAppEvent('gcp-remeasure', { immediate: true })

      el.style.height = `${from}px`
      void el.offsetHeight
      el.style.height = `${to}px`

      const done = (e: TransitionEvent) => {
        if (e.target !== el || e.propertyName !== 'height') return
        finish()
      }
      const finish = () => {
        el.removeEventListener('transitionend', done)
        el.removeEventListener('transitioncancel', done)
        window.clearTimeout(fallback)
        el.style.height = ''
      }
      // 跟外壳同一条 --gcp-morph 时钟；写死 1200ms 会在 reduced-motion/exlight 丢 transitionend 时把高度钉住。
      const morphMs = Number.parseFloat(
        getComputedStyle(el).getPropertyValue('--gcp-morph'),
      )
      const fallback = window.setTimeout(
        finish,
        (Number.isFinite(morphMs) && morphMs >= 0
          ? morphMs
          : PANEL_MORPH_BASE_MS) + PANEL_SETTLE_SLACK_MS,
      )
      el.addEventListener('transitionend', done)
      el.addEventListener('transitioncancel', done)

      return finish
    }, [gridRows])

    useEffect(() => {
      if (!isEditMode) return
      const root = document.documentElement
      root.classList.add('gcp-widget-edit')
      return () => root.classList.remove('gcp-widget-edit')
    }, [isEditMode])

    useEffect(() => {
      if (panelVisible && isAdmin) return
      setIsEditMode(false)
    }, [panelVisible, isAdmin])

    // 打包放渲染层不放 state；widgets 保持完整列表，换回更大行数时溢出项会回来。
    const { placed: displayWidgets, overflow: hiddenWidgets } = useMemo(
      () => packControlPanelWidgets(widgets, gridRows),
      [widgets, gridRows],
    )

    useLayoutEffect(() => {
      hiddenWidgetsRef.current = hiddenWidgets
    }, [hiddenWidgets])

    const maxPage = useMemo(() => {
      let maxX = -1
      displayWidgets.forEach((w) => {
        if (w.position.x > maxX) maxX = w.position.x
      })

      const lastOccupiedPage = Math.floor(maxX / 4)
      if (isEditMode) return Math.min(2, lastOccupiedPage + 1)
      return Math.max(0, lastOccupiedPage)
    }, [displayWidgets, isEditMode])

    useEffect(() => {
      if (currentPage > maxPage) {
        setCurrentPage(maxPage)
      }
    }, [maxPage, currentPage])

    // 自动翻页另接 panelVisible 与 isHovering：收起/通知页或指针停在卡片上时不要翻。
    useVisibilityInterval(
      () => setCurrentPage((prev) => (prev >= maxPage ? 0 : prev + 1)),
      { delay: 10000, enabled: shouldAutoAdvanceWidgets({ isEditMode, maxPage, panelVisible, isHovering }) },
    )

    const filteredWidgets = useMemo((): WidgetType[] => {
      if (gridRows === 1) {
        return CONTROL_PANEL_WIDGETS.filter((w) => {
          const sizes = w.supportedSizes ?? []
          return (
            sizes.includes('4x1') ||
            sizes.includes('2x1') ||
            sizes.includes('1x1')
          )
        }).map((w) => {
          const sizes = w.supportedSizes ?? []
          const allowedSizes = sizes.filter(
            (s) => s === '4x1' || s === '2x1' || s === '1x1',
          )
          let defaultSize: WidgetType['defaultSize'] = '1x1'
          if (allowedSizes.includes('4x1')) defaultSize = '4x1'
          else if (allowedSizes.includes('2x1')) defaultSize = '2x1'

          return {
            ...w,
            defaultSize,
            supportedSizes: allowedSizes as WidgetType['supportedSizes'],
          }
        })
      }
      return CONTROL_PANEL_WIDGETS
    }, [gridRows, CONTROL_PANEL_WIDGETS])

    const { handleMouseDown, handleMouseUp, handleResizeStart, handleWheel, isDraggingRef } = useWidgetGestures({
      visible: panelVisible,
      isAdmin,
      editing: isEditMode,
      rows: gridRows,
      page: currentPage,
      maxPage,
      onEdit: () => setIsEditMode(true),
      onPrepareEdit: () => { void import('../WidgetLibraryIsland') },
      onRows: handleRowsChange,
      onPage: setCurrentPage,
    })

    return (
      <>
        {isEditMode ? (
          <Suspense fallback={null}>
            <WidgetLibraryIsland
              visible
              parkable={false}
              availableWidgets={filteredWidgets}
              onNewWidgetDragStart={onLibraryDragStart}
            />
          </Suspense>
        ) : null}

        <div
          ref={containerRef}
          className="control-panel-widgets-container relative w-full transition-all rounded-xl"
          onMouseDown={handleMouseDown}
          onMouseUp={handleMouseUp}
          onPointerEnter={(e) => {
            if (isHoverCapablePointer(e.pointerType)) setIsHovering(true)
          }}
          onPointerLeave={() => {
            handleMouseUp()
            setIsHovering(false)
          }}
          onTouchStart={handleMouseDown}
          onTouchEnd={handleMouseUp}
          onTouchCancel={handleMouseUp}
        >
          <div className="bg-gray-100/50 dark:bg-white/5 rounded-2xl p-1 border border-gray-200/50 dark:border-white/5 shadow-inner overflow-hidden relative group/container transition-all duration-300 ease-in-out">
            <div className="w-full overflow-hidden" onWheel={handleWheel}>
              <div
                className={`flex transition-transform duration-500 cubic-bezier(0.25, 1, 0.5, 1) pages-3 slider page-${Math.min(2, Math.max(0, currentPage))}`}
              >
                <div className="w-full transition-all duration-300 ease-in-out">
                  <WidgetGrid
                    ref={gridRef}
                    widgets={displayWidgets}
                    availableWidgets={filteredWidgets}
                    onWidgetsChange={handleWidgetsChange}
                    isEditMode={isEditMode}
                    customGridColumns={12}
                    customGridRows={gridRows}
                    autoHeight={true}
                  />
                </div>
              </div>
            </div>

            {isEditMode && (
              <div
                className="relative z-30 flex items-center justify-between px-1 pt-1.5 pb-0.5 select-none"
                onMouseDown={(e) => e.stopPropagation()}
              >
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation()
                    setCurrentPage((p) => Math.max(0, p - 1))
                  }}
                  disabled={currentPage <= 0}
                  aria-label={t.widgetGrid.prevPage}
                  className="flex h-6 w-6 items-center justify-center rounded-full text-gray-500 dark:text-white/60 transition-all hover:bg-black/5 dark:hover:bg-white/10 disabled:opacity-20 disabled:cursor-not-allowed"
                >
                  <FaChevronLeft size={9} />
                </button>

                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation()
                    setCurrentPage((p) => Math.min(maxPage, p + 1))
                  }}
                  disabled={currentPage >= maxPage}
                  aria-label={t.widgetGrid.nextPage}
                  className="flex h-6 w-6 items-center justify-center rounded-full text-gray-500 dark:text-white/60 transition-all hover:bg-black/5 dark:hover:bg-white/10 disabled:opacity-20 disabled:cursor-not-allowed"
                >
                  <FaChevronRight size={9} />
                </button>
              </div>
            )}

            {isEditMode && (
              <div
                className="absolute bottom-0 left-1/2 -translate-x-1/2 w-32 h-4 cursor-ns-resize z-20 flex items-end justify-center opacity-0 group-hover/container:opacity-100 transition-opacity hover:opacity-100!"
                onMouseDown={handleResizeStart}
                onClick={(e) => {
                  e.stopPropagation()
                  if (isDraggingRef.current) return
                  handleRowsChange(gridRows === 1 ? 2 : 1)
                }}
              >
                <div className="w-16 h-1 bg-gray-300/50 dark:bg-white/20 rounded-full mb-1 hover:bg-gray-400/50 dark:hover:bg-white/40 transition-colors" />
              </div>
            )}
          </div>
        </div>
      </>
    )
  },
)

ControlPanelWidgets.displayName = 'ControlPanelWidgets'
