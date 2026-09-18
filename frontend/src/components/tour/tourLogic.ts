import type { GuideRect } from '../settings/settingTitleGuideLogic'
import type {
  TourAudience,
  TourDefinition,
  TourStepAction,
  TourStepDef,
  TourSurfacePick,
} from './tourTypes'
import {
  CONTROL_PANEL_COLLAPSED_RADIUS_REM,
  CONTROL_PANEL_EXPANDED_RADIUS_REM,
  predictCollapsedControlPanelBox,
  predictExpandedControlPanelBox,
  predictRestoredLibraryDockBox,
} from '../../utils/libraryDockStage'
import { getAgentPanelVisible } from '../agent-panel/agentPanelVisible'
import { isAgentSettingsPath } from '../agent/settings/agentSettingsPath'
import { isJournalAppPath } from '../phantasi/logic/journalRoutes'
import { expandCollapsibleAncestors } from '../settings/guides/guideAnchor'
import { clamp } from '../settings/settingTitleGuideLogic'

export const TOUR_HOLE_PAD = 8
export const TOUR_VIEWPORT_PAD = 16
export const TOUR_CARD_GAP = 14
export {
  TOUR_ACTIVE_ATTR,
  TOUR_ACTIVE_EVENT,
  isTourDomActive,
  setTourDomActive,
} from './tourDom'
export {
  homeBrowseTourPanelPose,
  homeEditTourDockPose,
  isHomeEditSurface,
  setHomeEditSurface,
  subscribeHomeEditSurface,
  type HomeBrowseTourPanelPose,
  type HomeEditTourDockPose,
} from './tourHomePose'

export type ConfigTourSurface = 'browse' | 'persona' | 'ai-persona' | 'none'

let configTourSurface: ConfigTourSurface =
  typeof window === 'undefined' ? 'browse' : readConfigTourSurfaceFromLocation()
const configTourListeners = new Set<() => void>()

function readConfigTourSurfaceFromLocation(): ConfigTourSurface {
  if (typeof window === 'undefined') return 'browse'
  const path =
    typeof window === 'undefined'
      ? '/'
      : window.location.pathname.replace(/\/+$/, '') || '/'
  const params = new URLSearchParams(window.location.search)
  const page = params.get('page')
  if (isAgentSettingsPath(path)) {
    if (page === 'merope-setup') return 'none'
    if (page === 'merope') return 'persona'
    return 'ai-persona'
  }
  return 'browse'
}

export function getConfigTourSurface(): ConfigTourSurface {
  return configTourSurface
}

export function subscribeConfigTourSurface(
  onStoreChange: () => void,
): () => void {
  configTourListeners.add(onStoreChange)
  return () => {
    configTourListeners.delete(onStoreChange)
  }
}

export function refreshConfigTourSurface(): void {
  const next = readConfigTourSurfaceFromLocation()
  if (configTourSurface === next) return
  configTourSurface = next
  configTourListeners.forEach((listener) => listener())
}

export function readTourSurface(
  editingHome: boolean,
  pathname?: string,
): TourSurfacePick {
  if (editingHome) return 'edit'
  const path = normalizeTourPath(
    pathname ??
      (typeof window === 'undefined' ? '/' : window.location.pathname),
  )
  if (path === '/library') {
    return libraryTourPickSurface(getLibraryTourSurfaceSnapshot())
  }
  if (isAgentSettingsPath(path)) return configTourSurface
  if (path !== '/config') return 'browse'
  return configTourSurface
}

export function isLibraryCanvasTourSurface(): boolean {
  return getLibraryTourSurfaceSnapshot() === 'canvas'
}

export type LibraryTourSurface = 'list' | 'canvas' | 'empty' | 'pending'

/** List and canvas tours are separate. Unresolved or empty canvas must not pick the waterfall tour. */
export function libraryTourPickSurface(
  library: LibraryTourSurface,
): TourSurfacePick {
  if (library === 'canvas') return 'canvas'
  if (library === 'list') return 'browse'
  return 'none'
}

export function libraryTourSurfaceFromFlags(flags: {
  preference: boolean
  surface: boolean
  empty: boolean
  resolved?: boolean
}): LibraryTourSurface {
  if (flags.resolved === false) return 'pending'
  if (flags.surface) return 'canvas'
  if (flags.preference) return flags.empty ? 'empty' : 'pending'
  return 'list'
}

