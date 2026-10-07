import type { NavigateFunction } from 'react-router-dom'
import type { PhantasiSource } from '../../types/phantasi'

import { useCallback, useEffect, useMemo, useRef } from 'react'
import * as phantasiApi from '../../services/phantasiApi'
import { phantasiSubject } from '../../utils/phantasiSubject'
import { JOURNAL_ROOT, journalItemPath } from './logic/journalRoutes'
import { ownItemState } from './logic/ownState'
import {
  phantasiItemNavigateMode,
  phantasiItemParamId,
  phantasiOpenedItemId,
  phantasiOpenedItemState,
  phantasiOpenedWebItem,
  restoreAfterFailedOpen,
  shouldPopOpenedItem,
} from './logic/phantasiItemRoute'
import { useArticleOpen } from './useArticleOpen'

export function usePhantasiItemRoute(
  itemIdParam: string | undefined,
  sources: PhantasiSource[],
  sourcesLoaded: boolean,
  navigate: NavigateFunction,
  setError: (message: string) => void,
  loadFailed: string,
  listPath = JOURNAL_ROOT,
  location?: { pathname: string; state?: unknown },
) {
  const session = useArticleOpen(setError, loadFailed)
  const { selectedItem } = session

  const selectedItemSource = useMemo(() => {
    if (!selectedItem) return null
    return (
      sources.find((source) => source.id === selectedItem.source_id) ?? null
    )
  }, [selectedItem, sources])

  const selectedItemOwnState = useMemo(
    () => ownItemState(selectedItem, selectedItemSource, sourcesLoaded),
    [selectedItem, selectedItemSource, sourcesLoaded],
  )

  const openedId = phantasiOpenedItemId(location?.state)
  const pathname =
    location?.pathname ??
    (itemIdParam ? journalItemPath(Number(itemIdParam)) : listPath)
  const routeKey = JSON.stringify([pathname, itemIdParam, openedId])
  const previousRoute = useRef(routeKey)
  const expectedRoute = useRef<string | null>(null)
  const initial = useRef(true)
  useEffect(() => {
    const changed = initial.current || previousRoute.current !== routeKey
    initial.current = false
    previousRoute.current = routeKey
    if (changed) {
      if (expectedRoute.current === routeKey && selectedItem) {
        expectedRoute.current = null
        return
      }
      expectedRoute.current = null
      if (itemIdParam || openedId != null) {
        const id = itemIdParam ? phantasiItemParamId(itemIdParam) : openedId!
        const kept =
          selectedItem && selectedItem.id !== id
            ? {
                id: selectedItem.id,
                own: selectedItemOwnState === 'own',
              }
            : null
        if (id == null) {
          setError(loadFailed)
          const restore = restoreAfterFailedOpen(
            itemIdParam,
            itemIdParam,
            undefined,
            kept,
            listPath,
          )
          if (restore) {
            expectedRoute.current = JSON.stringify([
              restore.path,
              restore.param,
              undefined,
            ])
            navigate(restore.path, { replace: true })
          }
        } else if (selectedItem?.id !== id) {
          const requested = routeKey
          let requestSignal: AbortSignal | undefined
          void session
            .openArticle(
              (signal) => {
                requestSignal = signal
                const webItem = itemIdParam
                  ? undefined
                  : phantasiOpenedWebItem(
                      location?.state,
                      phantasiSubject.getSnapshot(),
                    )
                return webItem !== undefined
                  ? Promise.resolve(webItem)
                  : phantasiApi.getItem(id, undefined, { signal })
              },
              { queue: null },
            )
            .then((item) => {
              if (
                requestSignal?.aborted ||
                previousRoute.current !== requested ||
                item?.id === id
              ) {
                return
}
              const restore = itemIdParam
                ? restoreAfterFailedOpen(
                    itemIdParam,
                    itemIdParam,
                    item?.id,
                    kept,
                    listPath,
                  )
                : { path: listPath, param: undefined }
              if (!restore) return
              expectedRoute.current = JSON.stringify([
                restore.path,
                restore.param,
                undefined,
              ])
              navigate(restore.path, { replace: true })
            })
        }
      } else {
        session.closeArticle()
      }
      return
    }
    if (!selectedItem || selectedItemOwnState === 'unknown' || session.opening)
      return
    const target =
      selectedItemOwnState === 'own' ? String(selectedItem.id) : undefined
    let mode = phantasiItemNavigateMode(itemIdParam, target)
    if (!target && !itemIdParam && openedId !== selectedItem.id) {
      mode = openedId == null ? 'push' : 'replace'
    }
    if (mode !== 'none') {
      const path = target ? journalItemPath(selectedItem.id) : listPath
      expectedRoute.current = JSON.stringify([path, target, selectedItem.id])
      if (target) {
        navigate(journalItemPath(selectedItem.id), {
          replace: mode === 'replace',
          state: phantasiOpenedItemState(selectedItem.id),
        })
      } else {
        navigate(listPath, {
          replace: mode === 'replace',
          state: phantasiOpenedItemState(
            selectedItem.id,
            openedId != null
              ? shouldPopOpenedItem(location?.state, openedId)
              : mode === 'push',
            selectedItem,
            phantasiSubject.getSnapshot(),
          ),
        })
      }
    }
  }, [
    itemIdParam,
    openedId,
    routeKey,
    location?.state,
    selectedItem,
    selectedItemOwnState,
    session.opening,
    session.openArticle,
    session.closeArticle,
    navigate,
    setError,
    loadFailed,
    listPath,
  ])

  const closeReader = useCallback(() => {
    session.closeArticle()
    if (shouldPopOpenedItem(location?.state, selectedItem?.id)) navigate(-1)
    else if (itemIdParam || openedId != null)
      navigate(listPath, { replace: true })
  }, [
    session.closeArticle,
    location?.state,
    selectedItem?.id,
    navigate,
    itemIdParam,
    openedId,
    listPath,
  ])

  return {
    ...session,
    closeReader,
    selectedItemSource,
    selectedItemOwnState,
    selectedItemIsOwn: selectedItemOwnState === 'own',
  }
}
