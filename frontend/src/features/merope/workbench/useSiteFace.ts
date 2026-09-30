import type { MeropeRigManifest } from '../rig/types'
import { useCallback, useEffect, useRef, useState } from 'react'
import { userFacingError } from '../../../utils/userFacingError'
import { getSiteFace } from '../api'
import { reportMeropeError } from './workbenchShared'

/** The site face the workbench edits: master portrait plus the worn rig. */
export function useSiteFace(loadFailed: string) {
  const [rigManifest, setRigManifest] = useState<MeropeRigManifest | null>(null)
  const [rigAssetId, setRigAssetId] = useState<string | null>(null)
  const manifestRef = useRef(rigManifest)
  manifestRef.current = rigManifest
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [generationFingerprint, setGenerationFingerprint] = useState<
    string | null
  >(null)

  const loadFace = useCallback(async () => {
    const face = await getSiteFace()
    setRigManifest(face.manifest)
    setRigAssetId(face.assetId)
    setPortraitUrl(face.portraitUrl)
    setGenerationFingerprint(face.generationFingerprint)
    return face
  }, [])

  useEffect(() => {
    let cancelled = false
    void loadFace().catch((reason) => {
      if (!cancelled) {
        setRigManifest(null)
        setPortraitUrl(null)
        setGenerationFingerprint(null)
        reportMeropeError(userFacingError(reason, loadFailed))
      }
    })
    return () => {
      cancelled = true
    }
  }, [loadFace, loadFailed])

  /** The portrait was removed; drop the face until the next load. */
  const clearPortrait = useCallback(() => {
    setPortraitUrl(null)
    setGenerationFingerprint(null)
    setRigManifest(null)
  }, [])

  return {
    rigManifest,
    setRigManifest,
    rigAssetId,
    setRigAssetId,
    manifestRef,
    portraitUrl,
    generationFingerprint,
    loadFace,
    clearPortrait,
  }
}

export type SiteFace = ReturnType<typeof useSiteFace>