function readLibraryCanvasTourFlags(): {
  preference: boolean
  surface: boolean
  empty: boolean
  resolved?: boolean
} {
  if (typeof document === 'undefined') {
    return { preference: false, surface: false, empty: false }
  }
  const root = document.documentElement
  return {
    preference: root.dataset.libraryCanvas === 'active',
    surface: root.dataset.libraryCanvasSurface === '1',
    empty: root.dataset.libraryEmpty === '1',
    resolved: root.dataset.libraryLayoutResolved === '1',
  }
}

export function getLibraryTourSurfaceSnapshot(): LibraryTourSurface {
  return libraryTourSurfaceFromFlags(readLibraryCanvasTourFlags())
}

const TOUR_STATIC_HOST_ANCHORS = new Set(['library-grid'])

export function tourMeasureWatchesHost(anchor: string): boolean {
  return !TOUR_STATIC_HOST_ANCHORS.has(anchor)
}

export function shouldAbortLibraryTour(
  tourId: string,
  librarySurface: LibraryTourSurface,
): boolean {
  if (tourId === 'library-visitor' || tourId === 'library-owner') {
    return librarySurface !== 'list'
  }
  if (
    tourId === 'library-canvas-visitor' ||
    tourId === 'library-canvas-owner'
  ) {
    return librarySurface !== 'canvas'
  }
  return false
}

export function tourMeasureWatchesScroll(
  canvasSurface = typeof document !== 'undefined' &&
    document.documentElement.dataset.libraryCanvasSurface === '1',
): boolean {
  return !canvasSurface
}

export function subscribeLibraryCanvasTourSurface(
  onStoreChange: () => void,
): () => void {
  if (typeof window === 'undefined') return () => {}
  window.addEventListener('libraryCanvasModeChanged', onStoreChange)
  return () => {
    window.removeEventListener('libraryCanvasModeChanged', onStoreChange)
  }
}

export function pageNameForPath(
  pathname: string,
  nav: {
    home: string
    library: string
    phantasi: string
    reports: string
    tapp: string
    config: string
    agent: string
  },
  editMode: string,
  editingHome: boolean,
): string {
  const path = pathname.replaceAll(/\/+$/g, '') || '/'
  if ((path === '/' || path === '') && editingHome) return editMode
  if (path === '/library' || path.startsWith('/library')) return nav.library
  if (isJournalAppPath(path)) {
    return nav.phantasi
  }
  if (path === '/reports' || path.startsWith('/reports')) return nav.reports
  if (path === '/tapp' || path.startsWith('/tapp')) return nav.tapp
  if (path === '/agent/settings' || path.startsWith('/agent/settings')) {
    return nav.agent
  }
  if (path === '/config' || path.startsWith('/config')) return nav.config
  return nav.home
}

export type PersonaTourPanel = 'overview' | 'persona' | 'wardrobe' | 'motion'

export function personaTourPanel(
  tourId: string | null,
  stepId: string | null,
): PersonaTourPanel | undefined {
  if (tourId !== 'config-persona-owner') return undefined
  switch (stepId) {
    case 'config-persona-identity':
      return 'persona'
    case 'config-persona-wardrobe':
      return 'wardrobe'
    case 'config-persona-motion':
      return 'motion'
    default:
      return stepId ? 'overview' : undefined
  }
}

export function rootFontSizePx(): number {
  if (typeof document === 'undefined') return 16
  return (
    Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
  )
}

export function predictedLibraryDockTourBox(
  viewportWidth: number,
  viewportHeight: number,
  rootFontSize = rootFontSizePx(),
): Box {
  return predictRestoredLibraryDockBox(
    viewportWidth,
    viewportHeight,
    rootFontSize,
  )
}

let lastControlPanelContentHeight = 0

export function readControlPanelContentHeight(
  node?: HTMLElement | null,
): number {
  if (typeof document === 'undefined') return lastControlPanelContentHeight
  const el = node ?? queryTourAnchor('control-panel')
  const raw = el?.scrollHeight ?? 0
  if (raw > 0) lastControlPanelContentHeight = raw
  return lastControlPanelContentHeight
}

