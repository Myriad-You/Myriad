import type { ArmVariantSource } from '@myriad/figure/rig'
import type { ArmVariantKey, SiteArmVariants } from '../armVariantsApi'
import { useCallback, useEffect, useRef, useState } from 'react'
import { ARM_VARIANT_KEYS, armVariantLost, listSiteArmVariants, startSiteArmVariants } from '../armVariantsApi'
import { drawingFailures, untilNoneDrawn } from './serverDrawing'
import { useServerGeneration } from './useServerGeneration'

const keyOf = (variant: ArmVariantSource): ArmVariantKey => `${variant.gesture}-${variant.side}`

/** Gesture arms: the current portrait (a full-body set's picture with `outfitId`) redrawn with one arm raised, kept per portrait. */
export function useArmVariants(portraitUrl: string | null, outfitId?: string) {
  const [variants, setVariants] = useState<ArmVariantSource[]>([])
  // Preflight reads the latest ones, even right after drawing them.
  const variantsRef = useRef<ArmVariantSource[]>([])
  variantsRef.current = variants
  // Being drawn on the server: asked for by this page, begun before a
  // reload, or in another tab.
  const [drawing, setDrawing] = useState<ArmVariantKey[]>([])
  const drawingRef = useRef<ArmVariantKey[]>([])
  drawingRef.current = drawing
  // Why the ones the server could not draw were not kept, as the list read last has it.
  const failedRef = useRef<SiteArmVariants['failed']>({})
  const listedRef = useRef<Promise<void>>(Promise.resolve())
  const waitersRef = useRef<Array<() => void>>([])
  // The picture the lists above are of: gestures still being drawn for another one are not taken into them.
  const pictureRef = useRef('')
  pictureRef.current = `${outfitId ?? ''}|${portraitUrl ?? ''}`

  const take = useCallback((listed: SiteArmVariants) => {
    variantsRef.current = listed.variants
    drawingRef.current = listed.drawing
    failedRef.current = listed.failed
    setVariants(listed.variants)
    setDrawing(listed.drawing)
    // None being drawn: the ones waiting for that go on, whether or not the page ever showed some as drawn.
    if (listed.drawing.length === 0) {
      for (const resolve of waitersRef.current.splice(0)) resolve()
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    // Another picture's gestures are not this one's, not even until its own are listed.
    variantsRef.current = []
    drawingRef.current = []
    failedRef.current = {}
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

  const reread = useCallback(async () => {
    const picture = pictureRef.current
    const listed = await listSiteArmVariants(outfitId)
    // Read for a picture the panel no longer shows: not this one's list.
    if (pictureRef.current === picture) take(listed)
  }, [outfitId, take])
  // The picture shown now is the one read again, also by a drawing asked for before it changed.
  const rereadRef = useRef(reread)
  rereadRef.current = reread
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
   * that fails its check is left out; the rest are kept. The server draws
   * them on its own once asked; this resolves when it draws none any more.
   */
  const generate = useCallback(async (again = false) => {
    // What this picture has kept is known first, or ones it has would be drawn (and paid for) again.
    const picture = pictureRef.current
    const gone = () => pictureRef.current !== picture
    await listedRef.current
    // Asked for a picture the panel no longer shows: nothing is drawn for it.
    if (gone()) return
    const before = new Map(variantsRef.current.map((variant) => [keyOf(variant), variant.url]))
    // Ones the server is drawing already are waited for, not asked again.
    const already = drawingRef.current
    const asked = ARM_VARIANT_KEYS.filter((key) => !already.includes(key) && (again || !before.has(key)))
    // Each ask is answered as soon as the server has taken it up: none waits for a connection another holds.
    // The right arm's are taken up before the left arm's are asked.
    const begun = await startSiteArmVariants(asked, outfitId)
    // The panel shows another picture by now: these stay on the server for the one they were drawn for.
    if (gone()) return
    const taken = asked.filter((_, index) => begun[index].status === 'fulfilled')
    if (taken.length > 0) {
      // Shown as being drawn at once; the list is read every few seconds while any is.
      const marked = [...new Set([...drawingRef.current, ...taken])]
      drawingRef.current = marked
      setDrawing(marked)
    }
    if (taken.length > 0 || already.length > 0) {
      await untilNoneDrawn({ settled, reread: () => rereadRef.current(), drawing: () => drawingRef.current, gone })
    }
    if (gone()) return
    const now = new Map(variantsRef.current.map((variant) => [keyOf(variant), variant.url]))
    const failures = drawingFailures(
      asked,
      begun,
      { before: (key) => before.get(key), after: (key) => now.get(key) },
      failedRef.current,
      armVariantLost,
    )
    // Some drawn is enough to go on with; none at all is a failure.
    if (failures.length > 0 && variantsRef.current.length === 0) throw failures[0]
  }, [outfitId, settled])

  /** What an import should cut into gesture arms right now. */
  const references = useCallback(() => variantsRef.current, [])

  return {
    ready: variants.length,
    /** Some are being drawn on the server, asked for here or begun elsewhere. */
    drawing: busy,
    generate,
    settled,
    references,
  }
}
