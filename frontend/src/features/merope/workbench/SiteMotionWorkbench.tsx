import type { UpperBodyVisualIdentityKey } from '../../../components/agent/onboarding/onboardingTypes'
import type { PoseCorrection } from '../anime25drig/poseCorrections'
import type { RigCharacterHandle } from '../character/RigCharacter'
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { createPortal } from 'react-dom'
import { activityKey, moodBand } from '../../../components/agent/meropeVitals'
import { genderFromProfile } from '../../../components/agent/onboarding/onboardingTypes'
import PersonaIdentityView from '../../../components/agent/onboarding/ui/PersonaIdentityView'
import PersonaImportPanel from '../../../components/agent/onboarding/ui/PersonaImportPanel'
import VisualIdentityView from '../../../components/agent/onboarding/ui/VisualIdentityView'
import {
  getTourSnapshot,
  subscribeTour,
} from '../../../components/tour/tourEngine'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { siteMediaUrl } from '../../../utils/siteMediaUrl'
import { isAnime25DPlayback } from '../anime25drig/types'
import {
  decomposeSitePortraitWithSeeThrough,
  saveRigPoseCorrections,
} from '../api'
import { commitRigPsdAsset, preflightRigPsdAsset } from '../assets/pipeline'
import RigCharacter from '../character/RigCharacter'
import { notifyFaceUpdated } from '../events'
import { useRigPreviewMotionLifecycle } from '../motion/useRigMotionLifecycle'
import OutfitWardrobe from '../OutfitWardrobe'
import { applyOutfit, sortWardrobe } from '../wardrobe'
import Anime25DWorkbench from './Anime25DWorkbench'
import { OutfitDetail } from './OutfitDetail'
import { useAddressee } from './useAddressee'
import { useAiExpressions } from './useAiExpressions'
import { usePersonaWardrobe } from './usePersonaWardrobe'
import { useSeeThroughToken } from './useSeeThroughToken'
import { useSiteFace } from './useSiteFace'
import { useStickerAvatar } from './useStickerAvatar'
import { wardrobeOverview } from './wardrobeOverview'
import { WorkbenchOverview } from './WorkbenchOverview'
import { downloadPicture, reportMeropeError, toMeropeActivity } from './workbenchShared'
import '../merope.css'
import '../merope-motion-home.css'

interface Props {
  mood: number
  arousal?: number
  activity: string
}

