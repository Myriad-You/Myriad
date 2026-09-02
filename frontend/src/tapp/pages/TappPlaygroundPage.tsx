/**
 * Tapp Playground 页面
 * 用自然语言生成、预览并安装 Tapp；全程运行在无授权的临时沙箱中。
 * 布局与多窗口运行页一致：透明工作区 + 可自由拖拽/缩放的浮动窗格，
 * 输入与日志收纳在底部控制岛。
 */

import type { PlaygroundLastFailedAttempt } from '../components/PlaygroundComposer'
import type { TappPlaygroundProject } from '../services/TappPlaygroundService'
import type { TappInstance, WidgetSize } from '../types'
import type {
  PlaygroundSessionsStore,
  PruneStoreMeta,
} from '../utils/playgroundSession'
import {
  FaArrowLeft,
  FaCode,
  FaExchangeAlt,
  FaGripVertical,
  FaLock,
  FaTimes,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import Prism from 'prismjs'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Navigate, useNavigate } from 'react-router-dom'
import { Spinner } from '../../components/Spinner'
import { useI18n } from '../../contexts/I18nContext'
import { useImmersiveChrome } from '../../contexts/NavigationContext'
import { isExlight, useAnimationLevel } from '../../hooks/useAnimationLevel'
import { usePageSeo } from '../../hooks/usePageSeo'
import { useBreakpoints } from '../../hooks/useSharedEventListener'
import { buildPrivatePageSeo } from '../../utils/modulePageSeo'
import {
  isUselessErrorText,
  userFacingError,
} from '../../utils/userFacingError'
import { PlaygroundComposer } from '../components/PlaygroundComposer'
import { TappPlaygroundIcon } from '../components/PlaygroundIcons'
import { getTappRuntime } from '../runtime'
import { TappPageSandbox } from '../runtime/TappPageSandbox'
import { TappWidgetSandbox } from '../runtime/TappWidgetSandbox'
import { installFromCode } from '../services/TappApiService'
import { generatePlaygroundProject } from '../services/TappPlaygroundService'
import { exportPlaygroundProjectAsTapp } from '../utils/exportPlaygroundTapp'
import { tappHasPage } from '../utils/manifestLayers'
import {
  computeLineDiff,
  countDiffChanges,
  toSideBySide,
} from '../utils/playgroundDiff'
import {
  mapPlaygroundGenerateError,
  mapPlaygroundRuntimeError,
} from '../utils/playgroundErrorMessages'
import { playgroundCodeToRuntime } from '../utils/playgroundPackageFiles'
import {
  buildPlaygroundMemoryHistory,
  clearSessionContent,
  createAndActivateSessionWithMeta,
  deleteSessionWithMeta,
  getActiveSession,
  loadSessionsStore,
  pushManualEditRevision,
  pushRevision,
  saveSessionsStore,
  switchSession,
  updateActiveSessionWithMeta,
} from '../utils/playgroundSession'
import { selectPreviewGrantedPermissions } from '../utils/previewGrants'
import { TAPP_LIST_PATH, tappDetailPath } from '../utils/tappPaths'
import {
  formatPlaygroundPackageErrors,
  PlaygroundPackageValidationError,
  validatePlaygroundPackage,
} from '../utils/validatePlaygroundPackage'
import 'prismjs/components/prism-json'
import './TappPlaygroundPage.css'

const CAPABILITY_NOTE_DISMISS_KEY =
  'myriad:tapp-playground:capability-note-dismissed'

type FileId =
  | 'manifest'
  | 'html'
  | 'page'
  | 'core'
  | 'styles'
  | 'i18n'
  | 'widget'
  | 'widgetHtml'
  | 'assets'

function phaseIndexFromElapsedMs(elapsedMs: number): number {
  const seconds = Math.floor(elapsedMs / 1000)
  return seconds < 5 ? 0 : seconds < 14 ? 1 : seconds < 90 ? 2 : 3
}

function isAbortLikeError(error: unknown): {
  aborted: boolean
  timeout: boolean
} {
  const name =
    error instanceof Error || error instanceof DOMException ? error.name : ''
  const message =
    error instanceof Error || error instanceof DOMException
      ? error.message || ''
      : String(error || '')
  const lower = `${name} ${message}`.toLowerCase()
  const timeout =
    name === 'TimeoutError' ||
    /timeout/i.test(name) ||
    /timed?\s*out/i.test(message) ||
    /aborted due to timeout/i.test(message) ||
    /signal timed out/i.test(message)
  const aborted =
    name === 'AbortError' ||
    /aborterror/i.test(lower) ||
    /the operation was aborted/i.test(message)
  return { aborted: aborted || timeout, timeout }
}

const FILE_LABELS: Record<FileId, string> = {
  page: 'page/index.js',
  html: 'page.html',
  styles: 'styles.css',
  core: 'core.js',
  manifest: 'manifest.json',
  i18n: 'i18n.json',
  widget: 'widget/index.js',
  widgetHtml: 'widget.html',
  assets: 'assets',
}

function fileContents(project: TappPlaygroundProject, file: FileId): string {
  switch (file) {
    case 'manifest':
      return JSON.stringify(project.manifest, null, 2)
    case 'html':
      return project.code.pageHtml || ''
    case 'page':
      return project.code.page || ''
    case 'core':
      return project.code.core || ''
    case 'styles':
      return project.code.styles || ''
    case 'i18n':
      return JSON.stringify(project.code.i18n || {}, null, 2)
    case 'widget':
      return project.code.widget || ''
    case 'widgetHtml':
      return project.code.widgetHtml || ''
    case 'assets':
      return JSON.stringify(
        Object.fromEntries(
          Object.entries(project.code.assets || {}).map(([path, value]) => [
            path,
            `[encoded asset: ${value.length} bytes]`,
          ]),
        ),
        null,
        2,
      )
  }
}

/* ============================================================
 * 浮动窗格 - 拖拽/缩放逻辑取自 TappWindowManager（简化版）
 * ============================================================ */

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

const MIN_PANE_SIZE = { width: 320, height: 220 }

const RESIZE_HANDLES = [
  { direction: 'n', className: 'top-0 left-2 right-2 h-1 cursor-n-resize' },
  { direction: 's', className: 'bottom-0 left-2 right-2 h-1 cursor-s-resize' },
  { direction: 'e', className: 'right-0 top-2 bottom-2 w-1 cursor-e-resize' },
  { direction: 'w', className: 'left-0 top-2 bottom-2 w-1 cursor-w-resize' },
  { direction: 'ne', className: 'top-0 right-0 w-3 h-3 cursor-ne-resize' },
  { direction: 'nw', className: 'top-0 left-0 w-3 h-3 cursor-nw-resize' },
  { direction: 'se', className: 'bottom-0 right-0 w-3 h-3 cursor-se-resize' },
  { direction: 'sw', className: 'bottom-0 left-0 w-3 h-3 cursor-sw-resize' },
] as const

interface FloatingPaneProps {
  defaultRect: Rect
  bounds: { width: number; height: number }
  isActive: boolean
  onFocus: () => void
  /** 标题栏内容（拖拽把手区域） */
  header: React.ReactNode
  children: React.ReactNode
  /** 移动端渲染为静态块，禁用拖拽/缩放 */
  interactive: boolean
  /** 静态模式下的高度 class */
  staticClassName?: string
}

