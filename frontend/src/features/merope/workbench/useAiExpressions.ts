import type { AuthoredExpressionKind } from '@myriad/figure/rig/authoredExpression'
import type { SiteExpressionUrls } from '../api'
import { AUTHORED_EXPRESSION_KINDS } from '@myriad/figure/rig/authoredExpression'
import { useCallback, useEffect, useRef, useState } from 'react'
import { generateSiteExpression, listSiteExpressions } from '../api'
import { useServerGeneration } from './useServerGeneration'

/** AI expression enhancement: redraws of the current portrait, kept per portrait. */
export function useAiExpressions(portraitUrl: string | null) {
  const [urls, setUrls] = useState<SiteExpressionUrls>({})
  // Preflight reads the latest redraws, even right after generating them.
  const urlsRef = useRef<SiteExpressionUrls>({})
  urlsRef.current = urls
  // Being drawn on the server without this page asking: begun before a
  // reload, or in another tab.
  const [drawing, setDrawing] = useState<AuthoredExpressionKind[]>([])
  const drawingRef = useRef<AuthoredExpressionKind[]>([])
  drawingRef.current = drawing
  const listedRef = useRef<Promise<void>>(Promise.resolve())
  const waitersRef = useRef<Array<() => void>>([])

  const take = useCallback((listed: Awaited<ReturnType<typeof listSiteExpressions>>) => {
    urlsRef.current = listed.urls
    drawingRef.current = listed.drawing
    setUrls(listed.urls)
    setDrawing(listed.drawing)
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

  const generate = useCallback(async () => {
    // Ones the server is drawing already are waited for, not asked again.
    const already = drawingRef.current
    const results = await Promise.allSettled(
      AUTHORED_EXPRESSION_KINDS.filter((kind) => !already.includes(kind)).map(async (kind) => ({
        kind,
        url: await generateSiteExpression(kind),
      })),
    )
    if (already.length > 0) await settled()
    const drawn: SiteExpressionUrls = {}
    for (const result of results) {
      if (result.status === 'fulfilled') drawn[result.value.kind] = result.value.url
    }
    const next = { ...urlsRef.current, ...drawn }
    urlsRef.current = next
    setUrls(next)
    const failure = results.find((result) => result.status === 'rejected')
    if (failure) throw failure.reason
  }, [settled])

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
    /** Some are being drawn on the server, begun elsewhere. */
    drawing: busy,
    generate,
    settled,
    references,
  }
}
