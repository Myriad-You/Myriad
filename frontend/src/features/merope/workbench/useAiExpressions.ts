import type { AuthoredExpressionKind } from '@myriad/figure/rig'
import type { SiteExpressions, SiteExpressionUrls } from '../api'
import { AUTHORED_EXPRESSION_KINDS } from '@myriad/figure/rig'
import { useCallback, useEffect, useRef, useState } from 'react'
import { expressionLost, listSiteExpressions, startSiteExpression } from '../api'
import { drawingFailures, untilNoneDrawn } from './serverDrawing'
import { useServerGeneration } from './useServerGeneration'

/** AI expression enhancement: redraws of the current portrait, kept per portrait. */
export function useAiExpressions(portraitUrl: string | null) {
  const [urls, setUrls] = useState<SiteExpressionUrls>({})
  // Preflight reads the latest redraws, even right after generating them.
  const urlsRef = useRef<SiteExpressionUrls>({})
  urlsRef.current = urls
  // Being drawn on the server: asked for by this page, begun before a
  // reload, or in another tab.
  const [drawing, setDrawing] = useState<AuthoredExpressionKind[]>([])
  const drawingRef = useRef<AuthoredExpressionKind[]>([])
  drawingRef.current = drawing
  // Why the ones the server could not draw were not kept, as the list read last has it.
  const failedRef = useRef<SiteExpressions['failed']>({})
  const listedRef = useRef<Promise<void>>(Promise.resolve())
  const waitersRef = useRef<Array<() => void>>([])

  const take = useCallback((listed: SiteExpressions) => {
    urlsRef.current = listed.urls
    drawingRef.current = listed.drawing
    failedRef.current = listed.failed
    setUrls(listed.urls)
    setDrawing(listed.drawing)
    // None being drawn: the ones waiting for that go on, whether or not the page ever showed some as drawn.
    if (listed.drawing.length === 0) {
      for (const resolve of waitersRef.current.splice(0)) resolve()
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    if (!portraitUrl) {
      setUrls({})
      setDrawing([])
      return
    }
    listedRef.current = listSiteExpressions()
      .then((listed) => {
        if (!cancelled) take(listed)
      })
      .catch(() => {
        if (!cancelled) setUrls({})
      })
    return () => {
      cancelled = true
    }
  }, [portraitUrl, take])

  const reread = useCallback(async () => take(await listSiteExpressions()), [take])
  // Once nothing is drawn any more, the list read last has the new redraws.
  const busy = useServerGeneration(drawing.length > 0, reread, () => {
    for (const resolve of waitersRef.current.splice(0)) resolve()
  })

  /** Resolves once the server draws none of them any more. */
  const settled = useCallback(async () => {
    await listedRef.current
    if (drawingRef.current.length === 0) return
    await new Promise<void>((resolve) => waitersRef.current.push(resolve))
  }, [])

  /** Redraws every expression. The server draws them on its own once asked; this resolves when it draws none any more. */
  const generate = useCallback(async () => {
    const before = urlsRef.current
    // Ones the server is drawing already are waited for, not asked again.
    const already = drawingRef.current
    const asked = AUTHORED_EXPRESSION_KINDS.filter((kind) => !already.includes(kind))
    // Each ask is answered as soon as the server has taken it up: none waits for a connection another holds.
    const begun = await Promise.allSettled(asked.map((kind) => startSiteExpression(kind)))
    const taken = asked.filter((_, index) => begun[index].status === 'fulfilled')
    if (taken.length > 0) {
      // Shown as being drawn at once; the list is read every few seconds while any is.
      const marked = [...new Set([...drawingRef.current, ...taken])]
      drawingRef.current = marked
      setDrawing(marked)
    }
    if (taken.length > 0 || already.length > 0) {
      await untilNoneDrawn({ settled, reread, drawing: () => drawingRef.current })
    }
    const failures = drawingFailures(
      asked,
      begun,
      { before: (kind) => before[kind], after: (kind) => urlsRef.current[kind] },
      failedRef.current,
      expressionLost,
    )
    if (failures.length > 0) throw failures[0]
  }, [reread, settled])

  /** What an import should cut into expression parts right now. */
  const references = useCallback(
    () =>
      AUTHORED_EXPRESSION_KINDS.flatMap((kind) => {
        const url = urlsRef.current[kind]
        return url ? [{ kind, url }] : []
      }),
    [],
  )

  return {
    ready: AUTHORED_EXPRESSION_KINDS.filter((kind) => urls[kind]),
    /** Some are being drawn on the server, asked for here or begun elsewhere. */
    drawing: busy,
    generate,
    settled,
    references,
  }
}
