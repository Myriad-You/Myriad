import type {
  WidgetConfig,
  WidgetGridHandle,
  WidgetType,
} from '../widgetGridTypes'
import { FaChevronLeft, FaChevronRight } from '@lib/icons'
import React, {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { API_URL as CONFIG_API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import {
  useHomeResizeObserver,
  useHomeVisibilityInterval,
} from '../../hooks/animation'
import { useEditModeEscape } from '../../hooks/useEditModeEscape'
import { useTappWidgets } from '../../hooks/useTappWidgets'
import { getCSRFToken } from '../../utils/csrf'
import { getUIConfigDeduped } from '../../utils/requestDedup'
import { showError } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import WidgetGrid, { startGridLibraryDrag } from '../WidgetGrid'
import WidgetLibraryIsland from '../WidgetLibraryIsland'
import { getBuiltinWidgets } from '../widgets/builtinWidgets'
import {
  PANEL_MORPH_BASE_MS,
  PANEL_SETTLE_SLACK_MS,
} from './panelTransition'
import {
  isHoverCapablePointer,
  shouldAutoAdvanceWidgets,
} from './widgetCarousel'
import {
  mergeVisibleWithHidden,
  packControlPanelWidgets,
} from './widgetReflow'
import './ControlPanelWidgets.css'

const API_URL = CONFIG_API_URL

const DEFAULT_CONTROL_PANEL_LAYOUT: WidgetConfig[] = [
  {
    id: 'cp-weather',
    type: 'weather',
    size: '2x2',
    position: { x: 0, y: 0 },
  },
  {
    id: 'cp-quote',
    type: 'quote',
    size: '2x2',
    position: { x: 2, y: 0 },
  },
]

interface ControlPanelWidgetsProps {
  isAdmin?: boolean
  /**
   * 小组件当前是否真的看得见（面板已展开且停在控制页）。
   * 收起后本组件仍然挂载（外壳只是 content-visibility: hidden），
   * 若不接这个信号，自动轮播会在看不见的子树上继续每 10 秒重渲染一次，
   * 下次展开时页码已经漂到别处。与 MusicPlayer 的 panelVisible 同源。
   */
  panelVisible?: boolean
}

// memo：宿主 GlobalControlPanel 因音乐进度/歌词轮播频繁重渲染，
// 本组件 props 只有 isAdmin 与低频的 panelVisible（仅相位切换时变），
// 隔离后不再跟随宿主的高频重渲染
export const ControlPanelWidgets: React.FC<ControlPanelWidgetsProps> = memo(
  ({ isAdmin = false, panelVisible = true }) => {
    const { t } = useI18n()

    // Shared built-in catalog (same source as Home)
    const BUILTIN_WIDGETS: WidgetType[] = useMemo(
      () => getBuiltinWidgets(t.widgets, 'control-panel'),
      [t.widgets],
    )

    // Tapp-registered widgets (same merge as Home)
    const { tappWidgets, isLoading: isTappWidgetsLoading } = useTappWidgets()

    const CONTROL_PANEL_WIDGETS: WidgetType[] = useMemo(
      () => [...BUILTIN_WIDGETS, ...tappWidgets],
      [BUILTIN_WIDGETS, tappWidgets],
    )

    const [widgets, setWidgets] = useState<WidgetConfig[]>(
      DEFAULT_CONTROL_PANEL_LAYOUT,
    )
    // Raw layout for re-validation after Tapp widgets load
    const [rawLayoutData, setRawLayoutData] = useState<WidgetConfig[] | null>(
      null,
    )
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
    const [gridRows, setGridRows] = useState(2)
    const [_isLoading, setIsLoading] = useState(true)
    // 指针停在小组件区域内：视为用户正在阅读/准备点击，暂停自动翻页。
    // 与智能岛收缩态轮播的 isHovering 语义保持一致
    const [isHovering, setIsHovering] = useState(false)
    const longPressTimer = useRef<NodeJS.Timeout | null>(null)
    const wheelCooldown = useRef(false)
    const startYRef = useRef(0)
    const startRowsRef = useRef(2)
    const currentDragRowsRef = useRef(2)
    const isDraggingRef = useRef(false)
    // 当前行数装不下、因而没有渲染出来的小组件。
    // 用 ref 而不是让 handleWidgetsChange 直接依赖，避免编辑回调随布局重建
    const hiddenWidgetsRef = useRef<WidgetConfig[]>([])
    // 行数切换前的容器高度，供切换后的 FLIP 动画用
    const heightBeforeRowsRef = useRef<number | null>(null)
    // 行数切换动画进行中：期间的尺寸变化由下面的 layout effect 统一驱动，
    // ResizeObserver 不要再把动画中间值当成新目标报给外壳
    const rowsAnimatingRef = useRef(false)
    const saveTimeoutRef = useRef<NodeJS.Timeout | null>(null)
    const containerRef = useRef<HTMLDivElement>(null)

    // 从后端加载配置（使用去重机制）
    useEffect(() => {
      const loadConfig = async () => {
        try {
          const data = await getUIConfigDeduped()
          if (data.control_panel_layout) {
            try {
              const layout = JSON.parse(data.control_panel_layout)
              if (Array.isArray(layout) && layout.length > 0) {
                setRawLayoutData(layout)
                // Keep layout as-is; WidgetGrid skips unknown types.
                // Re-validate when Tapp widgets finish loading.
                setWidgets(layout)
              }
            } catch (e) {
              console.error('Failed to parse control panel layout', e)
              showError(userFacingError(e, t.errors.controlPanelLoadFailed))
            }
          }
          if (data.control_panel_rows) {
            setGridRows(data.control_panel_rows)
          }
        } catch (e) {
          console.error('Failed to load control panel config', e)
          showError(userFacingError(e, t.errors.controlPanelLoadFailed))
        } finally {
          setIsLoading(false)
        }
      }
      loadConfig()
    }, [])

    // When Tapp widgets load, re-apply layout so registered types can render
    useEffect(() => {
      if (isTappWidgetsLoading || !rawLayoutData || tappWidgets.length === 0)
        return
      setWidgets(rawLayoutData)
    }, [isTappWidgetsLoading, tappWidgets, rawLayoutData])

    // 保存配置到后端（防抖，仅管理员）
    const saveToBackend = useCallback(
      async (layout: WidgetConfig[], rows: number) => {
        if (!isAdmin) return

        // 清除之前的定时器
        if (saveTimeoutRef.current) {
          clearTimeout(saveTimeoutRef.current)
        }

        // 防抖 500ms
        saveTimeoutRef.current = setTimeout(async () => {
          try {
            const csrfToken = await getCSRFToken(true)
            if (!csrfToken) {
              showError(t.errors.csrfUnavailable)
              return
            }

            const res = await fetch(`${API_URL}/api/config/control-panel`, {
              method: 'POST',
              headers: {
                'Content-Type': 'application/json',
                'X-CSRF-Token': csrfToken,
              },
              credentials: 'include',
              body: JSON.stringify({
                control_panel_layout: JSON.stringify(layout),
                control_panel_rows: rows,
              }),
            })
            if (!res.ok) {
              throw new Error(
                `Failed to save control panel: HTTP ${res.status}`,
              )
            }
          } catch (err) {
            console.error('Failed to save control panel config:', err)
            showError(userFacingError(err, t.errors.controlPanelSaveFailed))
          }
        }, 500)
      },
      [isAdmin, t.errors.controlPanelSaveFailed, t.errors.csrfUnavailable],
    )

    const handleWidgetsChange = useCallback(
      (newWidgets: WidgetConfig[]) => {
        // Drop unregistered types so saved layout stays valid — but only after
        // Tapp catalog has finished loading. While loading, CONTROL_PANEL_WIDGETS
        // is builtins-only; filtering would strip stored Tapp widgets and POST
        // an emptied layout.
        const registeredIds = new Set(CONTROL_PANEL_WIDGETS.map((w) => w.id))
        const validWidgets = isTappWidgetsLoading
          ? newWidgets
          : newWidgets.filter((w) => registeredIds.has(w.type))
        // WidgetGrid 收到的是规范化后的可见集合，回传的自然也只有可见项。
        // 当前行数装不下、因而没有渲染出来的那些必须原样并回去，
        // 否则在 1 行模式下随便拖一下就把它们删了
        const merged = mergeVisibleWithHidden(
          validWidgets,
          hiddenWidgetsRef.current,
        )
        setWidgets(merged)
        setRawLayoutData(merged)
        if (!isTappWidgetsLoading) {
          saveToBackend(merged, gridRows)
        }
      },
      [gridRows, saveToBackend, CONTROL_PANEL_WIDGETS, isTappWidgetsLoading],
    )

    const handleRowsChange = useCallback(
      (rows: number) => {
        // 必须在 setState 之前同步读，之后 DOM 已经是新布局
        const el = containerRef.current
        heightBeforeRowsRef.current = el
          ? el.getBoundingClientRect().height
          : null
        setGridRows(rows)

        // 当切换到 1 行模式时，自动调整小组件尺寸为 4x1
        // 当切换到 2 行模式时，恢复为默认尺寸
        let updatedWidgets: WidgetConfig[]
        if (rows === 1) {
          // 只改能压成 4x1 的尺寸。不支持 4x1 的（或 Tapp 还在加载）原样留下，
          // 渲染时的 pack 会把它们放进 overflow，切回 2 行时再出来。
          // 绝不能在这里 filter：否则一改行数就把用户的小组件从存盘里删了。
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
          // 切换回 2 行模式时，恢复为 2x2 尺寸
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

        // 只调整 size，位置交给渲染时的规范化（displayWidgets）。
        // 这里不做打包也不裁剪：1 行模式装不下的小组件必须留在数据里，
        // 否则切一次行数就把它们从后端永久删掉了
        setWidgets(updatedWidgets)
        setRawLayoutData(updatedWidgets)
        // Avoid persisting a layout filtered without Tapp catalog
        if (!isTappWidgetsLoading) {
          saveToBackend(updatedWidgets, rows)
        }
      },
      [widgets, saveToBackend, CONTROL_PANEL_WIDGETS, isTappWidgetsLoading],
    )

    /**
     * 行数切换的高度过渡：让内容与外壳跑在同一条时间线上。
     *
     * 内容高度由 WidgetGrid 的 autoHeight 决定，是 auto —— `transition-all`
     * 对 auto 不生效，所以旧行为是内容瞬间跳变；而外壳要等
     * ResizeObserver 的 150ms 节流 + 测量侧的 600ms 节流才开始它的 700ms 过渡，
     * 中间几百毫秒面板底部要么露白要么把控制项裁掉。
     *
     * 这里把新高度钉成显式值做 FLIP，并在同一帧用 immediate 重测通知外壳，
     * 两条高度用同一个 --gcp-morph 时长与曲线同时开始、同时结束。
     */
    useLayoutEffect(() => {
      const el = containerRef.current
      const from = heightBeforeRowsRef.current
      heightBeforeRowsRef.current = null
      if (!el || from == null) return

      // 此刻 DOM 已是新布局，height 仍是 auto，读到的就是目标高度
      const to = el.getBoundingClientRect().height
      if (Math.abs(to - from) < 1) return

      // 先让外壳按新的内容高度拿到目标值（measure 读的是 auto 布局下的
      // scrollHeight，必须赶在下面把高度钉住之前完成）
      window.dispatchEvent(
        new CustomEvent('gcp-remeasure', { detail: { immediate: true } }),
      )

      rowsAnimatingRef.current = true
      el.style.height = `${from}px`
      void el.offsetHeight // 强制回流，确保下一行是一次真正的过渡起点
      el.style.height = `${to}px`

      const done = (e: TransitionEvent) => {
        if (e.target !== el || e.propertyName !== 'height') return
        finish()
      }
      const finish = () => {
        el.removeEventListener('transitionend', done)
        el.removeEventListener('transitioncancel', done)
        window.clearTimeout(fallback)
        rowsAnimatingRef.current = false
        el.style.height = '' // 交还给内容驱动
      }
      // 跟外壳同一条时钟：--gcp-morph + slack。写死 1200ms 会在
      // reduced-motion / exlight 丢 transitionend 时把高度钉住一整秒。
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

    // 🆕 使用首页原子化 ResizeObserver
    const { observeHomeResize, unobserveHomeResize } = useHomeResizeObserver()

    // 监听 WidgetGrid 的高度变化，通知父级控制面板重新计算高度
    useEffect(() => {
      const widgetContainer = containerRef.current
      if (!widgetContainer) return

      let throttleTimer: ReturnType<typeof setTimeout> | null = null
      const THROTTLE_MS = 150 // 最少150ms触发一次

      observeHomeResize(widgetContainer, () => {
        // 行数切换动画期间高度每帧都在变，交给 layout effect 一次性通知外壳
        if (rowsAnimatingRef.current) return
        if (throttleTimer) return

        throttleTimer = setTimeout(() => {
          throttleTimer = null
          // 触发自定义事件通知 GlobalControlPanel 重新计算高度
          window.dispatchEvent(new CustomEvent('control-panel-content-resize'))
        }, THROTTLE_MS)
      })

      return () => {
        if (throttleTimer) clearTimeout(throttleTimer)
        unobserveHomeResize(widgetContainer)
      }
    }, [observeHomeResize, unobserveHomeResize])

    const handleResizeStart = (e: React.MouseEvent) => {
      e.preventDefault()
      e.stopPropagation()
      startYRef.current = e.clientY
      startRowsRef.current = gridRows
      currentDragRowsRef.current = gridRows
      isDraggingRef.current = false

      document.addEventListener('mousemove', handleResizeMove)
      document.addEventListener('mouseup', handleResizeEnd)
    }

    const handleResizeMove = (e: MouseEvent) => {
      const deltaY = e.clientY - startYRef.current

      // Mark as dragging if moved more than small threshold
      if (Math.abs(deltaY) > 5) {
        isDraggingRef.current = true
      }

      const threshold = 10 // 10px threshold for switch
      let targetRows = startRowsRef.current

      if (startRowsRef.current === 2) {
        // If expanded, drag up to shrink
        if (deltaY < -threshold) {
          targetRows = 1
        } else {
          targetRows = 2
        }
      } else {
        // If compact, drag down to expand
        if (deltaY > threshold) {
          targetRows = 2
        } else {
          targetRows = 1
        }
      }

      if (targetRows !== currentDragRowsRef.current) {
        currentDragRowsRef.current = targetRows
        handleRowsChange(targetRows)
      }
    }

    const handleResizeEnd = () => {
      document.removeEventListener('mousemove', handleResizeMove)
      document.removeEventListener('mouseup', handleResizeEnd)
    }

    const handleMouseDown = useCallback(() => {
      // 只有管理员可以进入编辑模式
      if (!isAdmin || isEditMode) return
      longPressTimer.current = setTimeout(() => {
        setIsEditMode(true)
      }, 800)
    }, [isAdmin, isEditMode])

    const handleMouseUp = () => {
      if (longPressTimer.current) {
        clearTimeout(longPressTimer.current)
        longPressTimer.current = null
      }
    }

    useEffect(() => {
      if (!isEditMode) return
      const root = document.documentElement
      root.classList.add('gcp-widget-edit')
      return () => root.classList.remove('gcp-widget-edit')
    }, [isEditMode])

    useEffect(() => {
      if (panelVisible) return
      setIsEditMode(false)
    }, [panelVisible])

    // 计算最大页数 (基于内容)
    /**
     * 渲染用的规范化布局。
     *
     * 打包放在这里而不是放进 state：行数、尺寸都可能让存下来的坐标失效
     * （历史上还存进过互相重叠的布局），渲染前统一规范一次就都正了；
     * 而 widgets 保持完整逻辑列表，换回容量更大的行数时溢出项会自己回来。
     */
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
        // 简单判断：如果 x >= 8 则在第3页，x >= 4 则在第2页
        if (w.position.x > maxX) maxX = w.position.x
      })

      const lastOccupiedPage = Math.floor(maxX / 4)
      // 编辑模式下允许访问下一页（最多3页，即索引2）
      if (isEditMode) return Math.min(2, lastOccupiedPage + 1)
      // 浏览模式下仅允许访问有内容的页
      return Math.max(0, lastOccupiedPage)
    }, [displayWidgets, isEditMode])

    // 确保当前页不超过最大页
    useEffect(() => {
      if (currentPage > maxPage) {
        setCurrentPage(maxPage)
      }
    }, [maxPage, currentPage])

    // 自动切换页面（10 秒一次）。hook 自带的可见性是页面级（tab 是否可见），
    // 面板收起与指针停留都要另外把闸门关掉：
    // - panelVisible：收起或切到通知页时小组件根本看不见，不该继续翻页
    // - isHovering：用户正停在某张卡片上时翻走会打断阅读/点击
    //   （滚轮切页时指针必然在区域内，因此手动翻页期间轮播天然静默）
    useHomeVisibilityInterval(
      () => setCurrentPage((prev) => (prev >= maxPage ? 0 : prev + 1)),
      10000,
      shouldAutoAdvanceWidgets({ isEditMode, maxPage, panelVisible, isHovering }),
    )

    // 根据 gridRows 过滤可用小组件（1行模式只显示支持 4x1/2x1/1x1 的小组件）
    const filteredWidgets = useMemo((): WidgetType[] => {
      if (gridRows === 1) {
        // 显示支持 4x1、2x1、1x1 的小组件
        return CONTROL_PANEL_WIDGETS.filter((w) => {
          const sizes = w.supportedSizes || []
          return (
            sizes.includes('4x1') ||
            sizes.includes('2x1') ||
            sizes.includes('1x1')
          )
        }).map((w) => {
          const sizes = w.supportedSizes || []
          // 只保留高度为1的尺寸
          const allowedSizes = sizes.filter(
            (s) => s === '4x1' || s === '2x1' || s === '1x1',
          )
          // 优先使用 4x1，其次 2x1，最后 1x1
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

    // 滚轮切换页面处理
    const handleWheel = (e: React.WheelEvent) => {
      if (wheelCooldown.current) return

      // 阈值判断，避免过于灵敏
      if (Math.abs(e.deltaY) > 30) {
        if (e.deltaY > 0) {
          // 向下/向右滚动 -> 下一页
          if (currentPage < maxPage) {
            setCurrentPage((p) => p + 1)
            wheelCooldown.current = true
            setTimeout(() => (wheelCooldown.current = false), 400)
          }
        } else {
          // 向上/向左滚动 -> 上一页
          if (currentPage > 0) {
            setCurrentPage((p) => p - 1)
            wheelCooldown.current = true
            setTimeout(() => (wheelCooldown.current = false), 400)
          }
        }
      }
    }

    return (
      <>
        <WidgetLibraryIsland
          visible={isEditMode}
          parkable={false}
          availableWidgets={filteredWidgets}
          onNewWidgetDragStart={onLibraryDragStart}
        />

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
        >
          {/* 浅色大框架卡片 */}
          <div className="bg-gray-100/50 dark:bg-white/5 rounded-2xl p-1 border border-gray-200/50 dark:border-white/5 shadow-inner overflow-hidden relative group/container transition-all duration-300 ease-in-out">
            {/* 页面容器 - 通过 transform 切换 */}
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
                    customGridColumns={12} // 3页宽度 (4 * 3)
                    customGridRows={gridRows}
                    autoHeight={true}
                  />
                </div>
              </div>
            </div>

            {/* 翻页按钮 - 仅编辑模式显示；浏览模式依赖自动轮播与滚轮切页 */}
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

            {/* 底部拉伸条 - 仅在编辑模式下显示 */}
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