export function predictedControlPanelTourBox(
  viewportWidth: number,
  contentHeight: number,
  rootFontSize = rootFontSizePx(),
): Box {
  return predictExpandedControlPanelBox(
    viewportWidth,
    contentHeight,
    rootFontSize,
  )
}

export function predictedControlPanelHoleRadius(
  rootFontSize = rootFontSizePx(),
): number {
  const rem = rootFontSize > 0 ? rootFontSize : 16
  return CONTROL_PANEL_EXPANDED_RADIUS_REM * rem
}

export function predictedControlIslandTourBox(
  viewportWidth: number,
  rootFontSize = rootFontSizePx(),
): Box {
  return predictCollapsedControlPanelBox(viewportWidth, rootFontSize)
}

export function predictedControlIslandHoleRadius(
  rootFontSize = rootFontSizePx(),
): number {
  const rem = rootFontSize > 0 ? rootFontSize : 16
  return CONTROL_PANEL_COLLAPSED_RADIUS_REM * rem
}

export function isPredictedTourAnchor(
  anchor: string,
  stepId?: string,
): boolean {
  return (
    anchor === 'control-panel' ||
    anchor === 'control-island' ||
    anchor === 'home-widget-library' ||
    stepId === 'home-widget-library'
  )
}

export function tourHoleSync(
  step: { id: string; anchor: string } | null,
): 'dock' | 'panel' | undefined {
  if (!step) return undefined
  if (
    step.id === 'home-widget-library' ||
    step.anchor === 'home-widget-library'
  ) {
    return 'dock'
  }
  if (step.anchor === 'control-panel') {
    return 'panel'
  }
  return undefined
}

export function sameTourHole(
  a: { top: number; left: number; width: number; height: number; radius: number },
  b: { top: number; left: number; width: number; height: number; radius: number },
  epsilon = 0.5,
): boolean {
  return (
    Math.abs(a.top - b.top) < epsilon &&
    Math.abs(a.left - b.left) < epsilon &&
    Math.abs(a.width - b.width) < epsilon &&
    Math.abs(a.height - b.height) < epsilon &&
    Math.abs(a.radius - b.radius) < epsilon
  )
}

export function sameTourCardPos(
  a: TourCardPos,
  b: TourCardPos,
  epsilon = 0.5,
): boolean {
  return (
    a.placement === b.placement &&
    Math.abs(a.top - b.top) < epsilon &&
    Math.abs(a.left - b.left) < epsilon
  )
}

export function fillTourHint(
  template: string,
  key: string,
  value: string | undefined | null,
): string {
  const next = value?.trim() ?? ''
  if (!next) return ''
  const filled = template.replaceAll(`{${key}}`, next)
  return filled.includes(`{${key}}`) ? '' : filled
}

export type TourPlacement = 'top' | 'bottom' | 'left' | 'right' | 'dock'

export interface TourCardPos {
  top: number
  left: number
  placement: TourPlacement
}

export interface Box {
  top: number
  left: number
  width: number
  height: number
}

export function inflateRect(
  box: Box,
  pad: number = TOUR_HOLE_PAD,
): GuideRect {
  const top = box.top - pad
  const left = box.left - pad
  const width = box.width + pad * 2
  const height = box.height + pad * 2
  return {
    top,
    left,
    width,
    height,
    right: left + width,
    bottom: top + height,
  }
}

export function intersectBoxes(a: Box, b: Box): Box | null {
  const top = Math.max(a.top, b.top)
  const left = Math.max(a.left, b.left)
  const right = Math.min(a.left + a.width, b.left + b.width)
  const bottom = Math.min(a.top + a.height, b.top + b.height)
  const width = right - left
  const height = bottom - top
  if (width < 1 || height < 1) return null
  return { top, left, width, height }
}

export function unionBoxes(boxes: readonly Box[]): Box | null {
  let top = Infinity
  let left = Infinity
  let right = -Infinity
  let bottom = -Infinity
  let any = false
  for (const box of boxes) {
    if (box.width < 1 || box.height < 1) continue
    any = true
    top = Math.min(top, box.top)
    left = Math.min(left, box.left)
    right = Math.max(right, box.left + box.width)
    bottom = Math.max(bottom, box.top + box.height)
  }
  if (!any) return null
  return { top, left, width: right - left, height: bottom - top }
}

