/**
 * Dock library Stage Manager: park / restore + motion values.
 * Pure geometry lives in `utils/libraryDockStage.ts`.
 * `parkable: false` still sizes the dock window but never stages it.
 */

import type { CSSProperties } from 'react'
import type { HomeEditTourDockPose } from '../components/tour/tourLogic'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import {
  LIBRARY_BESIDE_PANEL_ANCHOR,
  LIBRARY_DOCK_BOTTOM_REM,
  LIBRARY_DOCK_POINTER_CHROME,
  LIBRARY_DOCK_RESTORE_BLOCK,
  LIBRARY_DOCK_RESTORE_SUPPRESS_MS,
  LIBRARY_DOCK_STAGE_ORIGIN_X,
  LIBRARY_DOCK_STAGE_ORIGIN_Y,
  LIBRARY_DOCK_STAGE_ROTATE_X,
  LIBRARY_DOCK_STAGE_ROTATE_Y,
  LIBRARY_DOCK_STAGE_SCALE,
  LIBRARY_DOCK_STAGE_SCALE_HOVER,
  libraryBesidePanelBox,
  libraryDockIslandBoxStyle,
  libraryDockIslandSize,
  libraryDockStageBadgePos,
  libraryDockStageLeft,
  libraryDockStageOffset,
  libraryDockStageVisualSize,
} from '../utils/libraryDockStage'

function usePanelAnchorRect(
  enabled: boolean,
  windowWidth: number,
  windowHeight: number,
): { top: number; left: number } | null {
  const [rect, setRect] = useState<{ top: number; left: number } | null>(null)

  useLayoutEffect(() => {
    if (!enabled) {
      setRect(null)
      return
    }
    const node = document.querySelector(LIBRARY_BESIDE_PANEL_ANCHOR)
    if (!(node instanceof HTMLElement)) {
      setRect(null)
      return
    }
    const update = () => {
      const next = node.getBoundingClientRect()
      setRect({ top: next.top, left: next.left })
    }
    update()
    const observer = new ResizeObserver(update)
    observer.observe(node)
    return () => observer.disconnect()
  }, [enabled, windowWidth, windowHeight])

  return rect
}

export interface LibraryDockStageMotion {
  x: number
  y: number
  scale: number
  rotateX: number
  rotateY: number
  originX: number
  originY: number
  parkLeft: number
  visualWidth: number
  visualHeight: number
  badgeLeft: number
  badgeTopOffset: number
}

const IDLE_MOTION: LibraryDockStageMotion = {
  x: 0,
  y: 0,
  scale: 1,
  rotateX: 0,
  rotateY: 0,
  originX: LIBRARY_DOCK_STAGE_ORIGIN_X,
  originY: LIBRARY_DOCK_STAGE_ORIGIN_Y,
  parkLeft: 0,
  visualWidth: 0,
  visualHeight: 0,
  badgeLeft: 0,
  badgeTopOffset: 0,
}

