/** Keyset list continuation. Page offset is only for the first request. */

export function isItemListCursor(raw: string): boolean {
  const sep = raw.indexOf(':')
  if (sep <= 0 || sep === raw.length - 1) return false
  const ms = raw.slice(0, sep)
  const id = raw.slice(sep + 1)
  if (!/^-?\d+$/.test(ms) || !/^[1-9]\d*$/.test(id)) return false
  return Number.isSafeInteger(Number(ms))
}

export function itemListHasMore(
  nextCursor: string | null | undefined,
  itemCount: number,
  perPage: number,
): boolean {
  if (nextCursor !== undefined) return Boolean(nextCursor)
  return perPage > 0 && itemCount >= perPage
}

export function itemListRequest(input: {
  cursor?: string | null
  page?: number
  perPage: number
}): { cursor?: string; page?: number; per_page: number } {
  if (input.cursor && isItemListCursor(input.cursor)) {
    return { cursor: input.cursor, per_page: input.perPage }
  }
  return {
    page: input.page,
    per_page: input.perPage,
  }
}