export function isDegenerateBox(box: Box, min = 24): boolean {
  return box.width < min || box.height < min
}

export function fitTourUnion(boxes: readonly Box[], host: Box): Box | null {
  if (isDegenerateBox(host, 1)) {
    const union = unionBoxes(boxes)
    return union && !isDegenerateBox(union) ? union : null
  }
  const pieces: Box[] = []
  for (const box of boxes) {
    const clipped = intersectBoxes(box, host)
    if (clipped) pieces.push(clipped)
  }
  const union = unionBoxes(pieces)
  if (union && !isDegenerateBox(union)) return union
  return null
}

export function readTourBox(node: HTMLElement): Box {
  const r = node.getBoundingClientRect()
  const host = { top: r.top, left: r.left, width: r.width, height: r.height }
  const fit = node.getAttribute('data-tour-fit')
  if (fit) {
    const fitted = fitTourUnion(
      Iterator.from(node.querySelectorAll(fit))
        .map((el) => {
          const box = el.getBoundingClientRect()
          return {
            top: box.top,
            left: box.left,
            width: box.width,
            height: box.height,
          }
        })
        .toArray(),
      host,
    )
    if (fitted) return fitted
  }
  return host
}

export function filterVisibleSteps<T extends { anchor: string }>(
  steps: T[],
  hasAnchor: (anchor: string) => boolean,
): T[] {
  return steps.filter((step) => hasAnchor(step.anchor))
}

export function normalizeTourPath(pathname: string): string {
  if (!pathname) return '/'
  if (pathname.length > 1 && pathname.endsWith('/')) {
    const trimmed = pathname.replaceAll(/\/+$/g, '')
    return trimmed.length > 0 ? trimmed : '/'
  }
  return pathname
}

export function pickTour(
  tours: readonly TourDefinition[],
  pathname: string,
  isOwner: boolean,
  surface: TourSurfacePick = 'browse',
): TourDefinition | null {
  if (surface === 'none') return null
  const path = normalizeTourPath(pathname)
  const audience: TourAudience = isOwner ? 'owner' : 'visitor'
  const matchesSurface = (tour: TourDefinition) =>
    (tour.surface ?? 'browse') === surface
  return (
    tours.find(
      (tour) =>
        tour.route === path &&
        tour.audience === audience &&
        matchesSurface(tour),
    ) ??
    tours.find(
      (tour) =>
        tour.matchPrefix === true &&
        tour.audience === audience &&
        matchesSurface(tour) &&
        path.startsWith(`${tour.route}/`),
    ) ??
    null
  )
}

export function firstVisibleIndex(
  steps: readonly TourStepDef[],
  hasAnchor: (anchor: string, step: TourStepDef) => boolean,
  from = 0,
): number {
  for (let i = from; i < steps.length; i += 1) {
    const step = steps[i]!
    if (hasAnchor(step.anchor, step)) return i
  }
  return -1
}

export function previousVisibleIndex(
  steps: readonly TourStepDef[],
  hasAnchor: (anchor: string, step: TourStepDef) => boolean,
  from: number,
): number {
  for (let i = from - 1; i >= 0; i -= 1) {
    const step = steps[i]!
    if (hasAnchor(step.anchor, step)) return i
  }
  return -1
}

export function tourActionHostReady(action?: TourStepAction): boolean {
  if (action === 'open-agent') {
    return (
      typeof document !== 'undefined' &&
      document.querySelector('.agent-panel-longpress') != null
    )
  }
  return true
}

export function isTourActionSatisfied(
  step: Pick<TourStepDef, 'action'>,
  panelVisible: () => boolean = getAgentPanelVisible,
): boolean {
  if (step.action === 'open-agent') return panelVisible()
  return true
}

export function tourStepBlocksAdvance(
  step: Pick<TourStepDef, 'action'> | null | undefined,
  panelVisible: () => boolean = getAgentPanelVisible,
): boolean {
  if (!step?.action) return false
  return !isTourActionSatisfied(step, panelVisible)
}

export function isTourStepAvailable(step: TourStepDef): boolean {
  if (step.after) {
    if (!tourActionHostReady(step.after)) return false
    if (step.after === 'open-agent' && !getAgentPanelVisible()) return false
  }
  if (!tourActionHostReady(step.action)) return false
  return isTourAnchorMeasurable(step.anchor)
}