function FloatingPane({
  defaultRect,
  bounds,
  isActive,
  onFocus,
  header,
  children,
  interactive,
  staticClassName,
}: FloatingPaneProps) {
  const paneRef = useRef<HTMLDivElement>(null)
  const [rect, setRect] = useState<Rect>(() => defaultRect)
  const [isDragging, setIsDragging] = useState(false)
  const [resizeDirection, setResizeDirection] = useState<string | null>(null)

  const dragStartRef = useRef({ x: 0, y: 0 })
  const rectStartRef = useRef<Rect>(rect)
  const currentRectRef = useRef<Rect>(rect)

  useEffect(() => {
    currentRectRef.current = rect
  }, [rect])

  const beginInteraction = useCallback(
    (e: React.MouseEvent | React.TouchEvent, direction: string | null) => {
      e.preventDefault()
      e.stopPropagation()
      const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
      const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY
      dragStartRef.current = { x: clientX, y: clientY }
      rectStartRef.current = { ...currentRectRef.current }
      if (direction) {
        setResizeDirection(direction)
      } else {
        setIsDragging(true)
      }
      onFocus()
    },
    [onFocus],
  )

  // 移动/缩放 - RAF 节流，直接操作 DOM，结束时一次性提交 state
  useEffect(() => {
    if (!isDragging && !resizeDirection) return

    let rafId: number | null = null
    let lastX = dragStartRef.current.x
    let lastY = dragStartRef.current.y

    const handleMove = (e: MouseEvent | TouchEvent) => {
      const clientX =
        'touches' in e ? (e.touches[0]?.clientX ?? lastX) : e.clientX
      const clientY =
        'touches' in e ? (e.touches[0]?.clientY ?? lastY) : e.clientY
      if (clientX === lastX && clientY === lastY) return
      lastX = clientX
      lastY = clientY

      if (rafId) cancelAnimationFrame(rafId)
      rafId = requestAnimationFrame(() => {
        const node = paneRef.current
        if (!node) return
        const deltaX = clientX - dragStartRef.current.x
        const deltaY = clientY - dragStartRef.current.y
        const start = rectStartRef.current
        let { x, y, width, height } = start

        if (isDragging) {
          x = Math.max(0, Math.min(start.x + deltaX, bounds.width - width))
          y = Math.max(0, Math.min(start.y + deltaY, bounds.height - height))
        } else if (resizeDirection) {
          if (resizeDirection.includes('e')) {
            width = Math.max(MIN_PANE_SIZE.width, start.width + deltaX)
          }
          if (resizeDirection.includes('w')) {
            const widthDelta = Math.min(
              deltaX,
              start.width - MIN_PANE_SIZE.width,
            )
            width = start.width - widthDelta
            x = start.x + widthDelta
          }
          if (resizeDirection.includes('s')) {
            height = Math.max(MIN_PANE_SIZE.height, start.height + deltaY)
          }
          if (resizeDirection.includes('n')) {
            const heightDelta = Math.min(
              deltaY,
              start.height - MIN_PANE_SIZE.height,
            )
            height = start.height - heightDelta
            y = start.y + heightDelta
          }
          width = Math.min(width, bounds.width - x)
          height = Math.min(height, bounds.height - y)
        }

        node.style.transform = `translate3d(${x}px, ${y}px, 0)`
        node.style.width = `${width}px`
        node.style.height = `${height}px`
        currentRectRef.current = { x, y, width, height }
      })
    }

    const handleEnd = () => {
      if (rafId) cancelAnimationFrame(rafId)
      setRect(currentRectRef.current)
      setIsDragging(false)
      setResizeDirection(null)
    }

    document.addEventListener('mousemove', handleMove, { passive: true })
    document.addEventListener('mouseup', handleEnd)
    document.addEventListener('touchmove', handleMove, { passive: true })
    document.addEventListener('touchend', handleEnd)
    document.addEventListener('touchcancel', handleEnd)
    return () => {
      if (rafId) cancelAnimationFrame(rafId)
      document.removeEventListener('mousemove', handleMove)
      document.removeEventListener('mouseup', handleEnd)
      document.removeEventListener('touchmove', handleMove)
      document.removeEventListener('touchend', handleEnd)
      document.removeEventListener('touchcancel', handleEnd)
    }
  }, [isDragging, resizeDirection, bounds])

  const isInteracting = isDragging || !!resizeDirection

  if (!interactive) {
    return (
      <div
        className={`relative flex flex-col overflow-hidden rounded-xl w-full ${staticClassName || ''}`}
        style={{
          border: '1px solid var(--border-color)',
          boxShadow: '0 4px 12px rgba(0, 0, 0, 0.1)',
        }}
      >
        <div
          className="flex items-center h-9 shrink-0 select-none glass-surface glass-80"
          style={{
            borderBottom: '1px solid var(--border-color)',
          }}
        >
          {header}
        </div>
        <div
          className="flex-1 min-h-0 overflow-hidden relative"
          style={{ backgroundColor: 'var(--bg-primary)' }}
        >
          {children}
        </div>
      </div>
    )
  }

  return (
    <div
      ref={paneRef}
      className="absolute flex flex-col overflow-hidden rounded-xl"
      style={{
        top: 0,
        left: 0,
        width: rect.width,
        height: rect.height,
        transform: `translate3d(${rect.x}px, ${rect.y}px, 0)`,
        zIndex: isActive ? 30 : 20,
        transition: isInteracting ? 'none' : 'box-shadow 0.15s',
        boxShadow: isActive
          ? '0 8px 24px rgba(0, 0, 0, 0.2)'
          : '0 4px 12px rgba(0, 0, 0, 0.1)',
        border: '1px solid var(--border-color)',
      }}
      onMouseDown={onFocus}
      onTouchStart={onFocus}
    >
      {/* 标题栏 - 拖拽把手 */}
      <div
        className={`flex items-center h-9 shrink-0 select-none glass-surface glass-80 ${
          isDragging ? 'cursor-grabbing' : 'cursor-grab'
        }`}
        style={{
          borderBottom: '1px solid var(--border-color)',
          opacity: isActive ? 1 : 0.7,
          transition: 'opacity 0.2s ease',
        }}
        onMouseDown={(e) => beginInteraction(e, null)}
        onTouchStart={(e) => beginInteraction(e, null)}
      >
        {header}
      </div>

      {/* 内容 */}
      <div
        className="flex-1 min-h-0 overflow-hidden relative"
        style={{ backgroundColor: 'var(--bg-primary)' }}
      >
        {/* 交互时的透明遮罩，防止 iframe 捕获指针事件 */}
        {isInteracting && <div className="absolute inset-0 z-50" />}
        {children}
      </div>

      {/* 缩放手柄 */}
      {RESIZE_HANDLES.map(({ direction, className }) => (
        <div
          key={direction}
          className={`absolute z-40 ${className}`}
          onMouseDown={(e) => beginInteraction(e, direction)}
          onTouchStart={(e) => beginInteraction(e, direction)}
        />
      ))}
    </div>
  )
}

/* ============================================================
 * 代码编辑器：Prism 高亮层 + 透明 textarea 输入层
 * ============================================================ */

const FILE_LANGUAGE: Record<FileId, string> = {
  page: 'javascript',
  core: 'javascript',
  widget: 'javascript',
  html: 'markup',
  widgetHtml: 'markup',
  styles: 'css',
  manifest: 'json',
  i18n: 'json',
  assets: 'json',
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function CodeEditor({
  value,
  language,
  readOnly,
  label,
  onChange,
}: {
  value: string
  language: string
  readOnly: boolean
  label: string
  onChange: (next: string) => void
}) {
  const highlighted = useMemo(() => {
    const grammar = Prism.languages[language]
    return grammar
      ? Prism.highlight(value, grammar, language)
      : escapeHtml(value)
  }, [value, language])

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Tab') {
      event.preventDefault()
      const el = event.currentTarget
      el.setRangeText('  ', el.selectionStart, el.selectionEnd, 'end')
      onChange(el.value)
    }
  }

  return (
    <div className="playground-code-editor">
      <pre aria-hidden="true">
        <code dangerouslySetInnerHTML={{ __html: `${highlighted}\n` }} />
      </pre>
      {!readOnly && (
        <textarea
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={handleKeyDown}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          wrap="soft"
          aria-label={label}
        />
      )}
    </div>
  )
}

/* ============================================================
 * 页面
 * ============================================================ */

