import { useEffect, useState } from 'react'

const ADD_HIGHLIGHT_MS = 1400

export function useAddedSlug(slugs: string[]): string | null {
  const [seenSlugs, setSeenSlugs] = useState(slugs)
  const [addedSlug, setAddedSlug] = useState<string | null>(null)

  let highlight = addedSlug
  if (
    slugs.length !== seenSlugs.length ||
    slugs.some((slug, index) => slug !== seenSlugs[index])
  ) {
    const hydrating = seenSlugs.length === 0 && slugs.length > 1
    const fresh = hydrating
      ? null
      : (slugs.find((slug) => !seenSlugs.includes(slug)) ?? null)
    setSeenSlugs(slugs)
    if (fresh) {
      highlight = fresh
      setAddedSlug(fresh)
    }
  }

  useEffect(() => {
    if (!addedSlug) return undefined
    const timer = window.setTimeout(setAddedSlug, ADD_HIGHLIGHT_MS, null)
    return () => window.clearTimeout(timer)
  }, [addedSlug])

  return highlight
}

export function useAddedCardOpen(justAdded: boolean, startOpen: boolean) {
  const [open, setOpen] = useState(justAdded ? false : startOpen)
  useEffect(() => {
    if (!justAdded) return undefined
    setOpen(true)
    return undefined
  }, [justAdded])
  return [open, setOpen] as const
}