// 紧后一步还没挂上就等，不要跨过去。
export function nextIndexAfterTourAction(
  steps: readonly TourStepDef[],
  index: number,
  isAvailable: (step: TourStepDef) => boolean,
  hostReady: (action: TourStepAction) => boolean = tourActionHostReady,
): number | 'wait' {
  const current = steps[index]
  const immediate = steps[index + 1]
  if (!immediate) return -1
  if (isAvailable(immediate)) return index + 1
  if (current?.action && immediate.after === current.action) {
    if (!hostReady(current.action)) {
      return firstVisibleIndex(steps, (_anchor, step) => isAvailable(step), index + 2)
    }
    return 'wait'
  }
  return firstVisibleIndex(steps, (_anchor, step) => isAvailable(step), index + 1)
}

export const HOME_AGENT_PRESS_REM = 7.5

export interface HomeAgentPressInsets {
  left: number
  top: number
  right: number
  bottom: number
}

export function isHomeAgentActionStep(
  step: Pick<TourStepDef, 'id' | 'action'> | null | undefined,
): boolean {
  return step?.action === 'open-agent'
}

export function homeAgentPressInsets(
  rem: number,
  mobile: boolean,
): HomeAgentPressInsets {
  const edge = 1.25 * rem
  if (mobile) {
    return { left: edge, top: 4.75 * rem, right: edge, bottom: 5.5 * rem }
  }
  return { left: 5.5 * rem, top: 4.75 * rem, right: edge, bottom: edge }
}

function overlapArea(a: Box, b: Box): number {
  const left = Math.max(a.left, b.left)
  const top = Math.max(a.top, b.top)
  const right = Math.min(a.left + a.width, b.left + b.width)
  const bottom = Math.min(a.top + a.height, b.top + b.height)
  return Math.max(0, right - left) * Math.max(0, bottom - top)
}

export function pickHomeAgentPressBox(
  vw: number,
  vh: number,
  rem = 16,
  occupied: readonly Box[] = [],
  insets: HomeAgentPressInsets = homeAgentPressInsets(rem, false),
): Box {
  const size = HOME_AGENT_PRESS_REM * rem
  const minLeft = insets.left
  const minTop = insets.top
  const maxLeft = Math.max(minLeft, vw - insets.right - size)
  const maxTop = Math.max(minTop, vh - insets.bottom - size)
  const preferX = (minLeft + maxLeft) / 2
  const preferY = minTop + (maxTop - minTop) * 0.55
  const cols = 6
  const rows = 5
  let best: Box = { top: preferY, left: preferX, width: size, height: size }
  let bestScore = Number.POSITIVE_INFINITY
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < cols; col += 1) {
      const left = minLeft + ((maxLeft - minLeft) * col) / (cols - 1)
      const top = minTop + ((maxTop - minTop) * row) / (rows - 1)
      const box = { top, left, width: size, height: size }
      let overlap = 0
      for (const other of occupied) overlap += overlapArea(box, other)
      const cx = left + size / 2
      const cy = top + size / 2
      const score =
        overlap * 40 + Math.abs(cx - preferX - size / 2) + Math.abs(cy - preferY - size / 2)
      if (score < bestScore) {
        bestScore = score
        best = box
      }
    }
  }
  return best
}

export function readHomeAgentOccupiedBoxes(): Box[] {
  if (typeof document === 'undefined') return []
  const nodes = document.querySelectorAll<HTMLElement>(
    '.widget-grid-item, .nav-container, .global-control-bar, .control-bar-trigger',
  )
  const boxes: Box[] = []
  for (const node of nodes) {
    const r = node.getBoundingClientRect()
    const box = { top: r.top, left: r.left, width: r.width, height: r.height }
    if (!isDegenerateBox(box, 8)) boxes.push(box)
  }
  return boxes
}

export function visibleBoxArea(box: Box, vw: number, vh: number): number {
  const visW = Math.min(box.left + box.width, vw) - Math.max(box.left, 0)
  const visH = Math.min(box.top + box.height, vh) - Math.max(box.top, 0)
  return Math.max(0, visW) * Math.max(0, visH)
}

