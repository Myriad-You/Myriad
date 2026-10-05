import type { AnnotationItem } from '../../../../services/phantasiaiApi'
import type { ReaderTooltipAnchor } from '../tooltipPlacement'
import type { ReaderCopy } from '../types'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import * as phantasiaiApi from '../../../../services/phantasiaiApi'
import { userFacingError } from '../../../../utils/userFacingError'
import { RequestTurn } from '../../logic/requestTurn'
import { useArticleTaskScope } from './useArticleTaskScope'

interface UseAnnotationsOptions {
  itemId: number
  isPhantasiai: boolean
  showToastMessage: (message: string, duration?: number) => void
  t: ReaderCopy
}

interface UseAnnotationsReturn {
  annotations: AnnotationItem[]
  annotationsLoading: boolean
  annotationsError: string | null
  showAnnotations: boolean
  selectedAnnotation: AnnotationItem | null
  showPhantasiaiPanel: boolean
  hoveredAnnotation: AnnotationItem | null
  tooltipPosition: ReaderTooltipAnchor

  setAnnotations: (annotations: AnnotationItem[]) => void
  setShowAnnotations: (show: boolean) => void
  setSelectedAnnotation: (annotation: AnnotationItem | null) => void
  setShowPhantasiaiPanel: (show: boolean) => void
  setHoveredAnnotation: (annotation: AnnotationItem | null) => void
  setTooltipPosition: (anchor: ReaderTooltipAnchor) => void
  loadAnnotations: () => Promise<void>
  regenerateAnnotations: () => Promise<void>
  toggleAnnotations: () => void
  scrollToAnnotation: (
    annotation: AnnotationItem,
    contentRef: React.RefObject<HTMLDivElement | null>,
    articleRef: React.RefObject<HTMLElement | null>,
  ) => void

  hoverTimeoutRef: React.RefObject<ReturnType<typeof setTimeout> | null>
  annotationsLoadingRef: React.RefObject<boolean>
}

export function useAnnotations({
  itemId,
  isPhantasiai,
  showToastMessage,
  t,
}: UseAnnotationsOptions): UseAnnotationsReturn {
  const { format } = useI18n()
  const captureTask = useArticleTaskScope(itemId)
  const turns = useRef(new RequestTurn())
  useEffect(() => () => turns.current.cancel(), [itemId])
  const [annotations, setAnnotations] = useState<AnnotationItem[]>([])
  const [annotationsLoading, setAnnotationsLoading] = useState(false)
  const [annotationsError, setAnnotationsError] = useState<string | null>(null)
  const [showAnnotations, setShowAnnotations] = useState(false)
  const [selectedAnnotation, setSelectedAnnotation] =
    useState<AnnotationItem | null>(null)
  const [showPhantasiaiPanel, setShowPhantasiaiPanel] = useState(false)
  const [hoveredAnnotation, setHoveredAnnotation] =
    useState<AnnotationItem | null>(null)
  const [tooltipPosition, setTooltipPosition] = useState<ReaderTooltipAnchor>({
    x: 0,
    top: 0,
    bottom: 0,
  })

  const hoverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const annotationsLoadingRef = useRef(false)

  const loadAnnotations = useCallback(async () => {
    if (!isPhantasiai || annotationsLoadingRef.current) return

    if (annotations.length > 0) {
      setShowAnnotations(true)
      return
    }

    const isCurrent = captureTask()
    const signal = turns.current.begin()
    annotationsLoadingRef.current = true
    setAnnotationsLoading(true)
    setAnnotationsError(null)

    try {
      const response = await phantasiaiApi.getAnnotations(itemId, signal)
      if (!isCurrent() || signal.aborted) return

      if (response.success) {
        setAnnotations(response.annotations)
        setShowAnnotations(true)
        const cacheHint = response.from_cache ? t.phantasi.fromCache : ''
        showToastMessage(
          `${format(t.phantasi.foundAnnotations, { count: response.annotations.length })}${cacheHint}`,
        )
      } else {
        setAnnotationsError(
          userFacingError(response.error, t.phantasi.fetchAnnotationFailed),
        )
      }
    } catch (err) {
      if (!isCurrent() || signal.aborted) return
      console.error('Failed to load annotations:', err)
      setAnnotationsError(
        userFacingError(err, t.phantasi.fetchAnnotationFailed),
      )
    } finally {
      annotationsLoadingRef.current = false
      if (isCurrent() && !signal.aborted) setAnnotationsLoading(false)
    }
  }, [captureTask, format, isPhantasiai, annotations.length, itemId, showToastMessage, t])

  const regenerateAnnotations = useCallback(async () => {
    if (!isPhantasiai || annotationsLoading) return

    const isCurrent = captureTask()
    const signal = turns.current.begin()
    setAnnotationsLoading(true)
    setAnnotationsError(null)

    try {
      const response = await phantasiaiApi.regenerateAnnotations(itemId, signal)
      if (!isCurrent() || signal.aborted) return

      if (response.success) {
        setAnnotations(response.annotations)
        setShowAnnotations(true)
        showToastMessage(
          format(t.phantasi.regeneratedAnnotations, {
            count: response.annotations.length,
          }),
        )
      } else {
        setAnnotationsError(response.error || t.phantasi.regenerateFailed)
      }
    } catch (err) {
      if (!isCurrent() || signal.aborted) return
      console.error('Failed to regenerate annotations:', err)
      setAnnotationsError(userFacingError(err, t.phantasi.regenerateFailed))
    } finally {
      if (isCurrent() && !signal.aborted) setAnnotationsLoading(false)
    }
  }, [
    captureTask,
    isPhantasiai,
    annotationsLoading,
    itemId,
    showToastMessage,
    t,
    format,
  ])

  const toggleAnnotations = useCallback(() => {
    if (annotations.length === 0) {
      loadAnnotations()
    } else {
      setShowAnnotations((prev) => !prev)
    }
  }, [annotations.length, loadAnnotations])

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

        mark.classList.add('phantasiai-highlight-flash')
        setTimeout(() => mark.classList.remove('phantasiai-highlight-flash'), 1500)

        setShowPhantasiaiPanel(false)
      }
    },
    [annotations],
  )

  return {
    annotations,
    annotationsLoading,
    annotationsError,
    showAnnotations,
    selectedAnnotation,
    showPhantasiaiPanel,
    hoveredAnnotation,
    tooltipPosition,

    setAnnotations,
    setShowAnnotations,
    setSelectedAnnotation,
    setShowPhantasiaiPanel,
    setHoveredAnnotation,
    setTooltipPosition,
    loadAnnotations,
    regenerateAnnotations,
    toggleAnnotations,
    scrollToAnnotation,

    hoverTimeoutRef,
    annotationsLoadingRef,
  }
}
