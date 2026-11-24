import type { Anime25DPartProfile } from '@myriad/figure/workbench'
import type { StagedPartSearch } from './movingPartsSearch'
import { useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { stagedPartSearch } from './movingPartsSearch'
import { PartProfileSummary } from './PartProfileSummary'

/** How often a search still looking is asked after. */
const WATCH_MS = 5000

/** Where the search a preview began stands, asked again while it looks; null before the first answer or without one. */
function useStagedPartSearch(search: string | null): StagedPartSearch | null {
  const [standing, setStanding] = useState<StagedPartSearch | null>(null)
  useEffect(() => {
    setStanding(null)
    if (!search) return undefined
    let current = true
    let timer: ReturnType<typeof setTimeout> | undefined
    const ask = () => {
      stagedPartSearch(search).then((next) => {
        if (!current) return
        setStanding(next)
        if (next.state === 'looking') timer = setTimeout(ask, WATCH_MS)
      }, () => {
        // Not reachable for a moment: asked again. Activation does not wait on this.
        if (current) timer = setTimeout(ask, WATCH_MS)
      })
    }
    ask()
    return () => {
      current = false
      clearTimeout(timer)
    }
  }, [search])
  return standing
}

/**
 * The moving parts of a preflighted rig: the server looks for them in the
 * background from the preview on (`search`), so this shows that it is still
 * looking, then what it found. Activation never waits on it; `carried` is a
 * profile the preflight brought itself.
 */
export function StagedPartsSummary({ search, carried }: { search: string | null, carried: Anime25DPartProfile | undefined }) {
  const { t } = useI18n()
  const standing = useStagedPartSearch(search)
  if (standing?.state === 'looking') {
    return <p className="merope-motion-rig__hint" role="status">{t.merope.rigPartsLooking}</p>
  }
  return <PartProfileSummary profile={standing?.state === 'found' ? standing.partProfile : carried} />
}