export function pickLargestVisible<T>(
  items: readonly T[],
  getBox: (item: T) => Box,
  vw: number,
  vh: number,
): T | undefined {
  let best: T | undefined
  let bestArea = -1
  for (const item of items) {
    const area = visibleBoxArea(getBox(item), vw, vh)
    if (area > bestArea) {
      bestArea = area
      best = item
    }
  }
  return best
}

// 沉浸导航/收起槽/隐藏节点即使有锚也不能量。
export function isTourAnchorEligible(node: HTMLElement): boolean {
  if (node.closest('.nav-container.immersive')) return false
  if (node.closest('[inert]')) return false
  if (node.closest('[hidden]')) return false
  const hiddenRoot = node.closest('[aria-hidden]')
  return hiddenRoot?.getAttribute('aria-hidden') !== 'true'
}

export function queryTourAnchor(anchor: string): HTMLElement | null {
  if (typeof document === 'undefined') return null
  const escaped =
    typeof CSS !== 'undefined' && typeof CSS.escape === 'function'
      ? CSS.escape(anchor)
      : anchor
  const nodes = Iterator.from(
    document.querySelectorAll<HTMLElement>(`[data-tour="${escaped}"]`),
  )
    .filter(isTourAnchorEligible)
    .toArray()
  if (nodes.length <= 1) return nodes[0] ?? null
  return (
    pickLargestVisible(
      nodes,
      (node) => {
        const r = node.getBoundingClientRect()
        return { top: r.top, left: r.left, width: r.width, height: r.height }
      },
      window.innerWidth,
      window.innerHeight,
    ) ?? null
  )
}

// 锚在但量出来是空盒（display:contents / 未布局）时不能开步。
export function isTourAnchorMeasurable(anchor: string): boolean {
  const node = queryTourAnchor(anchor)
  if (!node) return false
  return !isDegenerateBox(readTourBox(resolveTourMeasureNode(anchor, node)))
}

export const LIBRARY_FILTER_EXPAND_WAIT_MS = 900
export const TOUR_ANCHOR_POLL_MS = 50

export function waitForTourAnchor(
  anchor: string,
  timeoutMs: number,
  now: () => number = () =>
    typeof performance !== 'undefined' ? performance.now() : Date.now(),
  enqueue: (cb: () => void) => void = (cb) => {
    setTimeout(cb, TOUR_ANCHOR_POLL_MS)
  },
): Promise<boolean> {
  return new Promise((resolve) => {
    const deadline = now() + timeoutMs
    const tick = () => {
      if (isTourAnchorMeasurable(anchor)) {
        resolve(true)
        return
      }
      if (now() >= deadline) {
        resolve(false)
        return
      }
      enqueue(tick)
    }
    tick()
  })
}

export function boxRight(box: Box): number {
  return box.left + box.width
}

export function boxBottom(box: Box): number {
  return box.top + box.height
}

export function tourAnchorNeedsReveal(
  box: Box,
  viewportW: number,
  viewportH: number,
  pad = TOUR_VIEWPORT_PAD,
): boolean {
  return (
    boxBottom(box) < pad ||
    box.top > viewportH - pad ||
    boxRight(box) < pad ||
    box.left > viewportW - pad
  )
}

export function tourAnchorScrollIsTrapped(node: HTMLElement): boolean {
  if (node.closest('[data-library-canvas-surface="true"]')) return true
  let cur: HTMLElement | null = node.parentElement
  while (cur) {
    const style = getComputedStyle(cur)
    const clipped =
      style.overflow === 'hidden' ||
      style.overflow === 'clip' ||
      style.overflowX === 'hidden' ||
      style.overflowX === 'clip' ||
      style.overflowY === 'hidden' ||
      style.overflowY === 'clip'
    const layered =
      style.position === 'fixed' ||
      style.position === 'sticky' ||
      (style.transform !== 'none' && style.transform !== '')
    if (clipped && layered) return true
    cur = cur.parentElement
  }
  return false
}