export default function SiteMotionWorkbench({
  mood,
  arousal,
  activity,
}: Props) {
  const { t, locale, format } = useI18n()
  const o = t.agentPersona.onboarding
  const personaTouring = useSyncExternalStore(
    subscribeTour,
    () =>
      getTourSnapshot().active &&
      getTourSnapshot().tourId === 'config-persona-owner',
    () => false,
  )
  const face = useSiteFace(t.merope.loadFailed)
  const {
    rigManifest,
    setRigManifest,
    rigAssetId,
    setRigAssetId,
    manifestRef,
    portraitUrl,
    generationFingerprint,
    loadFace,
  } = face
  const sticker = useStickerAvatar(portraitUrl, t.merope.avatarFailed)
  const persona = usePersonaWardrobe({ face, onPortraitReplaced: sticker.voided })
  const {
    personaSnapshot,
    structuredPersona,
    visualIdentity,
    wardrobeItems,
    activeOutfitId,
    managingOutfitId,
    setManagingOutfitId,
    generating,
  } = persona
  const addressee = useAddressee(
    persona.setPersonaSnapshot,
    t.errors.addresseeSaveFailed,
  )
  const seeThrough = useSeeThroughToken(t.merope.seeThroughStatusFailed)
  const aiExpressions = useAiExpressions(portraitUrl)
  const managingId = personaTouring ? null : managingOutfitId
  const [studioHost, setStudioHost] = useState<HTMLDivElement | null>(null)
  const rigCharacterRef = useRef<RigCharacterHandle>(null)
  useRigPreviewMotionLifecycle(rigCharacterRef, {
    mood,
    arousal,
    activity: toMeropeActivity(activity),
  })
  const visualLabels: Record<UpperBodyVisualIdentityKey, string> = {
    faceDesign: o.visualFaceDesign,
    eyeDesign: o.visualEyeDesign,
    hairShape: o.visualHairShape,
    hairLayerPlan: o.visualHairLayers,
    upperBodySilhouette: o.visualUpperBodySilhouette,
    outfitConstruction: o.visualOutfitConstruction,
    sleeveArmDesign: o.visualSleeveArmDesign,
    materialPlan: o.visualMaterialPlan,
    heroAccessory: o.visualHeroAccessory,
    paletteHint: o.visualPalette,
    motif: o.visualMotif,
  }

  // The persona as first loaded seeds every panel that edits it.
  const { adopt, forget } = persona
  const { hydrate: hydrateAddressee } = addressee
  const { setUrl: setStickerUrl } = sticker
  useEffect(() => {
    let cancelled = false
    void agentService
      .getPersona()
      .then((loaded) => {
        if (cancelled) return
        adopt(loaded)
        hydrateAddressee(loaded)
        setStickerUrl(loaded?.avatarAssetId ?? null)
      })
      .catch(() => {
        if (cancelled) return
        forget()
        hydrateAddressee(null)
        setStickerUrl(null)
      })
    return () => {
      cancelled = true
    }
  }, [adopt, forget, hydrateAddressee, setStickerUrl])

  const { references: expressionReferences } = aiExpressions
  const preflightRigPsd = useCallback(
    async (
      file: File,
      onStage: NonNullable<Parameters<typeof preflightRigPsdAsset>[2]>,
      signal?: AbortSignal,
    ) => {
      if (!portraitUrl) throw new Error(t.merope.assetNeedsPortrait)
      return preflightRigPsdAsset(
        file,
        portraitUrl,
        onStage,
        generationFingerprint || undefined,
        signal,
        expressionReferences(),
      )
    },
    [expressionReferences, generationFingerprint, portraitUrl, t.merope.assetNeedsPortrait],
  )

  const decomposeRigPsd = useCallback(async () => {
    if (!portraitUrl) throw new Error(t.merope.assetNeedsPortrait)
    return decomposeSitePortraitWithSeeThrough({
      sourceMasterAssetId: portraitUrl,
      sourceGenerationFingerprint: generationFingerprint || undefined,
      resolution: 1280,
      seed: 42,
      splitArmsAndLegs: true,
    })
  }, [generationFingerprint, portraitUrl, t.merope.assetNeedsPortrait])

  const { reload } = persona
  const commitRigPsd = useCallback(
    async (
      preflight: Parameters<typeof commitRigPsdAsset>[0],
      onStage: NonNullable<Parameters<typeof commitRigPsdAsset>[1]>,
    ) => {
      const imported = await commitRigPsdAsset(preflight, onStage)
      setRigManifest(imported.manifest)
      await reload()
      await loadFace()
      notifyFaceUpdated()
      return {
        partCount: imported.partCount,
        score: imported.report.score,
      }
    },
    [loadFace, reload, setRigManifest],
  )

  const { setWardrobeItems } = persona
  const savePoseCorrections = useCallback(async (corrections: PoseCorrection[]) => {
    if (!rigAssetId || !rigManifest) throw new Error(t.merope.poseCorrection.failed)
    const saved = await saveRigPoseCorrections(rigAssetId, corrections)
    notifyFaceUpdated()
    // A late save must not replace a newer portrait/outfit displayed meanwhile.
    if (manifestRef.current !== rigManifest) return
    setRigManifest(saved.manifest)
    setRigAssetId(saved.assetId)
    setWardrobeItems(items => items.map(item => item.id === activeOutfitId ? { ...item, rigAssetId: saved.assetId } : item))
  }, [rigAssetId, rigManifest, activeOutfitId, manifestRef, setRigAssetId, setRigManifest, setWardrobeItems, t.merope.poseCorrection.failed])

  /** The owner uploaded a new master portrait for the worn outfit. */
  const adoptUploadedPortrait = async (url: string) => {
    reportMeropeError('')
    await loadFace()
    await reload()
    sticker.voided()
    try {
      await persona.applyVisualFromPortrait(url)
    } catch {
      await loadFace()
    }
    notifyFaceUpdated()
  }

  const motionEnabled = Boolean(
    rigManifest?.anime25dPlayback &&
    isAnime25DPlayback(rigManifest.anime25dPlayback) &&
    rigManifest.textures[0]?.url,
  )

  const portraitStage = (
    <div className="merope-motion-asset__preview">
      {portraitUrl ? (
        <img
          className={`merope-motion-asset__still${motionEnabled ? ' is-behind' : ''}`}
          src={siteMediaUrl(portraitUrl)}
          alt={t.merope.visualTitle}
        />
      ) : (
        <p className="merope-motion-asset__empty">{t.merope.assetEmpty}</p>
      )}
      <div ref={setStudioHost} className="merope-motion-asset__live" />
    </div>
  )

  const visualIdentityView = (
    show: 'character' | 'outfit',
  ) =>
    visualIdentity ? (
      <VisualIdentityView
        identity={visualIdentity}
        labels={visualLabels}
        characterTitle={t.merope.visualFixedTitle}
        outfitTitle={t.merope.visualOutfitTitle}
        show={show}
        editLabel={o.editVisual}
        cancelLabel={o.cancelEdit}
        saveLabel={o.doneEditing}
        busy={generating}
        onIdentity={(next) => void persona.saveCharacter(next)}
      />
    ) : null

  const overviewCard = (
    <WorkbenchOverview
      persona={personaSnapshot}
      status={{
        name: personaSnapshot?.name.trim() || '—',
        mood: o.mood[moodBand(mood, arousal)],
        activity: o.activity[activityKey(activity)],
      }}
      rows={[
        {
          key: 'wardrobe',
          label: t.merope.wardrobeTitle,
          value: wardrobeOverview(
            sortWardrobe(wardrobeItems),
            activeOutfitId,
            t.merope,
            o.clothingStyle,
            format,
            locale,
          ),
        },
        ...(structuredPersona?.summary.trim()
          ? [
              {
                key: 'summary',
                label: t.merope.overviewSummary,
                value: structuredPersona.summary.trim(),
              },
            ]
          : []),
      ]}
      sticker={sticker}
      hasPortrait={Boolean(portraitUrl)}
      addressee={addressee}
    />
  )

  const personaCard = (
    <div className="merope-motion-persona">
      <PersonaImportPanel
        appearance="settings"
        name={personaSnapshot?.name.trim() || 'Arael'}
        disabled={generating}
        onImported={(next) =>
          void persona.saveStructuredPersona(next, { resetVisual: true })
        }
      />
      {structuredPersona ? (
        <PersonaIdentityView
          persona={structuredPersona}
          labels={{
            temperament: o.fieldTemperament,
            likes: o.fieldLikes,
            drives: o.fieldDrives,
            socialStyle: o.fieldSocial,
            speechStyle: o.fieldVoice,
            summary: o.fieldSummary,
          }}
          editLabel={o.editPersona}
          cancelLabel={o.cancelEdit}
          saveLabel={o.doneEditing}
          groupLabel={t.merope.personaGroup}
          onPersona={(next) => void persona.saveStructuredPersona(next)}
        />
      ) : (
        <p className="merope-motion-home__help">{t.merope.personaEmpty}</p>
      )}
    </div>
  )

  const closetCard = (
    <div className="merope-wardrobe__visual">
    <OutfitWardrobe
      identity={visualIdentity}
      gender={genderFromProfile(personaSnapshot?.visualProfile)}
      language={
        typeof personaSnapshot?.visualProfile?.language === 'string'
          ? (personaSnapshot.visualProfile.language as string)
          : locale
      }
      items={wardrobeItems}
      activeId={activeOutfitId}
      portraitUrl={portraitUrl}
      busy={generating}
      filling={generating && !visualIdentity}
      hasPortrait={Boolean(portraitUrl)}
      onFillFromPortrait={() => void persona.fillVisualFromPortrait()}
      onManage={async (item) => {
        setManagingOutfitId(item.id)
      }}
      onDelete={persona.deleteOutfit}
      onCreated={persona.createOutfit}
    />
    {visualIdentityView('character')}
    </div>
  )

  const managingOutfit = wardrobeItems.find(
    (item) => item.id === managingId,
  )
  const wearingManaged = managingOutfit?.id === activeOutfitId
  const outfitCard = managingOutfit ? (
    <OutfitDetail
      outfit={managingOutfit}
      wearing={wearingManaged}
      identity={
        visualIdentity ? applyOutfit(visualIdentity, managingOutfit) : null
      }
      picture={
        managingOutfit.portraitAssetId || (wearingManaged ? portraitUrl : null)
      }
      generating={generating}
      canDress={Boolean(visualIdentity)}
      visualLabels={visualLabels}
      onBack={() => setManagingOutfitId(null)}
      onRename={persona.renameOutfit}
      onWear={() => persona.wearOutfit(managingOutfit)}
      onGenerate={() => void persona.generatePortrait(managingOutfit)}
      onDownload={(url) => void downloadPicture(siteMediaUrl(url))}
      onUploaded={adoptUploadedPortrait}
      onDesign={(next) => void persona.saveOutfitDesign(managingOutfit.id, next)}
    />
  ) : null

  const studio = (
    <section
      className="merope-motion-home merope-motion-home--settings"
      aria-label={t.merope.portraitGroup}
    >
      <div className="merope-motion-home__studio">
        <div className="merope-motion-home__stage">
          <RigCharacter
            ref={rigCharacterRef}
            activity={toMeropeActivity(activity)}
            fallbackUrl={portraitUrl}
            manifest={rigManifest}
            mood={mood}
            manualControl
          />
        </div>
      </div>
    </section>
  )

  return (
    <div className="merope-motion-page">
      <aside
        className="merope-motion-page__stage"
        aria-label={t.merope.visualTitle}
        data-tour="config-persona-portrait"
      >
        {portraitStage}
      </aside>
      <div className="merope-motion-page__settings">
        <Anime25DWorkbench
          overviewLead={overviewCard}
          personaLead={personaCard}
          wardrobeLead={closetCard}
          outfitLead={outfitCard}
          outfitRig={wearingManaged}
          characterRef={rigCharacterRef}
          sourceMasterAssetId={portraitUrl || ''}
          sourceGenerationFingerprint={generationFingerprint || undefined}
          seeThroughTokenConfigured={seeThrough.configured}
          onSaveSeeThroughToken={seeThrough.save}
          onDecomposeRigPsd={decomposeRigPsd}
          onPreflightRigPsd={preflightRigPsd}
          onCommitRigPsd={commitRigPsd}
          aiExpressions={aiExpressions.ready}
          onGenerateAiExpressions={portraitUrl ? aiExpressions.generate : undefined}
          motionEnabled={motionEnabled}
          correctionPlayback={rigManifest?.anime25dPlayback ?? null}
          correctionAssetId={rigAssetId}
          onSavePoseCorrections={savePoseCorrections}
        />
      </div>
      {portraitUrl && motionEnabled && studioHost
        ? createPortal(studio, studioHost)
        : null}
    </div>
  )
}
