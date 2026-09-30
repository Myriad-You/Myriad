import type { SiteExpressionUrls } from '../api'
import { useCallback, useEffect, useRef, useState } from 'react'
import { generateSiteExpression, listSiteExpressions } from '../api'
import { AUTHORED_EXPRESSION_KINDS } from '../rig/authoredExpression'

/** AI expression enhancement: redraws of the current portrait, kept per portrait. */
export function useAiExpressions(portraitUrl: string | null) {
  const [urls, setUrls] = useState<SiteExpressionUrls>({})
  // Preflight reads the latest redraws, even right after generating them.
  const urlsRef = useRef<SiteExpressionUrls>({})
  urlsRef.current = urls

  useEffect(() => {
    let cancelled = false
    if (!portraitUrl) {
      setUrls({})
      return
    }
    void listSiteExpressions()
      .then((listed) => {
        if (!cancelled) setUrls(listed)
      })
      .catch(() => {
        if (!cancelled) setUrls({})
      })
    return () => {
      cancelled = true
    }
  }, [portraitUrl])

  const generate = useCallback(async () => {
    const results = await Promise.allSettled(
      AUTHORED_EXPRESSION_KINDS.map(async (kind) => ({
        kind,
        url: await generateSiteExpression(kind),
      })),
    )
    const drawn: SiteExpressionUrls = {}
    for (const result of results) {
      if (result.status === 'fulfilled') drawn[result.value.kind] = result.value.url
    }
    const next = { ...urlsRef.current, ...drawn }
    urlsRef.current = next
    setUrls(next)
    const failure = results.find((result) => result.status === 'rejected')
    if (failure) throw failure.reason
  }, [])

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
    generate,
    references,
  }
}