export function revealTourAnchor(anchor: string, stepId?: string): boolean {
  if (typeof document === 'undefined') return false
  if (isPredictedTourAnchor(anchor, stepId)) return false
  const node = queryTourAnchor(anchor)
  if (!node) return false
  if (tourAnchorScrollIsTrapped(node)) return true
  expandCollapsibleAncestors(node)
  const rect = node.getBoundingClientRect()
  const box = {
    top: rect.top,
    left: rect.left,
    width: rect.width,
    height: rect.height,
  }
  if (!tourAnchorNeedsReveal(box, window.innerWidth, window.innerHeight)) {
    return true
  }
  node.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'auto' })
  return true
}

export function holePadForBox(box: Box): number {
  const minSide = Math.min(box.width, box.height)
  if (minSide >= 280) return 6
  if (minSide <= 48) return 10
  return TOUR_HOLE_PAD
}

export function resolveTourMeasureNode(
  anchor: string,
  node: HTMLElement,
): HTMLElement {
  if (anchor !== 'control-panel') return node
  return node.closest('.control-bar-trigger') ?? node
}

export function holePadForTourAnchor(anchor: string, box: Box): number {
  if (anchor === 'control-panel' || anchor === 'home-agent') return 0
  return holePadForBox(box)
}

export function holeRadiusFor(rawRadius: number, hole: Box): number {
  const cap = Math.min(hole.width, hole.height) / 2
  const floor = Math.min(8, cap)
  return clamp(rawRadius, floor, cap)
}

function centerOnRange(
  start: number,
  size: number,
  item: number,
  min: number,
  max: number,
): number {
  return clamp(start + size / 2 - item / 2, min, max)
}

export function dockTourCard(
  cardW: number,
  cardH: number,
  viewportW: number,
  viewportH: number,
  pad: number,
): TourCardPos {
  const maxLeft = Math.max(pad, viewportW - cardW - pad)
  const maxTop = Math.max(pad, viewportH - cardH - pad)
  return {
    placement: 'dock',
    left: clamp((viewportW - cardW) / 2, pad, maxLeft),
    top: clamp(viewportH - cardH - 28, pad, maxTop),
  }
}

// 两侧都放不下时停在视口底部，不要掉进洞里。
export function computeTourCardPosition(
  hole: GuideRect,
  cardW: number,
  cardH: number,
  viewportW: number,
  viewportH: number,
  pad: number = TOUR_VIEWPORT_PAD,
  gap: number = TOUR_CARD_GAP,
): TourCardPos {
  const maxLeft = Math.max(pad, viewportW - cardW - pad)
  const maxTop = Math.max(pad, viewportH - cardH - pad)

  const space = {
    top: hole.top - pad,
    bottom: viewportH - hole.bottom - pad,
    left: hole.left - pad,
    right: viewportW - hole.right - pad,
  }
  const needV = cardH + gap
  const needH = cardW + gap
  const need = {
    top: needV,
    bottom: needV,
    left: needH,
    right: needH,
  }

  const tall = hole.height > hole.width * 1.35
  const wide = hole.width > hole.height * 1.35
  const bonus = {
    top: wide ? 80 : 0,
    bottom: wide ? 100 : 0,
    left: tall ? 40 : 0,
    right: tall ? 100 : 0,
  }

  let best: Exclude<TourPlacement, 'dock'> | null = null
  let bestScore = Number.NEGATIVE_INFINITY
  const sides = ['right', 'bottom', 'left', 'top'] as const
  for (const side of sides) {
    if (space[side] < need[side]) continue
    const score = space[side] + bonus[side]
    if (score > bestScore) {
      bestScore = score
      best = side
    }
  }

  if (!best) {
    return dockTourCard(cardW, cardH, viewportW, viewportH, pad)
  }

  let top = 0
  let left = 0
  switch (best) {
    case 'top':
      top = hole.top - gap - cardH
      left = centerOnRange(hole.left, hole.width, cardW, pad, maxLeft)
      break
    case 'bottom':
      top = hole.bottom + gap
      left = centerOnRange(hole.left, hole.width, cardW, pad, maxLeft)
      break
    case 'left':
      left = hole.left - gap - cardW
      top = centerOnRange(hole.top, hole.height, cardH, pad, maxTop)
      break
    case 'right':
      left = hole.right + gap
      top = centerOnRange(hole.top, hole.height, cardH, pad, maxTop)
      break
  }

  left = clamp(left, pad, maxLeft)
  top = clamp(top, pad, maxTop)

  return { placement: best, left, top }
}
