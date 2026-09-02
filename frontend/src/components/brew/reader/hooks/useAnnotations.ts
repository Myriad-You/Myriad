/**
 * AI 注释 Hook
 * 管理 Brewlia AI 注释的加载、显示、高亮等功能
 */

import type { AnnotationItem } from '../../../../services/brewliaApi'
import { useCallback, useRef, useState } from 'react'
import * as brewliaApi from '../../../../services/brewliaApi'
import { userFacingError } from '../../../../utils/userFacingError'

export interface UseAnnotationsOptions {
  itemId: number
  isBrewlia: boolean
  showToastMessage: (message: string, duration?: number) => void
  t: Record<string, any>
}

export interface UseAnnotationsReturn {
  // 状态
  annotations: AnnotationItem[]
  annotationsLoading: boolean
  annotationsError: string | null
  showAnnotations: boolean
  selectedAnnotation: AnnotationItem | null
  showBrewliaPanel: boolean
  hoveredAnnotation: AnnotationItem | null
  tooltipPosition: { x: number; y: number }

  // 操作
  setAnnotations: (annotations: AnnotationItem[]) => void
  setShowAnnotations: (show: boolean) => void
  setSelectedAnnotation: (annotation: AnnotationItem | null) => void
  setShowBrewliaPanel: (show: boolean) => void
  setHoveredAnnotation: (annotation: AnnotationItem | null) => void
  setTooltipPosition: (position: { x: number; y: number }) => void
  loadAnnotations: () => Promise<void>
  regenerateAnnotations: () => Promise<void>
  toggleAnnotations: () => void
  scrollToAnnotation: (
    annotation: AnnotationItem,
    contentRef: React.RefObject<HTMLDivElement | null>,
    articleRef: React.RefObject<HTMLElement | null>,
  ) => void

  // Refs
  hoverTimeoutRef: React.RefObject<ReturnType<typeof setTimeout> | null>
  annotationsLoadingRef: React.RefObject<boolean>
}

export function useAnnotations({
  itemId,
  isBrewlia,
  showToastMessage,
  t,
}: UseAnnotationsOptions): UseAnnotationsReturn {
  // 状态
  const [annotations, setAnnotations] = useState<AnnotationItem[]>([])
  const [annotationsLoading, setAnnotationsLoading] = useState(false)
  const [annotationsError, setAnnotationsError] = useState<string | null>(null)
  const [showAnnotations, setShowAnnotations] = useState(false)
  const [selectedAnnotation, setSelectedAnnotation] =
    useState<AnnotationItem | null>(null)
  const [showBrewliaPanel, setShowBrewliaPanel] = useState(false)
  const [hoveredAnnotation, setHoveredAnnotation] =
    useState<AnnotationItem | null>(null)
  const [tooltipPosition, setTooltipPosition] = useState({ x: 0, y: 0 })

  // Refs
  const hoverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const annotationsLoadingRef = useRef(false)

  // 加载注释
  const loadAnnotations = useCallback(async () => {
    if (!isBrewlia || annotationsLoadingRef.current) return

    // 如果已有注释，直接显示
    if (annotations.length > 0) {
      setShowAnnotations(true)
      return
    }

    annotationsLoadingRef.current = true
    setAnnotationsLoading(true)
    setAnnotationsError(null)

    try {
      const response = await brewliaApi.getAnnotations(itemId)

      if (response.success) {
        setAnnotations(response.annotations)
        setShowAnnotations(true)
        const cacheHint = response.from_cache ? t.brew.fromCache : ''
        showToastMessage(
          `${t.brew.foundAnnotations.replace('{count}', String(response.annotations.length))}${cacheHint}`,
        )
      } else {
        setAnnotationsError(
          userFacingError(response.error, t.brew.fetchAnnotationFailed),
        )
      }
    } catch (err) {
      console.error('Failed to load annotations:', err)
      setAnnotationsError(
        userFacingError(err, t.brew.fetchAnnotationFailed),
      )
    } finally {
      setAnnotationsLoading(false)
      annotationsLoadingRef.current = false
    }
  }, [isBrewlia, annotations.length, itemId, showToastMessage, t])

  // 重新生成注释
  const regenerateAnnotations = useCallback(async () => {
    if (!isBrewlia || annotationsLoading) return

    setAnnotationsLoading(true)
    setAnnotationsError(null)

    try {
      const response = await brewliaApi.regenerateAnnotations(itemId)

      if (response.success) {
        setAnnotations(response.annotations)
        setShowAnnotations(true)
        showToastMessage(
          t.brew.regeneratedAnnotations.replace(
            '{count}',
            String(response.annotations.length),
          ),
        )
      } else {
        setAnnotationsError(response.error || t.brew.regenerateFailed)
      }
    } catch (err) {
      console.error('Failed to regenerate annotations:', err)
      setAnnotationsError(userFacingError(err, t.brew.regenerateFailed))
    } finally {
      setAnnotationsLoading(false)
    }
  }, [isBrewlia, annotationsLoading, itemId, showToastMessage, t])

  // 切换注释显示
  const toggleAnnotations = useCallback(() => {
    if (annotations.length === 0) {
      loadAnnotations()
    } else {
      setShowAnnotations((prev) => !prev)
    }
  }, [annotations.length, loadAnnotations])

  // 跳转到注释位置
  const scrollToAnnotation = useCallback(
    (
      annotation: AnnotationItem,
      contentRef: React.RefObject<HTMLDivElement | null>,
      articleRef: React.RefObject<HTMLElement | null>,
    ) => {
      const annotationId =
        annotation.id ||
        `${annotation.type}-${annotations.indexOf(annotation) + 1}`
      const mark = contentRef.current?.querySelector(
        `mark[data-annotation-id="${annotationId}"]`,
      )

      if (mark && articleRef.current) {
        const articleRect = articleRef.current.getBoundingClientRect()
        const markRect = mark.getBoundingClientRect()
        const scrollTop =
          articleRef.current.scrollTop + markRect.top - articleRect.top - 150

        articleRef.current.scrollTo({
          top: scrollTop,
          behavior: 'smooth',
        })

        // 高亮闪烁效果
        mark.classList.add('brewlia-highlight-flash')
        setTimeout(() => mark.classList.remove('brewlia-highlight-flash'), 1500)

        // 关闭面板
        setShowBrewliaPanel(false)
      }
    },
    [annotations],
  )

  return {
    // 状态
    annotations,
    annotationsLoading,
    annotationsError,
    showAnnotations,
    selectedAnnotation,
    showBrewliaPanel,
    hoveredAnnotation,
    tooltipPosition,

    // 操作
    setAnnotations,
    setShowAnnotations,
    setSelectedAnnotation,
    setShowBrewliaPanel,
    setHoveredAnnotation,
    setTooltipPosition,
    loadAnnotations,
    regenerateAnnotations,
    toggleAnnotations,
    scrollToAnnotation,

    // Refs
    hoverTimeoutRef,
    annotationsLoadingRef,
  }
}
