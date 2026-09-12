import type { BrewItem, BrewSource } from '../../types/brew'
import type { OwnItemState } from './logic/ownState'

import { useMemo } from 'react'
import { usePageSeo } from '../../hooks/usePageSeo'
import {
  buildBrewItemPageSeo,
  buildBrewListPageSeo,
} from '../../utils/brewPageSeo'
import { brewOwnItemPath } from './constants'

export function useBrewSeo(
  selectedItem: BrewItem | null,
  source: BrewSource | null,
  ownState: OwnItemState,
  moduleOpenToAll: boolean,
  listLabel: string,
  listDescription: string | undefined,
) {
  usePageSeo(
    useMemo(() => {
      if (selectedItem && ownState !== 'unknown') {
        return buildBrewItemPageSeo({
          item: selectedItem,
          source,
          moduleOpenToAll,
        })
      }
      if (selectedItem && ownState === 'unknown') {
        return {
          title: undefined,
          path: brewOwnItemPath(selectedItem.id),
          noindex: true,
        }
      }
      return buildBrewListPageSeo({
        listLabel,
        listDescription,
        moduleOpenToAll,
      })
    }, [
      selectedItem,
      source,
      ownState,
      moduleOpenToAll,
      listLabel,
      listDescription,
    ]),
  )
}
