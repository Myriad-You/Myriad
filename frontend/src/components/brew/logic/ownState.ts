/** 源未解析前禁止据此清掉 /brew/item/*。 */

import { isOwnBrewSource } from '../constants'

export type OwnItemState = 'unknown' | 'own' | 'not-own'

export function ownItemState(
  item: { fromWebSearch?: boolean; source_id: number } | null,
  source: { category?: string | null; admin_only?: boolean } | null,
  sourcesLoaded: boolean,
): OwnItemState {
  if (!item) return 'unknown'
  if (item.fromWebSearch || item.source_id <= 0) return 'not-own'
  if (source) return isOwnBrewSource(source) ? 'own' : 'not-own'
  if (!sourcesLoaded) return 'unknown'
  return 'not-own'
}