export function useLibraryDockStage(input: {
  parkable: boolean
  visible: boolean
  widgetDragActive: boolean
  windowWidth: number
  windowHeight: number
  reducedMotion: boolean
  /** Sticker cell-pick: keep the dock parked and ignore grid clicks. */
  pausePointer?: boolean
  /** 编辑教程：只在小组件库那一步拉开，其余步骤停靠且不让点击改姿态。 */
  tourDockPose?: HomeEditTourDockPose
}) {
  const {
    parkable,
    visible,
    widgetDragActive,
    windowWidth,
    windowHeight,
    reducedMotion,
    pausePointer = false,
    tourDockPose,
  } = input

  const keepParkedRef = useRef(parkable)
  const stagedRef = useRef(false)
  const suppressRestoreUntilRef = useRef(0)
  const parkedBeforeDragRef = useRef(false)
  const parkIgnorePointerRef = useRef<number | null>(null)

  const [staged, setStaged] = useState(parkable)
  const [stageHovered, setStageHovered] = useState(false)
  const [parkMotionDone, setParkMotionDone] = useState(false)

  const parked = parkable && staged
  stagedRef.current = staged
  const panelRect = usePanelAnchorRect(
    !parkable && visible,
    windowWidth,
    windowHeight,
  )

  const rootFontSize = useMemo(() => {
    if (typeof document === 'undefined') return 16
    return (
      Number.parseFloat(getComputedStyle(document.documentElement).fontSize) ||
      16
    )
  }, [windowWidth])

  const islandSize = useMemo(
    () => libraryDockIslandSize(windowWidth, windowHeight, rootFontSize),
    [windowWidth, windowHeight, rootFontSize],
  )

  const stageMotion = useMemo((): LibraryDockStageMotion => {
    if (!parkable) return IDLE_MOTION
    const offset = libraryDockStageOffset({
      viewportWidth: windowWidth,
      viewportHeight: windowHeight,
      islandWidth: islandSize.width,
      islandHeight: islandSize.height,
      dockBottom: LIBRARY_DOCK_BOTTOM_REM * rootFontSize,
      rootFontSize,
    })
    const scale = reducedMotion
      ? LIBRARY_DOCK_STAGE_SCALE_HOVER
      : LIBRARY_DOCK_STAGE_SCALE
    const rotateX = reducedMotion ? 0 : LIBRARY_DOCK_STAGE_ROTATE_X
    const rotateY = reducedMotion ? 0 : LIBRARY_DOCK_STAGE_ROTATE_Y
    const badge = libraryDockStageBadgePos({
      islandHeight: islandSize.height,
      scale,
      rotateXDeg: rotateX,
      rootFontSize,
    })
    const visual = libraryDockStageVisualSize({
      islandWidth: islandSize.width,
      islandHeight: islandSize.height,
      scale,
      rotateYDeg: rotateY,
      rotateXDeg: rotateX,
    })
    return {
      x: offset.x,
      y: offset.y,
      scale,
      rotateX,
      rotateY,
      originX: LIBRARY_DOCK_STAGE_ORIGIN_X,
      originY: LIBRARY_DOCK_STAGE_ORIGIN_Y,
      parkLeft: libraryDockStageLeft(rootFontSize),
      visualWidth: visual.width,
      visualHeight: visual.height,
      badgeLeft: badge.left,
      badgeTopOffset: badge.topOffset,
    }
  }, [
    parkable,
    reducedMotion,
    windowWidth,
    windowHeight,
    islandSize.width,
    islandSize.height,
    rootFontSize,
  ])

  const restX = parkable ? -islandSize.width / 2 : 0

  const islandBoxStyle = useMemo(
    (): CSSProperties =>
      parkable
        ? libraryDockIslandBoxStyle(islandSize, rootFontSize)
        : libraryBesidePanelBox({
            preferred: islandSize,
            viewportWidth: windowWidth,
            viewportHeight: windowHeight,
            panel: panelRect,
            rootFontSize,
          }),
    [
      islandSize,
      panelRect,
      parkable,
      rootFontSize,
      windowHeight,
      windowWidth,
    ],
  )

  const park = useCallback(() => {
    if (!parkable) return
    keepParkedRef.current = true
    suppressRestoreUntilRef.current =
      performance.now() + LIBRARY_DOCK_RESTORE_SUPPRESS_MS
    if (stagedRef.current) return
    setStaged(true)
  }, [parkable])

  const restore = useCallback(() => {
    if (!parkable) return
    if (widgetDragActive) return
    if (parkIgnorePointerRef.current != null) return
    if (performance.now() < suppressRestoreUntilRef.current) return
    keepParkedRef.current = false
    setStageHovered(false)
    setParkMotionDone(false)
    setStaged(false)
  }, [parkable, widgetDragActive])

  const consumeEscape = useCallback(() => {
    if (tourDockPose) return false
    if (!parkable || !staged) return false
    restore()
    return true
  }, [parkable, restore, staged, tourDockPose])

  const onIslandAnimationComplete = useCallback(() => {
    if (stagedRef.current) setParkMotionDone(true)
  }, [])

  useEffect(() => {
    if (visible) return
    keepParkedRef.current = parkable
    setStaged(parkable)
    setStageHovered(false)
    setParkMotionDone(false)
  }, [parkable, visible])

  useLayoutEffect(() => {
    if (!parkable || !visible) return
    keepParkedRef.current = true
    if (stagedRef.current) setParkMotionDone(true)
  }, [parkable, visible])

  useEffect(() => {
    if (!parkable || !visible || !widgetDragActive) return
    parkedBeforeDragRef.current = stagedRef.current
    park()
  }, [park, parkable, visible, widgetDragActive])

  useEffect(() => {
    if (parked) return
    setParkMotionDone(false)
  }, [parked])

  useEffect(() => {
    if (widgetDragActive || !parkable || !visible) return
    if (!keepParkedRef.current) setStaged(false)
  }, [parkable, visible, widgetDragActive])

  useEffect(() => {
    if (!pausePointer || !parkable || !visible) return
    park()
  }, [pausePointer, park, parkable, visible])

  const lastTourDockPoseRef = useRef(tourDockPose)
  useLayoutEffect(() => {
    const previousPose = lastTourDockPoseRef.current
    lastTourDockPoseRef.current = tourDockPose
    if (!parkable || !visible) return
    if (tourDockPose === 'restored') {
      if (pausePointer) return
      keepParkedRef.current = false
      suppressRestoreUntilRef.current = 0
      setStageHovered(false)
      setParkMotionDone(false)
      if (!stagedRef.current) return
      setStaged(false)
      return
    }
    if (tourDockPose === 'parked' || previousPose === 'restored') {
      park()
    }
  }, [park, parkable, pausePointer, tourDockPose, visible])

  useEffect(() => {
    if (!parkable || !visible) return

    const onPointerDown = (event: PointerEvent) => {
      if (tourDockPose) return
      if (pausePointer) return
      if (event.pointerType === 'mouse' && event.button !== 0) return
      const target = event.target
      if (!(target instanceof Element)) return
      if (target.closest(LIBRARY_DOCK_POINTER_CHROME)) return
      if (staged) {
        if (target.closest(LIBRARY_DOCK_RESTORE_BLOCK)) return
        restore()
        return
      }
      parkIgnorePointerRef.current = event.pointerId
      park()
    }

    const onPointerUp = (event: PointerEvent) => {
      if (event.pointerId !== parkIgnorePointerRef.current) return
      window.setTimeout(() => {
        if (parkIgnorePointerRef.current === event.pointerId) {
          parkIgnorePointerRef.current = null
        }
      }, 0)
    }

    window.addEventListener('pointerdown', onPointerDown, true)
    window.addEventListener('pointerup', onPointerUp, true)
    window.addEventListener('pointercancel', onPointerUp, true)
    return () => {
      window.removeEventListener('pointerdown', onPointerDown, true)
      window.removeEventListener('pointerup', onPointerUp, true)
      window.removeEventListener('pointercancel', onPointerUp, true)
    }
  }, [park, parkable, pausePointer, restore, staged, tourDockPose, visible])

  return {
    parked,
    stageHovered,
    parkMotionDone,
    parkedBeforeDragRef,
    restX,
    stageMotion,
    islandBoxStyle,
    restore,
    setStageHovered,
    consumeEscape,
    onIslandAnimationComplete,
  }
}
