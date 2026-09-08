/**
 * Edit-mode widget catalog (Stage Manager dock).
 * Sibling of WidgetGrid — the page that owns edit mode mounts both.
 */

import type { TappCategory } from '../tapp/types'
import type { HomeLayoutMode } from '../utils/homeLayout'
import type { HomeEditTourDockPose } from './tour/tourLogic'
import type { WidgetType } from './widgetGridTypes'
import type { WidgetLibraryKindFilter } from './widgetLibrarySearch'
import { FaSearch, FaTimes } from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import React, {
  Suspense,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../contexts/I18nContext'
import { isExlight, useAnimationLevel } from '../hooks/useAnimationLevel'
import { useLibraryDockStage } from '../hooks/useLibraryDockStage'
import { useDebouncedWindowSize } from '../hooks/useSharedEventListener'
import { TAPP_CATEGORY_I18N_KEYS } from '../tapp/utils/tappCategories'
import { standardHomeCellSize } from '../utils/homeLayout'
import {
  LIBRARY_DOCK_CHROME_ATTR,
  LIBRARY_DOCK_PARK_EASE,
  LIBRARY_DOCK_PARK_TRANSITION,
  LIBRARY_DOCK_STAGE_ORIGIN_X,
  LIBRARY_DOCK_STAGE_ORIGIN_Y,
  LIBRARY_DOCK_STAGE_PERSPECTIVE,
  LIBRARY_DOCK_STAGE_SCALE_HOVER,
  libraryDockStageTransform,
} from '../utils/libraryDockStage'
import { useWidgetDragActive } from '../utils/widgetDragCursor'
import {
  libraryDockPreviewDisplayScale,
  STANDARD_CELL_SIZE,
  widgetSizeSpan,
} from '../utils/widgetSizeScale'
import {
  widgetDisplayLabel,
  widgetLibraryKindSource,
  widgetPreviewConfig,
  widgetSearchExtras,
} from './widgetLibraryModel'
import {
  presentWidgetLibraryKindFilters,
  tappCategoryFromKindFilter,
  widgetMatchesLibraryKind,
  widgetTypeMatchesLibrarySearch,
} from './widgetLibrarySearch'
import { preloadBuiltinWidgets } from './widgets/builtinWidgets'
import './WidgetLibraryIsland.css'

function libraryFilterLabel(
  id: WidgetLibraryKindFilter,
  t: ReturnType<typeof useI18n>['t'],
): string {
  if (id === 'all') return t.widgetGrid.filterAll
  if (id === 'report') return t.widgetGrid.filterReports
  const tappCategory = tappCategoryFromKindFilter(id)
  if (tappCategory && tappCategory in TAPP_CATEGORY_I18N_KEYS) {
    return t.tapp[TAPP_CATEGORY_I18N_KEYS[tappCategory as TappCategory]]
  }
  return tappCategory || id
}

const PREVIEW_LAZY_ROOT_MARGIN = '280px 0px'

const LibraryPreviewSlot = React.memo(
  ({
    scrollRef,
    renderWidth,
    renderHeight,
    displayScale,
    children,
  }: {
    scrollRef: React.RefObject<HTMLDivElement | null>
    renderWidth: number
    renderHeight: number
    displayScale: number
    children: React.ReactNode
  }) => {
    const [mounted, setMounted] = useState(false)
    const slotRef = useRef<HTMLDivElement | null>(null)

    useEffect(() => {
      if (mounted) return
      const node = slotRef.current
      if (!node) return
      if (typeof IntersectionObserver === 'undefined') {
        setMounted(true)
        return
      }
      const observer = new IntersectionObserver(
        (entries) => {
          if (entries.some((entry) => entry.isIntersecting)) {
            setMounted(true)
            observer.disconnect()
          }
        },
        { root: scrollRef.current ?? null, rootMargin: PREVIEW_LAZY_ROOT_MARGIN },
      )
      observer.observe(node)
      return () => observer.disconnect()
    }, [mounted, scrollRef])

    return (
      <div
        ref={slotRef}
        className="widget-library-preview"
        style={{
          width: renderWidth,
          height: renderHeight,
          transform: `scale(${displayScale})`,
        }}
      >
        {mounted ? <Suspense fallback={null}>{children}</Suspense> : null}
      </div>
    )
  },
)
LibraryPreviewSlot.displayName = 'LibraryPreviewSlot'

const WidgetLibraryTile = React.memo(
  ({
    widgetType,
    displayScale,
    scrollRef,
    widgetsI18n,
    onDragStart,
  }: {
    widgetType: WidgetType
    displayScale: number
    scrollRef: React.RefObject<HTMLDivElement | null>
    widgetsI18n: Record<string, unknown>
    onDragStart: (
      e: React.MouseEvent | React.TouchEvent,
      id: string,
    ) => void
  }) => {
    const WidgetComponent = widgetType.component
    const span = widgetSizeSpan(widgetType.defaultSize)
    const standard = {
      width: span.w * STANDARD_CELL_SIZE,
      height: span.h * STANDARD_CELL_SIZE,
    }
    const renderWidth = standard.width
    const renderHeight = standard.height
    const wrapperWidth = renderWidth * displayScale
    const wrapperHeight = renderHeight * displayScale
    const previewConfig = useMemo(
      () => widgetPreviewConfig(widgetType),
      [widgetType],
    )
    const libraryLabel = widgetDisplayLabel(widgetType, widgetsI18n)
    const startDrag = {
      onMouseDown: (e: React.MouseEvent) => onDragStart(e, widgetType.id),
      onTouchStart: (e: React.TouchEvent) => onDragStart(e, widgetType.id),
    }
    return (
      <div className="widget-library-tile" draggable={false} {...startDrag}>
        <div
          className="widget-library-tile-stage"
          style={{ width: wrapperWidth, height: wrapperHeight }}
        >
          <LibraryPreviewSlot
            scrollRef={scrollRef}
            renderWidth={renderWidth}
            renderHeight={renderHeight}
            displayScale={displayScale}
          >
            <WidgetComponent
              config={previewConfig}
              isEditMode={false}
              isPreview={true}
            />
          </LibraryPreviewSlot>
          <div className="widget-library-tile-frame" />
        </div>
        <div className="widget-library-tile-name" title={libraryLabel}>
          {libraryLabel}
        </div>
      </div>
    )
  },
)
WidgetLibraryTile.displayName = 'WidgetLibraryTile'

function LibrarySearchField({
  query,
  onQueryChange,
  searchLabel,
  clearLabel,
}: {
  query: string
  onQueryChange: (query: string) => void
  searchLabel: string
  clearLabel: string
}) {
  return (
    <div className="widget-library-search">
      <FaSearch className="widget-library-search-icon" size={12} aria-hidden />
      <input
        type="search"
        value={query}
        onChange={(e) => onQueryChange(e.target.value)}
        placeholder={searchLabel}
        aria-label={searchLabel}
        autoComplete="off"
        className="widget-library-search-input"
      />
      {query ? (
        <button
          type="button"
          onClick={() => onQueryChange('')}
          className="widget-library-search-clear"
          title={clearLabel}
          aria-label={clearLabel}
        >
          <FaTimes size={10} />
        </button>
      ) : null}
    </div>
  )
}

export interface WidgetLibraryIslandProps {
  visible: boolean
  availableWidgets: WidgetType[]
  onNewWidgetDragStart: (
    e: React.MouseEvent | React.TouchEvent,
    id: string,
  ) => void
  /** Home free/standard switch: skip park tween for one frame. */
  layoutMode?: HomeLayoutMode
  /**
   * Stage Manager park. Home keeps the thumbnail; control-panel catalog
   * stays a full window (`parkable={false}`).
   */
  parkable?: boolean
  /** Home sticker pick: do not restore/park from grid clicks. */
  pausePointer?: boolean
  /** 编辑教程只在小组件库那一步拉开。 */
  tourDockPose?: HomeEditTourDockPose
}

export default function WidgetLibraryIsland({
  visible,
  availableWidgets,
  onNewWidgetDragStart,
  layoutMode,
  parkable = true,
  pausePointer = false,
  tourDockPose,
}: WidgetLibraryIslandProps) {
  const { t } = useI18n()
  const anim = useAnimationLevel()
  const widgetDragActive = useWidgetDragActive()
  const { width: windowWidth, height: windowHeight } = useDebouncedWindowSize(150)
  const dock = useLibraryDockStage({
    parkable,
    visible,
    widgetDragActive,
    windowWidth,
    windowHeight,
    reducedMotion: isExlight(anim),
    pausePointer,
    tourDockPose,
  })
  const {
    parked: libraryParked,
    stageHovered,
    parkMotionDone,
    parkedBeforeDragRef,
    restX: dockRestX,
    stageMotion: dockStageMotion,
    islandBoxStyle: dockIslandBoxStyle,
    restore: restoreLibraryDock,
    setStageHovered,
    consumeEscape,
    onIslandAnimationComplete,
  } = dock

  const layoutModeRef = useRef(layoutMode)
  const layoutModeSwitched = layoutModeRef.current !== layoutMode
  layoutModeRef.current = layoutMode

  useEffect(() => {
    if (!visible) return
    void preloadBuiltinWidgets(availableWidgets.map((widget) => widget.id)).catch(
      () => {},
    )
  }, [availableWidgets, visible])

  useEffect(() => {
    if (!visible || !parkable) return
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      if (!consumeEscape()) return
      event.preventDefault()
      event.stopPropagation()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [consumeEscape, parkable, visible])

  const libraryScrollRef = useRef<HTMLDivElement>(null)
  const [librarySearchQuery, setLibrarySearchQuery] = useState('')
  const [libraryKindFilter, setLibraryKindFilter] =
    useState<WidgetLibraryKindFilter>('all')

  useEffect(() => {
    if (visible) return
    setLibrarySearchQuery('')
    setLibraryKindFilter('all')
  }, [visible])

  const libraryCatalog = useMemo(
    () =>
      availableWidgets.map((widgetType) => ({
        widgetType,
        source: widgetLibraryKindSource(widgetType),
      })),
    [availableWidgets],
  )
  const librarySidebarKinds = useMemo(
    () =>
      presentWidgetLibraryKindFilters(
        libraryCatalog.map((entry) => entry.source),
      ),
    [libraryCatalog],
  )
  const activeLibraryKind: WidgetLibraryKindFilter =
    librarySidebarKinds.includes(libraryKindFilter) ? libraryKindFilter : 'all'

  const libraryWidgets = useMemo(() => {
    const widgetsI18n = t.widgets as Record<string, unknown>
    const matchesSearch = (widgetType: WidgetType) =>
      widgetTypeMatchesLibrarySearch(librarySearchQuery, {
        id: widgetType.id,
        name: widgetType.name,
        label: widgetDisplayLabel(widgetType, widgetsI18n),
        extras: widgetSearchExtras(widgetType),
      })
    return libraryCatalog
      .filter(
        (entry) =>
          widgetMatchesLibraryKind(activeLibraryKind, entry.source) &&
          matchesSearch(entry.widgetType),
      )
      .map((entry) => entry.widgetType)
  }, [activeLibraryKind, libraryCatalog, librarySearchQuery, t.widgets])

  useEffect(() => {
    const el = libraryScrollRef.current
    if (!el) return
    el.scrollTop = 0
  }, [librarySearchQuery, activeLibraryKind])

  const libraryDockScale = libraryDockPreviewDisplayScale(
    standardHomeCellSize(
      typeof window !== 'undefined' ? window.innerWidth : windowWidth,
    ),
  )

  const libraryTiles = useMemo(
    () =>
      libraryWidgets.map((widgetType) => (
        <WidgetLibraryTile
          key={widgetType.id}
          widgetType={widgetType}
          displayScale={libraryDockScale}
          scrollRef={libraryScrollRef}
          widgetsI18n={t.widgets as Record<string, unknown>}
          onDragStart={onNewWidgetDragStart}
        />
      )),
    [libraryDockScale, libraryWidgets, onNewWidgetDragStart, t.widgets],
  )

  const libraryIslandClassName = `widget-library-island${
    libraryParked ? ' is-staged' : ''
  }`

  const parkedMotion = {
    x: dockStageMotion.x,
    y: dockStageMotion.y,
    scale: dockStageMotion.scale,
    rotateX: dockStageMotion.rotateX,
    rotateY: dockStageMotion.rotateY,
    z: 0,
  }

  return createPortal(
    <AnimatePresence>
      {visible ? (
        <motion.div
          key="widget-library-island"
          className={`widget-library-stage-root${
            widgetDragActive ? ' is-concealed' : ''
          }`}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{
            opacity: 0,
            transition: { duration: 0.2, ease: [0.4, 0, 1, 1] },
          }}
          transition={{ duration: 0.26, ease: [0.25, 0.1, 0.25, 1] }}
        >
          <div
            className="widget-library-stage-scene"
            style={
              parkable
                ? { perspective: LIBRARY_DOCK_STAGE_PERSPECTIVE }
                : undefined
            }
          >
            <motion.div
              key="widget-library-window"
              {...{ [LIBRARY_DOCK_CHROME_ATTR]: '' }}
              initial={
                libraryParked
                  ? parkedMotion
                  : {
                      y: 24,
                      x: dockRestX,
                      scale: 1,
                      rotateX: 0,
                      rotateY: 0,
                    }
              }
              animate={
                libraryParked
                  ? {
                      ...parkedMotion,
                      scale:
                        stageHovered && !widgetDragActive
                          ? LIBRARY_DOCK_STAGE_SCALE_HOVER
                          : dockStageMotion.scale,
                      opacity: 1,
                      transition:
                        widgetDragActive && parkedBeforeDragRef.current
                          ? { duration: 0 }
                          : layoutModeSwitched
                            ? { duration: 0 }
                            : stageHovered
                              ? { duration: 0.28, ease: LIBRARY_DOCK_PARK_EASE }
                              : LIBRARY_DOCK_PARK_TRANSITION,
                    }
                  : widgetDragActive && !parkable
                    ? {
                        x: dockRestX,
                        y: 16,
                        scale: 1,
                        rotateX: 0,
                        rotateY: 0,
                        z: 0,
                        opacity: 0,
                        transition: {
                          y: { duration: 0.2, ease: [0.4, 0, 1, 1] },
                          opacity: { duration: 0.16, ease: [0.4, 0, 1, 1] },
                        },
                      }
                    : {
                        x: dockRestX,
                        y: 0,
                        scale: 1,
                        rotateX: 0,
                        rotateY: 0,
                        z: 0,
                        opacity: 1,
                        transition: layoutModeSwitched
                          ? { duration: 0 }
                          : LIBRARY_DOCK_PARK_TRANSITION,
                      }
              }
              transformTemplate={(latest: Record<string, unknown>) =>
                libraryDockStageTransform(
                  latest as Parameters<typeof libraryDockStageTransform>[0],
                )
              }
              className={libraryIslandClassName}
              data-tour="home-widget-library"
              data-library-stage={libraryParked ? 'parked' : undefined}
              onAnimationComplete={onIslandAnimationComplete}
              style={{
                ...dockIslandBoxStyle,
                pointerEvents:
                  widgetDragActive || libraryParked ? 'none' : undefined,
                originX: LIBRARY_DOCK_STAGE_ORIGIN_X,
                originY: LIBRARY_DOCK_STAGE_ORIGIN_Y,
              }}
            >
              <div className="widget-library-title">
                <img
                  src="/icons/widgets/library.webp"
                  alt=""
                  aria-hidden="true"
                  draggable={false}
                  decoding="async"
                />
                <span>{t.widgetGrid.widgetLibrary}</span>
              </div>
              <div className="widget-library-gallery">
                <aside className="widget-library-sidebar">
                  <LibrarySearchField
                    query={librarySearchQuery}
                    onQueryChange={setLibrarySearchQuery}
                    searchLabel={t.widgetGrid.searchWidgets}
                    clearLabel={t.widgetGrid.clearSearch}
                  />
                  <nav
                    className="widget-library-nav"
                    aria-label={t.widgetGrid.filterWidgets}
                  >
                    {librarySidebarKinds.map((id) => (
                      <button
                        key={id}
                        type="button"
                        className={`widget-library-nav-item${
                          activeLibraryKind === id ? ' is-active' : ''
                        }`}
                        onClick={() => setLibraryKindFilter(id)}
                      >
                        {libraryFilterLabel(id, t)}
                      </button>
                    ))}
                  </nav>
                </aside>
                <div
                  ref={libraryScrollRef}
                  className="widget-library-body scrollbar-hide"
                >
                  {libraryWidgets.length === 0 ? (
                    <div className="widget-library-empty">
                      {t.widgetGrid.noSearchResults}
                    </div>
                  ) : (
                    libraryTiles
                  )}
                </div>
              </div>
            </motion.div>
          </div>
          {libraryParked && parkMotionDone && tourDockPose !== 'parked' ? (
            <button
              type="button"
              className="widget-library-stage-hit"
              {...{ [LIBRARY_DOCK_CHROME_ATTR]: '' }}
              style={{
                left: dockStageMotion.parkLeft,
                width: dockStageMotion.visualWidth,
                height: dockStageMotion.visualHeight,
              }}
              onPointerEnter={() => setStageHovered(true)}
              onPointerLeave={() => setStageHovered(false)}
              onClick={restoreLibraryDock}
              aria-label={t.widgetGrid.restoreWidgetLibrary}
            />
          ) : null}
          {libraryParked ? (
            <button
              type="button"
              className="widget-library-stage-badge"
              {...{ [LIBRARY_DOCK_CHROME_ATTR]: '' }}
              style={{
                left: dockStageMotion.badgeLeft,
                top: `calc(50% + ${dockStageMotion.badgeTopOffset}px)`,
              }}
              onPointerEnter={() => setStageHovered(true)}
              onPointerLeave={() => setStageHovered(false)}
              onClick={
                tourDockPose === 'parked' ? undefined : restoreLibraryDock
              }
              aria-label={t.widgetGrid.restoreWidgetLibrary}
            >
              <img
                src="/icons/widgets/library.webp"
                alt=""
                aria-hidden="true"
                draggable={false}
                decoding="async"
              />
            </button>
          ) : null}
        </motion.div>
      ) : null}
    </AnimatePresence>,
    document.body,
  )
}
