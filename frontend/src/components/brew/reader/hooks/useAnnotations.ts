import type { AnnotationItem } from '../../../../services/brewliaApi'
import type { ReaderCopy } from '../types'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import * as brewliaApi from '../../../../services/brewliaApi'
import { userFacingError } from '../../../../utils/userFacingError'
import { RequestTurn } from '../../logic/requestTurn'
import { useArticleTaskScope } from './useArticleTaskScope'

export interface UseAnnotationsOptions {
  itemId: number
  isBrewlia: boolean
  showToastMessage: (message: string, duration?: number) => void
  t: ReaderCopy
}

export interface UseAnnotationsReturn {
  annotations: AnnotationItem[]
  annotationsLoading: boolean
  annotationsError: string | null
  showAnnotations: boolean
  selectedAnnotation: AnnotationItem | null
  showBrewliaPanel: boolean
  hoveredAnnotation: AnnotationItem | null
  tooltipPosition: { x: number; y: number }

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

  hoverTimeoutRef: React.RefObject<ReturnType<typeof setTimeout> | null>
  annotationsLoadingRef: React.RefObject<boolean>
}

export function useAnnotations({
  itemId,
  isBrewlia,
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
  const [showBrewliaPanel, setShowBrewliaPanel] = useState(false)
  const [hoveredAnnotation, setHoveredAnnotation] =
    useState<AnnotationItem | null>(null)
  const [tooltipPosition, setTooltipPosition] = useState({ x: 0, y: 0 })

  const hoverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const annotationsLoadingRef = useRef(false)

  const loadAnnotations = useCallback(async () => {
    if (!isBrewlia || annotationsLoadingRef.current) return

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
      const response = await brewliaApi.getAnnotations(itemId, signal)
      if (!isCurrent() || signal.aborted) return

      if (response.success) {
        setAnnotations(response.annotations)
        setShowAnnotations(true)
        const cacheHint = response.from_cache ? t.brew.fromCache : ''
        showToastMessage(
          `${format(t.brew.foundAnnotations, { count: response.annotations.length })}${cacheHint}`,
        )
      } else {
        setAnnotationsError(
          userFacingError(response.error, t.brew.fetchAnnotationFailed),
        )
      }
    } catch (err) {
      if (!isCurrent() || signal.aborted) return
      console.error('Failed to load annotations:', err)
      setAnnotationsError(
        userFacingError(err, t.brew.fetchAnnotationFailed),
      )
    } finally {
      annotationsLoadingRef.current = false
      if (isCurrent() && !signal.aborted) setAnnotationsLoading(false)
    }
  }, [captureTask, format, isBrewlia, annotations.length, itemId, showToastMessage, t])

  const regenerateAnnotations = useCallback(async () => {
    if (!isBrewlia || annotationsLoading) return

    const isCurrent = captureTask()
    const signal = turns.current.begin()
    setAnnotationsLoading(true)
    setAnnotationsError(null)

    try {
      const response = await brewliaApi.regenerateAnnotations(itemId, signal)
      if (!isCurrent() || signal.aborted) return

      if (response.success) {
        setAnnotations(response.annotations)
        setShowAnnotations(true)
        showToastMessage(
          format(t.brew.regeneratedAnnotations, {
            count: response.annotations.length,
          }),
        )
      } else {
        setAnnotationsError(response.error || t.brew.regenerateFailed)
      }
    } catch (err) {
      if (!isCurrent() || signal.aborted) return
      console.error('Failed to regenerate annotations:', err)
      setAnnotationsError(userFacingError(err, t.brew.regenerateFailed))
    } finally {
      if (isCurrent() && !signal.aborted) setAnnotationsLoading(false)
    }
  }, [
    captureTask,
    isBrewlia,
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

        mark.classList.add('brewlia-highlight-flash')
        setTimeout(() => mark.classList.remove('brewlia-highlight-flash'), 1500)

        setShowBrewliaPanel(false)
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
    showBrewliaPanel,
    hoveredAnnotation,
    tooltipPosition,

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

    hoverTimeoutRef,
    annotationsLoadingRef,
  }
}
