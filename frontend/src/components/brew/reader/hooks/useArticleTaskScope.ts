import { useCallback, useLayoutEffect, useRef } from 'react'
import { brewSubject } from '../../../../utils/brewSubject'

/** Async continuations belong to one mounted article and one auth lifetime. */
export function useArticleTaskScope(itemId: number): () => () => boolean {
  const lifetime = useRef<object | null>(null)
  useLayoutEffect(() => {
    lifetime.current = {}
    return () => {
      lifetime.current = null
    }
  }, [itemId])
  return useCallback(() => {
    const owner = lifetime.current
    const subject = brewSubject.getSnapshot()
    return () =>
      owner !== null &&
      lifetime.current === owner &&
      subject.active &&
      !subject.signal.aborted
  }, [])
}