export function TappPlaygroundPage() {
  const navigate = useNavigate()
  const { t, locale, format } = useI18n()
  const { isMobile } = useBreakpoints()
  const animConfig = useAnimationLevel()

  usePageSeo(
    useMemo(
      () =>
        buildPrivatePageSeo({
          label: t.tapp.playgroundTitle || 'Playground',
          path: '/tapp/playground',
        }),
      [t],
    ),
  )

  const [store, setStore] = useState<PlaygroundSessionsStore>(loadSessionsStore)
  const [instruction, setInstruction] = useState('')
  const [busy, setBusy] = useState(false)
  const [busyMode, setBusyMode] = useState<'user' | 'runtime-repair'>('user')
  /** Latest real agent step summary from generate-stream (busy band line 2). */
  const [busyStepSummary, setBusyStepSummary] = useState<string | null>(null)
  const lastStreamStepRef = useRef<string | null>(null)
  const [installing, setInstalling] = useState(false)
  const [exporting, setExporting] = useState(false)
  const [selectedFile, setSelectedFile] = useState<FileId>('page')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  /** One-shot storage prune warning (separate from success notice). */
  const [pruneNotice, setPruneNotice] = useState('')
  const [previewError, setPreviewError] = useState('')
  const [lastSuccessElapsedMs, setLastSuccessElapsedMs] = useState<
    number | null
  >(null)
  const [capabilityNoteVisible, setCapabilityNoteVisible] = useState(() => {
    if (typeof window === 'undefined') return true
    try {
      return sessionStorage.getItem(CAPABILITY_NOTE_DISMISS_KEY) !== '1'
    } catch {
      return true
    }
  })
  const [diffOpen, setDiffOpen] = useState(false)
  const [diffMode, setDiffMode] = useState<'unified' | 'side-by-side'>(
    'unified',
  )
  /** Base revision index for diff; -1 = previous of current when using quick mode. */
  const [diffBaseIndex, setDiffBaseIndex] = useState<number | null>(null)
  const [diffCompareIndex, setDiffCompareIndex] = useState<number | null>(null)
  const [activePane, setActivePane] = useState<'preview' | 'code' | 'widget'>(
    'preview',
  )
  // 小组件预览选择：无效值自动回退到首个声明的组件及其默认尺寸
  const [widgetId, setWidgetId] = useState('')
  const [widgetSize, setWidgetSize] = useState<WidgetSize | ''>('')
  // 手动编辑代码的草稿：为空表示未编辑，直接展示项目内容
  const [draft, setDraft] = useState<string | null>(null)
  const [draftInvalid, setDraftInvalid] = useState(false)
  const draftTimerRef = useRef<number | undefined>(undefined)
  const runtimeRepairCountRef = useRef(0)
  const repairedRuntimeErrorsRef = useRef(new Set<string>())
  /** In-flight generate AbortController (user Cancel). */
  const generateAbortRef = useRef<AbortController | null>(null)
  const userCancelledRef = useRef(false)
  const generateRequestIdRef = useRef(0)
  /** Avoid re-notifying the same prune event on every effect run. */
  const lastPruneNoticeKeyRef = useRef('')

  const animationsEnabled = !isExlight(animConfig)
  const springTransition = animConfig.spring
    ? ({ type: 'spring', stiffness: 400, damping: 30 } as const)
    : ({ type: 'tween', duration: 0.25 * animConfig.durationScale } as const)

  const session = getActiveSession(store)
  const revision = session.revisions[session.revisionIndex]
  const project = revision?.project
  const sessionSummaries = useMemo(
    () =>
      [...store.sessions]
        .sort((a, b) => b.updatedAt - a.updatedAt)
        .map((s) => ({
          id: s.id,
          title: s.title,
          updatedAt: s.updatedAt,
          revisionCount: s.revisions.length,
        })),
    [store.sessions],
  )
  const historyRevisions = useMemo(
    () =>
      session.revisions.map((rev, index) => ({
        id: rev.id,
        index,
        instruction: rev.instruction,
        createdAt: rev.createdAt,
        origin: rev.origin,
        explanation: rev.explanation,
      })),
    [session.revisions],
  )

  // 项目同时包含页面和小组件时，预览拆成两个独立窗格；
  // 仅小组件（无可用 Page）时不挂载页面沙箱，左列整列给小组件预览。
  const manifestWidgets = project?.manifest.widgets || []
  const hasWidgetPreview =
    manifestWidgets.length > 0 &&
    !!(project?.code.widget || project?.code.widgetHtml)
  const hasUsablePage =
    !!project &&
    tappHasPage(project.manifest) &&
    !!(project.code.pageHtml && project.code.pageHtml.trim())
  const isWidgetOnly = hasWidgetPreview && !hasUsablePage
  const activeWidget =
    manifestWidgets.find((widget) => widget.id === widgetId) ||
    manifestWidgets[0]
  const activeWidgetSize: WidgetSize =
    (widgetSize && activeWidget?.sizes?.includes(widgetSize)
      ? widgetSize
      : activeWidget?.defaultSize) ||
    activeWidget?.sizes?.[0] ||
    '2x2'

  // 工作区尺寸（用于窗格默认布局与边界约束）
  // callback ref + ResizeObserver：motion 懒加载会重挂根节点，
  // 单次 effect 测量会拿到 0，观察器保证任何挂载/尺寸变化都能测到
  const [bounds, setBounds] = useState({ width: 0, height: 0 })
  const workspaceObserverRef = useRef<ResizeObserver | null>(null)
  const attachWorkspace = useCallback((node: HTMLDivElement | null) => {
    workspaceObserverRef.current?.disconnect()
    workspaceObserverRef.current = null
    if (!node) return
    const measure = () =>
      setBounds({ width: node.clientWidth, height: node.clientHeight })
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(node)
    workspaceObserverRef.current = observer
  }, [])
  useEffect(() => () => workspaceObserverRef.current?.disconnect(), [])

  // 进入沉浸模式，隐藏底部导航岛给控制岛让位（桌面端 only；移动端会立刻重定向）
  useImmersiveChrome('tapp-playground', !isMobile)

  useEffect(() => {
    saveSessionsStore(store)
  }, [store])

  // Widget-only: focus the widget pane (and prefer widget source tabs).
  useEffect(() => {
    if (!isWidgetOnly) return
    setActivePane('widget')
    setSelectedFile((current) => {
      if (current === 'widget' || current === 'widgetHtml') return current
      if (project?.code.widget) return 'widget'
      if (project?.code.widgetHtml) return 'widgetHtml'
      return current
    })
  }, [
    isWidgetOnly,
    project?.manifest.id,
    revision?.id,
    project?.code.widget,
    project?.code.widgetHtml,
  ])

  const applyPruneNotice = useCallback(
    (meta: PruneStoreMeta) => {
      if (!meta.changed) return
      const key = `${meta.sessionsDropped}:${meta.revisionsTrimmed}`
      if (lastPruneNoticeKeyRef.current === key) return
      lastPruneNoticeKeyRef.current = key
      if (meta.sessionsDropped > 0 && meta.revisionsTrimmed > 0) {
        setPruneNotice(t.tapp.playgroundPruneNotice)
      } else if (meta.sessionsDropped > 0) {
        setPruneNotice(
          format(t.tapp.playgroundPruneNoticeSessions, {
            n: meta.sessionsDropped,
          }),
        )
      } else if (meta.revisionsTrimmed > 0) {
        setPruneNotice(
          format(t.tapp.playgroundPruneNoticeRevisions, {
            n: meta.revisionsTrimmed,
          }),
        )
      }
      // Allow the same shape of prune to notify again later
      window.setTimeout(() => {
        if (lastPruneNoticeKeyRef.current === key) {
          lastPruneNoticeKeyRef.current = ''
        }
      }, 1500)
    },
    [t, format],
  )

  /** Commit store mutation that may prune; surface one-shot prune notice. */
  const commitStore = useCallback(
    (
      mutator: (
        current: PlaygroundSessionsStore,
      ) => { store: PlaygroundSessionsStore; meta: PruneStoreMeta },
    ) => {
      let meta: PruneStoreMeta = {
        sessionsDropped: 0,
        revisionsTrimmed: 0,
        changed: false,
      }
      setStore((current) => {
        const result = mutator(current)
        meta = result.meta
        return result.store
      })
      if (meta.changed) {
        queueMicrotask(() => applyPruneNotice(meta))
      }
      return meta
    },
    [applyPruneNotice],
  )

  const examplePrompts = useMemo(
    () => [
      {
        id: 'countdown',
        label: t.tapp.playgroundExampleLabelCountdown,
        prompt: t.tapp.playgroundExampleCountdown,
      },
      {
        id: 'notes',
        label: t.tapp.playgroundExampleLabelNotes,
        prompt: t.tapp.playgroundExampleNotes,
      },
      {
        id: 'widget-stats',
        label: t.tapp.playgroundExampleLabelWidgetStats,
        prompt: t.tapp.playgroundExampleWidgetStats,
      },
      {
        id: 'todo',
        label: t.tapp.playgroundExampleLabelTodo,
        prompt: t.tapp.playgroundExampleTodo,
      },
      {
        id: 'pomodoro',
        label: t.tapp.playgroundExampleLabelPomodoro,
        prompt: t.tapp.playgroundExamplePomodoro,
      },
      {
        id: 'markdown',
        label: t.tapp.playgroundExampleLabelMarkdown,
        prompt: t.tapp.playgroundExampleMarkdown,
      },
    ],
    [t],
  )

  const openDiffVsPrevious = useCallback(() => {
    if (session.revisions.length < 2) return
    const idx = session.revisionIndex
    if (idx > 0) {
      setDiffBaseIndex(idx - 1)
      setDiffCompareIndex(idx)
    } else {
      setDiffBaseIndex(0)
      setDiffCompareIndex(1)
    }
    setDiffOpen(true)
  }, [session.revisionIndex, session.revisions.length])

  const diffLines = useMemo(() => {
    if (!diffOpen || session.revisions.length < 2) return null
    const baseIdx =
      diffBaseIndex ??
      (session.revisionIndex > 0 ? session.revisionIndex - 1 : null)
    const compareIdx = diffCompareIndex ?? session.revisionIndex
    if (
      baseIdx == null ||
      compareIdx == null ||
      baseIdx < 0 ||
      compareIdx < 0 ||
      baseIdx >= session.revisions.length ||
      compareIdx >= session.revisions.length
    ) {
      return null
    }
    const beforeProj = session.revisions[baseIdx]?.project
    const afterProj = session.revisions[compareIdx]?.project
    if (!beforeProj || !afterProj) return null
    return {
      baseIdx,
      compareIdx,
      lines: computeLineDiff(
        fileContents(beforeProj, selectedFile),
        fileContents(afterProj, selectedFile),
      ),
    }
  }, [
    diffOpen,
    diffBaseIndex,
    diffCompareIndex,
    session.revisions,
    session.revisionIndex,
    selectedFile,
  ])

  // 默认布局：预览居左约 55%，代码居右，底部为控制岛预留空间；
  // 页面 + 小组件：左列上下拆分；仅小组件：左列整列给小组件预览。
  const defaultLayout = useMemo(() => {
    if (!bounds.width || !bounds.height) return null
    const margin = 14
    const top = 64
    const gap = 14
    const bottomReserve = 132
    const height = Math.max(
      MIN_PANE_SIZE.height,
      bounds.height - top - margin - bottomReserve,
    )
    const innerWidth = bounds.width - margin * 2 - gap
    const previewWidth = Math.max(
      MIN_PANE_SIZE.width,
      Math.round(innerWidth * 0.55),
    )
    const codeWidth = Math.max(MIN_PANE_SIZE.width, innerWidth - previewWidth)
    const leftCol = { x: margin, y: top, width: previewWidth }
    const code = {
      x: margin + previewWidth + gap,
      y: top,
      width: codeWidth,
      height,
    }

    // Widget-only: full left column for widget; no page preview pane.
    if (isWidgetOnly) {
      return {
        preview: null as {
          x: number
          y: number
          width: number
          height: number
        } | null,
        widget: { ...leftCol, height },
        code,
      }
    }

    const canSplit =
      hasWidgetPreview &&
      hasUsablePage &&
      height >= MIN_PANE_SIZE.height * 2 + gap
    const widgetHeight = canSplit
      ? Math.max(MIN_PANE_SIZE.height, Math.round(height * 0.4))
      : 0
    const previewHeight = canSplit ? height - widgetHeight - gap : height
    return {
      preview: { ...leftCol, height: previewHeight },
      widget: canSplit
        ? {
            ...leftCol,
            y: top + previewHeight + gap,
            height: widgetHeight,
          }
        : null,
      code,
    }
  }, [bounds, hasWidgetPreview, hasUsablePage, isWidgetOnly])

  const tappInstance = useMemo<TappInstance | null>(() => {
    if (!project) return null
    // MYR-024: Manifest permissions are install-time declarations only.
    // Preview grants are an explicit temporary allowlist intersection — never
    // the full declaration list (which would open real host capabilities / CSP).
    return {
      id: project.manifest.id,
      manifest: project.manifest,
      status: 'running',
      installedAt: new Date().toISOString(),
      grantedPermissions: selectPreviewGrantedPermissions(
        project.manifest.permissions,
      ),
      userRole: 'admin',
      isTemporary: true,
      isAdminTapp: false,
    }
  }, [project])

  const cancelGeneration = useCallback(() => {
    const controller = generateAbortRef.current
    if (!controller) return
    userCancelledRef.current = true
    controller.abort(
      new DOMException('User cancelled generation', 'AbortError'),
    )
  }, [])

  const executeGeneration = async (
    prompt: string,
    origin: 'user' | 'runtime-repair',
    runtimeFeedback: string[] = [],
  ) => {
    if (!prompt.trim() || busy) return
    // Cancel any leftover controller; start a fresh AbortController for this run.
    generateAbortRef.current?.abort()
    const abortController = new AbortController()
    generateAbortRef.current = abortController
    userCancelledRef.current = false
    // Generation request id: ignore late responses after cancel/unmount.
    const requestId = (generateRequestIdRef.current += 1)

    setBusy(true)
    setBusyMode(origin)
    setBusyStepSummary(null)
    lastStreamStepRef.current = null
    setError('')
    setNotice('')
    setLastSuccessElapsedMs(null)
    if (origin === 'user') setPreviewError('')
    // Snapshot multi-turn memory BEFORE clearing the failure banner so a prior
    // failed attempt is still sent as a failed tail entry.
    const history = buildPlaygroundMemoryHistory(session)
    // Clear prior failure banner while a new attempt is in flight
    commitStore((current) =>
      updateActiveSessionWithMeta(
        current,
        (active) =>
          active.lastFailedAttempt
            ? { ...active, lastFailedAttempt: null }
            : active,
        false,
      ),
    )
    const startedAt = Date.now()
    try {
      const response = await generatePlaygroundProject(
        {
          instruction: prompt.trim(),
          currentProject: project,
          runtimeFeedback,
          history,
        },
        {
          signal: abortController.signal,
          onStep: (step) => {
            if (requestId !== generateRequestIdRef.current) return
            const summary = step.summary?.trim()
            if (!summary) return
            lastStreamStepRef.current = summary
            setBusyStepSummary(summary)
          },
        },
      )
      if (
        requestId !== generateRequestIdRef.current ||
        userCancelledRef.current ||
        abortController.signal.aborted
      ) {
        return
      }
      const elapsedMs = Date.now() - startedAt
      commitStore((current) =>
        updateActiveSessionWithMeta(current, (active) =>
          pushRevision(active, {
            project: response.project,
            explanation: response.explanation,
            instruction: prompt.trim(),
            warnings: response.warnings,
            createdAt: Date.now(),
            origin,
            agentTrace: response.agentTrace,
            knowledgeSources: response.knowledgeSources,
            validation: response.validation,
          }),
        ),
      )
      if (origin === 'user') setInstruction('')
      setPreviewError('')
      setLastSuccessElapsedMs(elapsedMs)
      setNotice(response.explanation)
    } catch (requestError) {
      if (requestId !== generateRequestIdRef.current) return

      const elapsedMs = Date.now() - startedAt
      const { aborted, timeout } = isAbortLikeError(requestError)
      const userCancelled = userCancelledRef.current && aborted && !timeout

      const rawMessage =
        requestError instanceof Error
          ? requestError.message
          : requestError instanceof DOMException
            ? requestError.name || requestError.message
            : t.tapp.playgroundGenerateFailed
      const name =
        requestError instanceof Error || requestError instanceof DOMException
          ? requestError.name
          : ''
      const messageForMap =
        name === 'AbortError' || name === 'TimeoutError'
          ? name
          : rawMessage || t.tapp.playgroundGenerateFailed
      const friendly = mapPlaygroundGenerateError(messageForMap, t.tapp, {
        userCancelled,
        format,
        code:
          requestError &&
          typeof requestError === 'object' &&
          'code' in requestError &&
          typeof requestError.code === 'string'
            ? requestError.code
            : undefined,
      })

      if (userCancelled) {
        // Soft notice only — not the hard failure banner. Keep lastFailed so
        // the user can still Retry the same prompt from the status band if set;
        // we intentionally do NOT set lastFailed for cancel (distinct UX).
        setNotice(friendly)
        // Keep instruction for easy re-submit.
        return
      }

      const failed: PlaygroundLastFailedAttempt = {
        instruction: prompt.trim(),
        error: friendly,
        elapsedMs,
        finishedAt: Date.now(),
        origin,
        phaseIndex: phaseIndexFromElapsedMs(elapsedMs),
        lastStepSummary: lastStreamStepRef.current || undefined,
      }
      commitStore((current) =>
        updateActiveSessionWithMeta(current, (active) => ({
          ...active,
          lastFailedAttempt: failed,
        })),
      )
      // Keep instruction text on failure (do not clear). Prefer the status
      // band over a duplicate floating error card for generate failures.
    } finally {
      if (requestId === generateRequestIdRef.current) {
        setBusy(false)
        setBusyStepSummary(null)
        if (generateAbortRef.current === abortController) {
          generateAbortRef.current = null
        }
        userCancelledRef.current = false
      }
    }
  }

  const runGeneration = async () => {
    const prompt = instruction.trim()
    if (!prompt || busy) return
    runtimeRepairCountRef.current = 0
    repairedRuntimeErrorsRef.current.clear()
    await executeGeneration(prompt, 'user')
  }

  const retryFailedAttempt = async () => {
    const failed = session.lastFailedAttempt
    if (!failed?.instruction.trim() || busy) return
    // Restore the same prompt into the composer, then re-run as a user attempt
    setInstruction(failed.instruction)
    runtimeRepairCountRef.current = 0
    repairedRuntimeErrorsRef.current.clear()
    await executeGeneration(failed.instruction, 'user')
  }

  const dismissFailedAttempt = () => {
    commitStore((current) =>
      updateActiveSessionWithMeta(
        current,
        (active) =>
          active.lastFailedAttempt
            ? { ...active, lastFailedAttempt: null }
            : active,
        false,
      ),
    )
  }

  /** Page sandbox errors may auto-repair (existing behavior). */
  const handleSandboxError = (sandboxError: Error) => {
    const message =
      sandboxError.message || t.tapp.playgroundUnknownError
    setPreviewError(mapPlaygroundRuntimeError(message, t.tapp, format))
    // Widget-only projects have no page sandbox; never auto-repair for page absence.
    if (isWidgetOnly || !hasUsablePage) return
    if (!project || busy || runtimeRepairCountRef.current >= 2) return

    const errorKey = `${revision?.createdAt || 0}:${message}`
    if (repairedRuntimeErrorsRef.current.has(errorKey)) return
    repairedRuntimeErrorsRef.current.add(errorKey)
    runtimeRepairCountRef.current += 1

    window.setTimeout(() => {
      void executeGeneration(
        t.tapp.playgroundRepairInstruction,
        'runtime-repair',
        [message],
      )
    }, 500)
  }

  /** Widget errors surface in the status band but do not trigger auto-repair. */
  const handleWidgetError = (sandboxError: Error) => {
    const message =
      sandboxError.message || t.tapp.playgroundUnknownError
    setPreviewError(mapPlaygroundRuntimeError(message, t.tapp, format))
  }

  const showRevisionNotice = (rev: {
    explanation?: string
    instruction?: string
  }) => {
    const text = (rev.explanation || rev.instruction || '').trim()
    if (text) {
      setNotice(text.length > 160 ? `${text.slice(0, 159)}…` : text)
    } else {
      setNotice('')
    }
  }

  const moveRevision = (delta: number) => {
    if (session.revisions.length === 0) return
    const revisionIndex = Math.max(
      0,
      Math.min(session.revisions.length - 1, session.revisionIndex + delta),
    )
    if (revisionIndex === session.revisionIndex) return
    const nextRev = session.revisions[revisionIndex]
    commitStore((current) =>
      updateActiveSessionWithMeta(
        current,
        (active) => ({ ...active, revisionIndex }),
        false,
      ),
    )
    setError('')
    setPreviewError('')
    if (nextRev) showRevisionNotice(nextRev)
    else setNotice('')
  }

  const jumpToRevision = (index: number) => {
    if (session.revisions.length === 0) return
    const revisionIndex = Math.max(
      0,
      Math.min(session.revisions.length - 1, index),
    )
    const nextRev = session.revisions[revisionIndex]
    if (revisionIndex !== session.revisionIndex) {
      commitStore((current) =>
        updateActiveSessionWithMeta(
          current,
          (active) => ({ ...active, revisionIndex }),
          false,
        ),
      )
    }
    setError('')
    setPreviewError('')
    if (nextRev) showRevisionNotice(nextRev)
    else setNotice('')
  }

  const handleCreateSession = () => {
    if (busy) return
    commitStore((current) => createAndActivateSessionWithMeta(current))
    setInstruction('')
    setError('')
    setNotice('')
    setPreviewError('')
    setLastSuccessElapsedMs(null)
    runtimeRepairCountRef.current = 0
    repairedRuntimeErrorsRef.current.clear()
  }

  const handleSwitchSession = (sessionId: string) => {
    if (busy || sessionId === store.activeSessionId) return
    setStore((current) => switchSession(current, sessionId))
    setInstruction('')
    setError('')
    setNotice('')
    setPreviewError('')
    setLastSuccessElapsedMs(null)
    runtimeRepairCountRef.current = 0
    repairedRuntimeErrorsRef.current.clear()
  }

  const handleDeleteSession = (sessionId: string) => {
    if (busy) return
    commitStore((current) => deleteSessionWithMeta(current, sessionId))
    if (sessionId === store.activeSessionId) {
      setInstruction('')
      setError('')
      setNotice('')
      setPreviewError('')
      setLastSuccessElapsedMs(null)
      runtimeRepairCountRef.current = 0
      repairedRuntimeErrorsRef.current.clear()
    }
  }

  const packageValidationErrorMessage = (errors: string[]): string => {
    const detail = formatPlaygroundPackageErrors(errors)
    const template = t.tapp.playgroundPackageInvalid
    return template.includes('{errors}')
      ? template.replace('{errors}', detail)
      : `${template}\n${detail}`
  }

  const installProject = async () => {
    if (!project || installing || exporting) return
    setInstalling(true)
    setError('')
    setNotice('')
    try {
      const validation = validatePlaygroundPackage(project)
      if (!validation.ok) {
        setError(packageValidationErrorMessage(validation.errors))
        return
      }
      const installed = await installFromCode(
        project.manifest,
        project.code,
      )

      // Sync runtime so the new install is in installedTapps, then enable it.
      // Sync/start failures must not fail the install — user can enable on detail.
      const runtime = getTappRuntime()
      try {
        await runtime.syncFromBackend(true)
        await runtime.startTapp(installed.id)
        setNotice(t.tapp.playgroundInstallStartedSuccess)
      } catch (startError) {
        console.error(
          'Failed to sync/start Tapp after playground install:',
          startError,
        )
        const raw =
          startError instanceof Error ? startError.message.trim() : ''
        const detail =
          raw && !isUselessErrorText(raw)
            ? userFacingError(startError, t.tapp.startAppFailed)
            : ''
        setNotice(
          detail
            ? t.tapp.playgroundInstallStartFailed.replace('{error}', detail)
            : t.tapp.playgroundInstallSuccess,
        )
      }

      window.setTimeout(navigate, 450, tappDetailPath(installed.id))
    } catch (installError) {
      setError(userFacingError(installError, t.tapp.installFailed))
    } finally {
      setInstalling(false)
    }
  }

  const exportProject = async () => {
    if (!project || exporting || installing || busy) return
    setExporting(true)
    setError('')
    setNotice('')
    try {
      const filename = await exportPlaygroundProjectAsTapp(project)
      setNotice(
        t.tapp.playgroundExportSuccess.replace('{filename}', filename),
      )
    } catch (exportError) {
      if (exportError instanceof PlaygroundPackageValidationError) {
        setError(packageValidationErrorMessage(exportError.errors))
      } else {
        setError(
          userFacingError(exportError, t.tapp.playgroundExportFailed),
        )
      }
    } finally {
      setExporting(false)
    }
  }

  const clearSession = () => {
    if (busy) cancelGeneration()
    commitStore((current) =>
      updateActiveSessionWithMeta(current, (active) =>
        clearSessionContent(active),
      ),
    )
    setInstruction('')
    setError('')
    setNotice('')
    setPreviewError('')
    setLastSuccessElapsedMs(null)
    runtimeRepairCountRef.current = 0
    repairedRuntimeErrorsRef.current.clear()
  }

  // 切换文件或版本时丢弃未提交的编辑草稿；清理时取消待落盘的定时器
  useEffect(() => {
    setDraft(null)
    setDraftInvalid(false)
    return () => {
      if (draftTimerRef.current) window.clearTimeout(draftTimerRef.current)
    }
  }, [selectedFile, session.revisionIndex])

  // 手动编辑落盘：解析成功后推入 manual 版本（防抖 600ms；无变更则跳过）
  const applyDraft = useCallback((file: FileId, text: string) => {
    const textKeys = {
      page: 'page',
      html: 'pageHtml',
      styles: 'styles',
      core: 'core',
      widget: 'widget',
      widgetHtml: 'widgetHtml',
    } as const
    let parsedJson: unknown
    const isJsonFile =
      file === 'manifest' || file === 'i18n'
    if (isJsonFile) {
      try {
        parsedJson = JSON.parse(text)
      } catch {
        setDraftInvalid(true)
        return
      }
    } else if (!(file in textKeys)) {
      return
    }
    setDraftInvalid(false)
    const fileLabel = FILE_LABELS[file] || file
    commitStore((current) =>
      updateActiveSessionWithMeta(
        current,
        (active) => {
          const rev = active.revisions[active.revisionIndex]
          if (!rev) return active
          const revProject = rev.project
          let nextProject: TappPlaygroundProject
          if (file === 'manifest') {
            nextProject = {
              ...revProject,
              manifest: parsedJson as TappPlaygroundProject['manifest'],
            }
          } else if (file === 'i18n') {
            nextProject = {
              ...revProject,
              code: {
                ...revProject.code,
                i18n: parsedJson as TappPlaygroundProject['code']['i18n'],
              },
            }
          } else {
            nextProject = {
              ...revProject,
              code: {
                ...revProject.code,
                [textKeys[file as keyof typeof textKeys]]: text,
              },
            }
          }
          return pushManualEditRevision(active, nextProject, fileLabel, {
            instruction: format(t.tapp.playgroundManualEditInstruction, {
              file: fileLabel,
            }),
            explanation: format(t.tapp.playgroundManualEditExplanation, {
              file: fileLabel,
            }),
          })
        },
        true,
      ),
    )
  }, [commitStore, format, t])

  const handleCodeChange = (text: string) => {
    if (!project || busy) return
    setDraft(text)
    if (draftTimerRef.current) window.clearTimeout(draftTimerRef.current)
    const file = selectedFile
    draftTimerRef.current = window.setTimeout(applyDraft, 600, file, text)
  }

  const files: Array<{ id: FileId; label: string }> = [
    { id: 'page', label: FILE_LABELS.page },
    { id: 'html', label: FILE_LABELS.html },
    { id: 'styles', label: FILE_LABELS.styles },
    { id: 'core', label: FILE_LABELS.core },
    { id: 'manifest', label: FILE_LABELS.manifest },
    { id: 'i18n', label: FILE_LABELS.i18n },
    ...(project?.code.widget
      ? ([{ id: 'widget', label: FILE_LABELS.widget }] as const)
      : []),
    ...(project?.code.widgetHtml
      ? ([{ id: 'widgetHtml', label: FILE_LABELS.widgetHtml }] as const)
      : []),
    ...(Object.keys(project?.code.assets || {}).length
      ? ([{ id: 'assets', label: FILE_LABELS.assets }] as const)
      : []),
  ]

  const interactive = !isMobile

  /* 窗格内容 */

  const previewHeader = (
    <div className="flex items-center gap-2 min-w-0 w-full px-3">
      {interactive && (
        <FaGripVertical
          className="w-3 h-3 shrink-0"
          style={{ color: 'var(--text-muted)' }}
        />
      )}
      <span
        className={`w-2 h-2 rounded-full shrink-0 ${
          previewError
            ? 'bg-amber-500'
            : `bg-emerald-500 ${animConfig.loop && project ? 'animate-pulse' : ''}`
        }`}
      />
      <span
        className="text-xs font-medium truncate"
        style={{ color: 'var(--text-primary)' }}
      >
        {t.tapp.playgroundPreview}
      </span>
      {project && (
        <span
          className="ml-auto text-[10px] truncate"
          style={{ color: 'var(--text-muted)' }}
        >
          {project.manifest.name} · v{project.manifest.version}
        </span>
      )}
    </div>
  )

  const previewContent = (
    <>
      {tappInstance && project && hasUsablePage ? (
        <TappPageSandbox
          tappInstance={tappInstance}
          code={playgroundCodeToRuntime(project.manifest, project.code)}
          previewMode
          onError={handleSandboxError}
          onReady={() => setPreviewError('')}
          style={{ borderRadius: 0 }}
        />
      ) : project && isWidgetOnly ? (
        <div className="absolute inset-0 grid place-items-center p-6 text-center overflow-y-auto">
          <div>
            <h2 className="font-semibold text-gray-800 dark:text-gray-100">
              {t.tapp.playgroundNoPageTitle}
            </h2>
            <p className="mt-2 text-sm text-gray-500 dark:text-gray-400 max-w-sm leading-relaxed">
              {t.tapp.playgroundNoPageDesc}
            </p>
          </div>
        </div>
      ) : (
        <div className="absolute inset-0 grid place-items-center p-6 text-center overflow-y-auto">
          <motion.div
            initial={
              animationsEnabled ? { opacity: 0, scale: 0.94, y: 10 } : false
            }
            animate={{ opacity: 1, scale: 1, y: 0 }}
            transition={springTransition}
          >
            <div
              className="w-16 h-16 mx-auto rounded-[22px] grid place-items-center border"
              style={{
                color: 'var(--color-primary)',
                background:
                  'color-mix(in srgb, var(--color-primary) 10%, transparent)',
                borderColor:
                  'color-mix(in srgb, var(--color-primary) 15%, transparent)',
              }}
            >
              <TappPlaygroundIcon className="w-8 h-8" />
            </div>
            <h2 className="mt-4 font-bold text-gray-800 dark:text-gray-100">
              {t.tapp.playgroundEmptyTitle}
            </h2>
            <p className="mt-2 text-sm text-gray-500 dark:text-gray-400 max-w-sm leading-relaxed">
              {t.tapp.playgroundEmptyDesc}
            </p>
            <p className="mt-2 text-[11px] text-gray-400 dark:text-gray-500 max-w-sm">
              {t.tapp.playgroundCostHint}
            </p>
            <div className="mt-4 flex flex-wrap justify-center gap-1.5 max-w-md">
              {examplePrompts.map((example) => (
                <button
                  key={example.id}
                  type="button"
                  onClick={() => setInstruction(example.prompt)}
                  title={example.prompt}
                  className="rounded-full px-2.5 py-1 text-[11px] font-medium transition-colors"
                  style={{
                    color: 'var(--color-primary)',
                    background:
                      'color-mix(in srgb, var(--color-primary) 12%, transparent)',
                    border:
                      '1px solid color-mix(in srgb, var(--color-primary) 20%, transparent)',
                  }}
                >
                  {example.label}
                </button>
              ))}
            </div>
          </motion.div>
        </div>
      )}

      {/* 生成中的遮罩 */}
      <AnimatePresence>
        {busy && project && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="absolute inset-0 z-30 grid place-items-center glass-surface glass-45"
          >
            <div
              className="flex items-center justify-center rounded-full p-3 glass-surface glass-80 shadow-lg ring-1 ring-inset ring-black/5 dark:ring-white/10"
              role="status"
            >
              <Spinner size="sm" color="primary" />
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </>
  )

  const widgetRenderProps = useMemo(() => {
    const isDark = document.documentElement.classList.contains('dark')
    const primaryColor =
      getComputedStyle(document.documentElement)
        .getPropertyValue('--color-primary')
        .trim() || '#8b5cf6'
    // Playground widget pane must receive clicks (buttons/inputs). Catalog
    // previews keep isPreview:true → pointer-events:none; here we do not.
    return {
      size: activeWidgetSize,
      config: {},
      isEditMode: false,
      isPreview: false,
      theme: (isDark ? 'dark' : 'light') as 'light' | 'dark',
      primaryColor,
      locale,
    }
  }, [activeWidgetSize, locale])

  const widgetHeader = (
    <div className="flex items-center gap-2 min-w-0 w-full px-3">
      {interactive && (
        <FaGripVertical
          className="w-3 h-3 shrink-0"
          style={{ color: 'var(--text-muted)' }}
        />
      )}
      <span
        className="text-xs font-medium truncate"
        style={{ color: 'var(--text-primary)' }}
      >
        {t.tapp.playgroundWidgetPreview}
      </span>
      {activeWidget && (
        <div
          className="ml-auto flex items-center gap-1 shrink-0"
          onMouseDown={(event) => event.stopPropagation()}
          onTouchStart={(event) => event.stopPropagation()}
        >
          {manifestWidgets.length > 1 && (
            <select
              value={activeWidget.id}
              onChange={(event) => setWidgetId(event.target.value)}
              className="h-6 max-w-28 truncate rounded-md bg-black/5 dark:bg-white/10 px-1.5 text-[10px]"
              style={{ color: 'var(--text-secondary)' }}
              aria-label={t.tapp.playgroundWidgetPreview}
            >
              {manifestWidgets.map((widget) => (
                <option key={widget.id} value={widget.id}>
                  {widget.name || widget.id}
                </option>
              ))}
            </select>
          )}
          {(activeWidget.sizes || []).map((size) => (
            <button
              key={size}
              onClick={() => setWidgetSize(size)}
              className={`h-6 px-1.5 rounded-md text-[10px] font-mono transition-colors ${
                activeWidgetSize === size
                  ? 'bg-black/10 dark:bg-white/15'
                  : 'hover:bg-black/5 dark:hover:bg-white/10'
              }`}
              style={{
                color:
                  activeWidgetSize === size
                    ? 'var(--text-primary)'
                    : 'var(--text-muted)',
              }}
            >
              {size}
            </button>
          ))}
        </div>
      )}
    </div>
  )

  const widgetAspect = useMemo(() => {
    const [cols, rows] = activeWidgetSize
      .split('x')
      .map((part) => Number.parseInt(part, 10) || 1)
    return { cols, rows }
  }, [activeWidgetSize])

  const widgetContent =
    tappInstance && project && activeWidget ? (
      <div className="absolute inset-0 grid place-items-center p-4 overflow-hidden">
        <div
          className="rounded-2xl overflow-hidden shadow-lg ring-1 ring-black/5 dark:ring-white/10"
          style={{
            width: `min(100%, ${widgetAspect.cols * 130}px)`,
            aspectRatio: `${widgetAspect.cols} / ${widgetAspect.rows}`,
            maxHeight: '100%',
          }}
        >
          <TappWidgetSandbox
            key={`${activeWidget.id}-${activeWidgetSize}`}
            tappInstance={tappInstance}
            code={playgroundCodeToRuntime(project.manifest, project.code)}
            widgetId={activeWidget.id}
            widgetProps={widgetRenderProps}
            previewMode
            onError={handleWidgetError}
            onReady={() => setPreviewError('')}
            className="w-full h-full"
          />
        </div>
      </div>
    ) : null

  const codeHeader = (
    <div className="flex items-center gap-2 min-w-0 w-full px-3">
      {interactive && (
        <FaGripVertical
          className="w-3 h-3 shrink-0"
          style={{ color: 'var(--text-muted)' }}
        />
      )}
      <FaCode
        className="w-3 h-3 shrink-0"
        style={{ color: 'var(--color-primary)' }}
      />
      <span
        className="text-xs font-medium truncate"
        style={{ color: 'var(--text-primary)' }}
      >
        {t.tapp.playgroundCodeTitle}
      </span>
      {session.revisions.length >= 2 && (
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation()
            if (diffOpen) {
              setDiffOpen(false)
            } else {
              openDiffVsPrevious()
            }
          }}
          onMouseDown={(event) => event.stopPropagation()}
          className={`ml-1 shrink-0 h-6 px-2 rounded-md text-[10px] font-semibold flex items-center gap-1 transition-colors ${
            diffOpen
              ? 'bg-white/15 text-white'
              : 'text-gray-400 hover:text-gray-200 hover:bg-white/10'
          }`}
          title={
            diffOpen
              ? t.tapp.playgroundDiffClose
              : t.tapp.playgroundDiffVsPrevious
          }
          aria-pressed={diffOpen}
        >
          <FaExchangeAlt className="w-2.5 h-2.5" />
          {t.tapp.playgroundDiff}
        </button>
      )}
      {draftInvalid ? (
        <span className="ml-auto text-[10px] text-amber-500 whitespace-nowrap truncate">
          {t.tapp.playgroundInvalidJson}
        </span>
      ) : project ? (
        <span
          className="ml-auto text-[10px] font-mono"
          style={{ color: 'var(--text-muted)' }}
        >
          {session.revisionIndex + 1}/{session.revisions.length}
        </span>
      ) : null}
    </div>
  )

  // Playground is desktop-admin only; redirect mobile direct/bookmark URLs.
  if (isMobile) {
    return <Navigate to={TAPP_LIST_PATH} replace />
  }

  const codeValue =
    draft ??
    (project ? fileContents(project, selectedFile) : t.tapp.playgroundCodeEmpty)
  const codeReadOnly = !project || busy || selectedFile === 'assets' || diffOpen

  const diffStats = diffLines
    ? countDiffChanges(diffLines.lines)
    : { added: 0, removed: 0 }
  const sideBySideRows =
    diffLines && diffMode === 'side-by-side'
      ? toSideBySide(diffLines.lines)
      : null

  const codeContent = (
    <div className="absolute inset-0 flex flex-col bg-[#0d0f14] text-gray-200">
      <div className="h-9 shrink-0 flex items-center gap-1 px-2 overflow-x-auto border-b border-white/10">
        {files.map((file) => (
          <button
            key={file.id}
            onClick={() => setSelectedFile(file.id)}
            disabled={!project}
            className={`relative h-7 px-2.5 rounded-lg text-[11px] font-mono whitespace-nowrap transition-colors disabled:opacity-30 ${
              selectedFile === file.id
                ? 'text-white'
                : 'text-gray-500 hover:text-gray-300'
            }`}
          >
            {selectedFile === file.id && (
              <motion.span
                layoutId="playground-file-tab"
                transition={springTransition}
                className="absolute inset-0 rounded-lg bg-white/10"
              />
            )}
            <span className="relative z-10">{file.label}</span>
          </button>
        ))}
      </div>

      {diffOpen && session.revisions.length >= 2 && (
        <div className="shrink-0 flex flex-wrap items-center gap-2 px-2.5 py-1.5 border-b border-white/10 bg-white/[0.03]">
          <label className="flex items-center gap-1 text-[10px] text-gray-400">
            <span>{t.tapp.playgroundDiffBase}</span>
            <select
              value={diffLines?.baseIdx ?? diffBaseIndex ?? Math.max(0, session.revisionIndex - 1)}
              onChange={(event) => {
                setDiffBaseIndex(Number(event.target.value))
                setDiffOpen(true)
              }}
              className="h-6 rounded-md bg-black/40 border border-white/10 px-1.5 text-[10px] text-gray-200"
            >
              {session.revisions.map((rev, index) => (
                <option key={`base-${rev.id}`} value={index}>
                  v{index + 1}
                  {index === session.revisionIndex
                    ? ` · ${t.tapp.playgroundCurrentRevision}`
                    : ''}
                </option>
              ))}
            </select>
          </label>
          <label className="flex items-center gap-1 text-[10px] text-gray-400">
            <span>{t.tapp.playgroundDiffCompare}</span>
            <select
              value={
                diffLines?.compareIdx ??
                diffCompareIndex ??
                session.revisionIndex
              }
              onChange={(event) => {
                setDiffCompareIndex(Number(event.target.value))
                setDiffOpen(true)
              }}
              className="h-6 rounded-md bg-black/40 border border-white/10 px-1.5 text-[10px] text-gray-200"
            >
              {session.revisions.map((rev, index) => (
                <option key={`cmp-${rev.id}`} value={index}>
                  v{index + 1}
                  {index === session.revisionIndex
                    ? ` · ${t.tapp.playgroundCurrentRevision}`
                    : ''}
                </option>
              ))}
            </select>
          </label>
          <button
            type="button"
            onClick={openDiffVsPrevious}
            disabled={session.revisions.length < 2}
            className="h-6 px-2 rounded-md text-[10px] font-medium text-gray-300 bg-white/5 hover:bg-white/10 disabled:opacity-30"
          >
            {t.tapp.playgroundDiffVsPrevious}
          </button>
          <div className="flex items-center rounded-md bg-black/30 p-0.5">
            <button
              type="button"
              onClick={() => setDiffMode('unified')}
              className={`h-5 px-2 rounded text-[10px] font-medium ${
                diffMode === 'unified'
                  ? 'bg-white/15 text-white'
                  : 'text-gray-400'
              }`}
            >
              {t.tapp.playgroundDiffUnified}
            </button>
            <button
              type="button"
              onClick={() => setDiffMode('side-by-side')}
              className={`h-5 px-2 rounded text-[10px] font-medium ${
                diffMode === 'side-by-side'
                  ? 'bg-white/15 text-white'
                  : 'text-gray-400'
              }`}
            >
              {t.tapp.playgroundDiffSideBySide}
            </button>
          </div>
          {diffLines && (
            <span className="text-[10px] font-mono text-gray-400">
              <span className="text-emerald-400">
                {format(t.tapp.playgroundDiffAdded, { n: diffStats.added })}
              </span>
              {' '}
              <span className="text-red-400">
                {format(t.tapp.playgroundDiffRemoved, {
                  n: diffStats.removed,
                })}
              </span>
            </span>
          )}
          <button
            type="button"
            onClick={() => setDiffOpen(false)}
            className="ml-auto h-6 w-6 rounded-md grid place-items-center text-gray-400 hover:text-white hover:bg-white/10"
            aria-label={t.tapp.playgroundDiffClose}
          >
            <FaTimes className="w-2.5 h-2.5" />
          </button>
        </div>
      )}

      <div className="flex-1 min-h-0 overflow-auto">
        {diffOpen ? (
          !diffLines ? (
            <div className="p-4 text-xs text-gray-500">
              {t.tapp.playgroundDiffEmpty}
            </div>
          ) : diffStats.added === 0 && diffStats.removed === 0 ? (
            <div className="p-4 text-xs text-gray-500">
              {t.tapp.playgroundDiffNoChanges}
            </div>
          ) : diffMode === 'side-by-side' && sideBySideRows ? (
            <div className="playground-diff playground-diff--side">
              <div className="playground-diff-side-head">
                <span>
                  v{(diffLines.baseIdx ?? 0) + 1}
                </span>
                <span>
                  v{(diffLines.compareIdx ?? 0) + 1}
                </span>
              </div>
              {sideBySideRows.map((row, index) => (
                <div key={`sbs-${index}`} className="playground-diff-side-row">
                  <div
                    className={`playground-diff-cell playground-diff-cell--${row.left.op}`}
                  >
                    <span className="playground-diff-ln">
                      {row.left.line ?? ''}
                    </span>
                    <span className="playground-diff-text">
                      {row.left.text}
                    </span>
                  </div>
                  <div
                    className={`playground-diff-cell playground-diff-cell--${row.right.op}`}
                  >
                    <span className="playground-diff-ln">
                      {row.right.line ?? ''}
                    </span>
                    <span className="playground-diff-text">
                      {row.right.text}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="playground-diff playground-diff--unified">
              {diffLines.lines.map((line, index) => (
                <div
                  key={`u-${index}`}
                  className={`playground-diff-line playground-diff-line--${line.op}`}
                >
                  <span className="playground-diff-ln">
                    {line.op === 'add'
                      ? line.newLine ?? ''
                      : line.oldLine ?? ''}
                  </span>
                  <span className="playground-diff-sign">
                    {line.op === 'add' ? '+' : line.op === 'remove' ? '−' : ' '}
                  </span>
                  <span className="playground-diff-text">{line.text}</span>
                </div>
              ))}
            </div>
          )
        ) : (
          <CodeEditor
            value={codeValue}
            language={FILE_LANGUAGE[selectedFile]}
            readOnly={codeReadOnly}
            label={
              files.find((file) => file.id === selectedFile)?.label ||
              selectedFile
            }
            onChange={handleCodeChange}
          />
        )}
      </div>
    </div>
  )

  return (
    <motion.div
      ref={attachWorkspace}
      initial={animationsEnabled ? { opacity: 0 } : false}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3 * animConfig.durationScale }}
      className={`fixed inset-0 z-100 ${
        interactive ? 'overflow-hidden' : 'overflow-y-auto'
      }`}
      data-no-ripple
    >
      {/* 顶部工具栏 - 与多窗口运行页一致的浮动样式 */}
      <div className="absolute top-3.5 left-3.5 z-40">
        <div
          className="flex items-center gap-1.5 rounded-xl pl-1.5 pr-3 py-1.5 glass-surface glass-80"
          style={{
            border: '1px solid var(--border-color)',
            boxShadow: '0 4px 12px rgba(0, 0, 0, 0.1)',
          }}
        >
          <motion.button
            onClick={() => navigate(TAPP_LIST_PATH)}
            whileTap={animationsEnabled ? { scale: 0.92 } : {}}
            className="w-8 h-8 rounded-lg grid place-items-center transition-colors hover:bg-black/5 dark:hover:bg-white/10"
            style={{ color: 'var(--text-secondary)' }}
            aria-label={t.tapp.back}
          >
            <FaArrowLeft className="w-3.5 h-3.5" />
          </motion.button>
          <div
            className="w-px h-5 mx-0.5"
            style={{ backgroundColor: 'var(--border-color)' }}
          />
          <div
            className="w-7 h-7 rounded-lg grid place-items-center text-white shadow-sm"
            style={{
              background:
                'linear-gradient(135deg, var(--color-primary), color-mix(in srgb, var(--color-primary) 80%, black))',
            }}
          >
            <TappPlaygroundIcon className="w-4 h-4" />
          </div>
          <div className="flex flex-col min-w-0">
            <span
              className="text-sm font-semibold leading-tight whitespace-nowrap"
              style={{ color: 'var(--text-primary)' }}
            >
              {t.tapp.playgroundTitle}
            </span>
            <span
              className="hidden sm:flex items-center gap-1 text-[10px] leading-tight whitespace-nowrap cursor-help"
              style={{ color: 'var(--text-muted)' }}
              title={t.tapp.playgroundIsolationDesc}
            >
              <FaLock className="w-2.5 h-2.5" />
              {t.tapp.playgroundIsolationTitle}
            </span>
          </div>
        </div>
      </div>

      {/* 工作区窗格 */}
      {interactive ? (
        defaultLayout && (
          <>
            {defaultLayout.preview && (
              <FloatingPane
                key={
                  defaultLayout.widget
                    ? 'preview-split'
                    : isWidgetOnly
                      ? 'preview-hidden'
                      : 'preview-full'
                }
                defaultRect={defaultLayout.preview}
                bounds={bounds}
                isActive={activePane === 'preview'}
                onFocus={() => setActivePane('preview')}
                header={previewHeader}
                interactive
              >
                {previewContent}
              </FloatingPane>
            )}
            {defaultLayout.widget && (
              <FloatingPane
                key={isWidgetOnly ? 'widget-full' : 'widget-split'}
                defaultRect={defaultLayout.widget}
                bounds={bounds}
                isActive={activePane === 'widget'}
                onFocus={() => setActivePane('widget')}
                header={widgetHeader}
                interactive
              >
                {widgetContent}
              </FloatingPane>
            )}
            <FloatingPane
              defaultRect={defaultLayout.code}
              bounds={bounds}
              isActive={activePane === 'code'}
              onFocus={() => setActivePane('code')}
              header={codeHeader}
              interactive
            >
              {codeContent}
            </FloatingPane>
          </>
        )
      ) : (
        <div className="flex flex-col gap-3 px-3 pt-16 pb-48">
          {hasUsablePage && (
            <FloatingPane
              defaultRect={{ x: 0, y: 0, width: 0, height: 0 }}
              bounds={bounds}
              isActive
              onFocus={() => {}}
              header={previewHeader}
              interactive={false}
              staticClassName="h-[56vh]"
            >
              {previewContent}
            </FloatingPane>
          )}
          {hasWidgetPreview && (
            <FloatingPane
              defaultRect={{ x: 0, y: 0, width: 0, height: 0 }}
              bounds={bounds}
              isActive
              onFocus={() => {}}
              header={widgetHeader}
              interactive={false}
              staticClassName={isWidgetOnly ? 'h-[56vh]' : 'h-[36vh]'}
            >
              {widgetContent}
            </FloatingPane>
          )}
          {!project && (
            <FloatingPane
              defaultRect={{ x: 0, y: 0, width: 0, height: 0 }}
              bounds={bounds}
              isActive
              onFocus={() => {}}
              header={previewHeader}
              interactive={false}
              staticClassName="h-[56vh]"
            >
              {previewContent}
            </FloatingPane>
          )}
          <FloatingPane
            defaultRect={{ x: 0, y: 0, width: 0, height: 0 }}
            bounds={bounds}
            isActive
            onFocus={() => {}}
            header={codeHeader}
            interactive={false}
            staticClassName="h-[42vh]"
          >
            {codeContent}
          </FloatingPane>
        </div>
      )}

      {/* 底部控制岛（Composer） */}
      <PlaygroundComposer
        interactive={interactive}
        busy={busy}
        busyMode={busyMode}
        busyStepSummary={busyStepSummary}
        installing={installing}
        exporting={exporting}
        hasProject={!!project}
        instruction={instruction}
        revisionIndex={session.revisionIndex}
        revisionCount={session.revisions.length}
        historyRevisions={historyRevisions}
        sessions={sessionSummaries}
        activeSessionId={store.activeSessionId}
        error={error}
        previewError={previewError}
        notice={notice}
        warnings={revision?.warnings || []}
        agentTrace={revision?.agentTrace}
        knowledgeSources={revision?.knowledgeSources}
        validation={revision?.validation}
        lastFailedAttempt={session.lastFailedAttempt || null}
        lastSuccessElapsedMs={lastSuccessElapsedMs}
        examplePrompts={examplePrompts}
        capabilityNote={
          capabilityNoteVisible ? t.tapp.playgroundPreviewCapabilities : ''
        }
        storageNotice={pruneNotice}
        onInstructionChange={setInstruction}
        onSubmit={() => void runGeneration()}
        onCancel={cancelGeneration}
        onInstall={() => void installProject()}
        onExport={() => void exportProject()}
        onMoveRevision={moveRevision}
        onJumpToRevision={jumpToRevision}
        onClear={clearSession}
        onCreateSession={handleCreateSession}
        onSwitchSession={handleSwitchSession}
        onDeleteSession={handleDeleteSession}
        onDismissError={() => setError('')}
        onDismissPreviewError={() => setPreviewError('')}
        onDismissNotice={() => setNotice('')}
        onRetryFailed={() => void retryFailedAttempt()}
        onDismissFailed={dismissFailedAttempt}
        onPickExample={(prompt) => setInstruction(prompt)}
        onDismissCapabilityNote={() => {
          setCapabilityNoteVisible(false)
          try {
            sessionStorage.setItem(CAPABILITY_NOTE_DISMISS_KEY, '1')
          } catch {
            // ignore
          }
        }}
        onDismissStorageNotice={() => setPruneNotice('')}
      />
    </motion.div>
  )
}

export default TappPlaygroundPage
