import type { SiteFace } from '../api'
import type { RigAssetPreflight } from '../assets/pipeline'
import type { MeropeRigManifest } from '../rig/types'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { generationFailureMessage } from '../../../components/agent/onboarding/generationError'
import { SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { siteMediaUrl } from '../../../utils/siteMediaUrl'
import { userFacingError } from '../../../utils/userFacingError'
import {
  decomposeFullBodyWithSeeThrough,
  generateFullBodyPortrait,
  getFullBodyFace,
  uploadFullBodyPortrait,
} from '../api'
import { commitFullBodyPsdAsset, preflightFullBodyPsdAsset } from '../assets/pipeline'
import RigCharacter from '../character/RigCharacter'

interface Props {
  /** Changes with the worn outfit and its bust portrait, which the figure is drawn from. */
  outfitKey: string
  seeThroughTokenConfigured: boolean
  /** The outfit's full figure was redrawn, replaced or saved. */
  onChanged?: () => void
}

type Operation = 'generate' | 'upload' | 'decompose' | 'save'

/**
 * The worn outfit's optional full figure: drawn from its bust portrait, split
 * by See-through, previewed, then saved beside the bust. The panel keeps
 * playing the bust.
 */
export function FullBodyPanel({
  outfitKey,
  seeThroughTokenConfigured,
  onChanged,
}: Props) {
  const { t } = useI18n()
  const labels = t.merope
  const copy = labels.fullBody
  const [face, setFace] = useState<SiteFace | null>(null)
  const [operation, setOperation] = useState<Operation | null>(null)
  const [preflight, setPreflight] = useState<RigAssetPreflight | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  const uploadRef = useRef<HTMLInputElement>(null)

  const reload = useCallback(async () => {
    setFace(await getFullBodyFace())
  }, [])

  useEffect(() => {
    setPreflight(null)
    setSaved(false)
    setError(null)
    reload().catch(() => setFace(null))
  }, [outfitKey, reload])

  // A preflight has no stored atlas yet; play it from the packed one.
  const previewAtlas = useMemo(
    () => (preflight ? URL.createObjectURL(preflight.prepared.atlas) : null),
    [preflight],
  )
  useEffect(
    () => () => {
      if (previewAtlas) URL.revokeObjectURL(previewAtlas)
    },
    [previewAtlas],
  )
  const manifest: MeropeRigManifest | null =
    preflight && previewAtlas
      ? {
          ...preflight.manifest,
          textures: preflight.manifest.textures.map((texture, index) =>
            index === 0 ? { ...texture, url: previewAtlas } : texture,
          ),
        }
      : (face?.manifest ?? null)

  const run = async (
    next: Operation,
    task: () => Promise<void>,
    fallback: string,
  ) => {
    if (operation) return
    setOperation(next)
    setError(null)
    setSaved(false)
    try {
      await task()
    } catch (reason) {
      setError(
        userFacingError(
          generationFailureMessage(
            reason,
            fallback,
            labels.motionSeeThroughTimeout,
            {
              see_through_token_required: labels.motionSeeThroughTokenRequired,
              see_through_busy: labels.motionSeeThroughBusy,
              see_through_auth_failed: labels.motionSeeThroughAuthFailed,
              see_through_quota_unavailable: labels.motionSeeThroughQuota,
              see_through_timeout: labels.motionSeeThroughTimeout,
            },
          ),
          fallback,
        ),
      )
    } finally {
      setOperation(null)
    }
  }

  const generate = () =>
    run(
      'generate',
      async () => {
        await generateFullBodyPortrait()
        setPreflight(null)
        await reload()
        onChanged?.()
      },
      copy.failed,
    )

  const upload = (file: File) =>
    run(
      'upload',
      async () => {
        await uploadFullBodyPortrait(file)
        setPreflight(null)
        await reload()
        onChanged?.()
      },
      labels.portraitUploadFailed,
    )

  const decompose = () =>
    run(
      'decompose',
      async () => {
        const portrait = face?.portraitUrl
        if (!portrait) return
        const fingerprint = face.generationFingerprint ?? undefined
        const file = await decomposeFullBodyWithSeeThrough({
          sourceMasterAssetId: portrait,
          sourceGenerationFingerprint: fingerprint,
        })
        setPreflight(await preflightFullBodyPsdAsset(file, portrait, fingerprint))
      },
      labels.motionSeeThroughUpstream,
    )

  const save = () =>
    run(
      'save',
      async () => {
        if (!preflight) return
        await commitFullBodyPsdAsset(preflight)
        setPreflight(null)
        setSaved(true)
        await reload()
        onChanged?.()
      },
      labels.motionSeeThroughUpstream,
    )

  const portrait = face?.portraitUrl ?? null
  return (
    <div className="merope-motion-rig merope-full-body">
      <div className="merope-full-body__figures">
        {portrait ? (
          <figure className="merope-full-body__figure">
            <div className="merope-full-body__frame">
              <img src={siteMediaUrl(portrait)} alt={copy.portrait} />
            </div>
            <figcaption>{copy.portrait}</figcaption>
          </figure>
        ) : (
          <p className="merope-motion-rig__hint">{copy.none}</p>
        )}
        {manifest ? (
          <figure className="merope-full-body__figure">
            <div className="merope-full-body__frame">
              <RigCharacter
                activity="idle"
                mood={0}
                manifest={manifest}
                fallbackUrl={portrait}
              />
            </div>
            <figcaption>{copy.preview}</figcaption>
          </figure>
        ) : null}
      </div>
      <div className="merope-full-body__actions">
        <SettingsButton
          type="button"
          size="sm"
          disabled={operation !== null}
          loading={operation === 'generate'}
          confirm={portrait ? copy.generateConfirm : undefined}
          onClick={() => void generate()}
        >
          {operation === 'generate'
            ? copy.generating
            : portrait
              ? copy.regenerate
              : copy.generate}
        </SettingsButton>
        <SettingsButton
          type="button"
          size="sm"
          disabled={operation !== null}
          loading={operation === 'upload'}
          confirm={portrait ? copy.generateConfirm : undefined}
          onClick={() => uploadRef.current?.click()}
        >
          {operation === 'upload' ? copy.uploading : copy.upload}
        </SettingsButton>
        {portrait ? (
          <SettingsButton
            type="button"
            size="sm"
            disabled={operation !== null || !seeThroughTokenConfigured}
            loading={operation === 'decompose'}
            onClick={() => void decompose()}
          >
            {operation === 'decompose' ? copy.decomposing : copy.decompose}
          </SettingsButton>
        ) : null}
        {preflight ? (
          <SettingsButton
            type="button"
            size="sm"
            disabled={operation !== null}
            loading={operation === 'save'}
            onClick={() => void save()}
          >
            {operation === 'save' ? copy.saving : copy.save}
          </SettingsButton>
        ) : null}
      </div>
      <input
        ref={uploadRef}
        type="file"
        accept="image/png,image/jpeg,image/webp"
        hidden
        onChange={(event) => {
          const file = event.currentTarget.files?.[0]
          event.currentTarget.value = ''
          if (file) void upload(file)
        }}
      />
      {portrait && !seeThroughTokenConfigured ? (
        <p className="merope-motion-rig__hint">{copy.needsToken}</p>
      ) : null}
      {saved ? <p className="merope-motion-rig__hint">{copy.saved}</p> : null}
      {error ? (
        <p className="merope-motion-rig__hint" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  )
}
