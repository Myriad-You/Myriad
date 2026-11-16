import type { ArmVariantSource } from '@myriad/figure/rig'
import type { ArmVariantKey } from '../armVariantsApi'
import { useCallback, useEffect, useRef, useState } from 'react'
import { ARM_VARIANT_KEYS, generateSiteArmVariant, listSiteArmVariants } from '../armVariantsApi'
import { useServerGeneration } from './useServerGeneration'

const keyOf = (variant: ArmVariantSource): ArmVariantKey => `${variant.gesture}-${variant.side}`

/** Gesture arms: the current portrait (a full-body set's picture with `outfitId`) redrawn with one arm raised, kept per portrait. */
export function useArmVariants(portraitUrl: string | null, outfitId?: string) {
  const [variants, setVariants] = useState<ArmVariantSource[]>([])
  // Preflight reads the latest ones, even right after drawing them.
  const variantsRef = useRef<ArmVariantSource[]>([])
  variantsRef.current = variants
  // Being drawn on the server without this page asking: begun before a
  // reload, or in another tab.
  const [drawing, setDrawing] = useState<ArmVariantKey[]>([])
  const drawingRef = useRef<ArmVariantKey[]>([])
  drawingRef.current = drawing
  const listedRef = useRef<Promise<void>>(Promise.resolve())
  const waitersRef = useRef<Array<() => void>>([])
  // The picture the lists above are of: gestures still being drawn for another one are not taken into them.
  const pictureRef = useRef('')
  pictureRef.current = `${outfitId ?? ''}|${portraitUrl ?? ''}`

  const take = useCallback((listed: Awaited<ReturnType<typeof listSiteArmVariants>>) => {
    variantsRef.current = listed.variants
    drawingRef.current = listed.drawing
    setVariants(listed.variants)
    setDrawing(listed.drawing)
  }, [])

  useEffect(() => {
    let cancelled = false
    // Another picture's gestures are not this one's, not even until its own are listed.
    variantsRef.current = []
    drawingRef.current = []
    setVariants([])
    setDrawing([])
    if (!portraitUrl) return
    listedRef.current = listSiteArmVariants(outfitId)
      .then((listed) => {
        if (!cancelled) take(listed)
      })
      .catch(() => {
        if (!cancelled) setVariants([])
      })
    return () => {
      cancelled = true
    }
  }, [outfitId, portraitUrl, take])

  const reread = useCallback(async () => take(await listSiteArmVariants(outfitId)), [outfitId, take])
  // Once nothing is drawn any more, the list read last has the new ones.
  const busy = useServerGeneration(drawing.length > 0, reread, () => {
    for (const resolve of waitersRef.current.splice(0)) resolve()
  })

  /** Resolves once the server draws none of them any more. */
  const settled = useCallback(async () => {
    await listedRef.current
    if (drawingRef.current.length === 0) return
    await new Promise<void>((resolve) => waitersRef.current.push(resolve))
  }, [])

  /**
   * Draws every gesture this portrait lacks (all of them with `again`). One
   * that fails its check is left out; the rest are kept.
   */
  const generate = useCallback(async (again = false) => {
    // What this picture has kept is known first, or ones it has would be drawn (and paid for) again.
    const picture = pictureRef.current
    await listedRef.current
    // Asked for a picture the panel no longer shows: nothing is drawn for it.
    if (pictureRef.current !== picture) return
    const kept = new Set(variantsRef.current.map(keyOf))
    // Ones the server is drawing already are waited for, not asked again.
    const already = drawingRef.current
    const asked = ARM_VARIANT_KEYS.filter((key) => !already.includes(key) && (again || !kept.has(key)))
    const results = await Promise.allSettled(asked.map((key) => generateSiteArmVariant(key, outfitId)))
    if (already.length > 0) await settled()
    // The panel shows another picture by now: these stay on the server for the one they were drawn for.
    if (pictureRef.current !== picture) return
    const drawn = results.flatMap((result) => (result.status === 'fulfilled' ? [result.value] : []))
    const replaced = new Set(drawn.map(keyOf))
    const next = [...variantsRef.current.filter((variant) => !replaced.has(keyOf(variant))), ...drawn]
    variantsRef.current = next
    setVariants(next)
    // Some drawn is enough to go on with; none at all is a failure.
    const failure = results.find((result) => result.status === 'rejected')
    if (failure && drawn.length === 0 && next.length === 0) throw failure.reason
  }, [outfitId, settled])

  /** What an import should cut into gesture arms right now. */
  const references = useCallback(() => variantsRef.current, [])

  return {
    ready: variants.length,
    /** Some are being drawn on the server, begun elsewhere. */
    drawing: busy,
    generate,
    settled,
    references,
  }
}
