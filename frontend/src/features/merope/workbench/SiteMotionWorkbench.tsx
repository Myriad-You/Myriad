import type { UpperBodyVisualIdentityKey } from '../../../components/agent/onboarding/onboardingTypes'
import type { PoseCorrection } from '../anime25drig/poseCorrections'
import type { RigCharacterHandle } from '../character/RigCharacter'
import type { WardrobeItem } from '../persona/wardrobe'
import type { TurnKeyformsStatus } from '../turnKeyformsApi'
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { createPortal } from 'react-dom'
import { activityKey, moodBand } from '../../../components/agent/meropeVitals'
import AgentMindPanel from '../../../components/agent/mind/AgentMindPanel'
import { genderFromProfile } from '../../../components/agent/onboarding/onboardingTypes'
import PersonaIdentityView from '../../../components/agent/onboarding/ui/PersonaIdentityView'
import PersonaImportPanel from '../../../components/agent/onboarding/ui/PersonaImportPanel'
import VisualIdentityView from '../../../components/agent/onboarding/ui/VisualIdentityView'
import { SettingTitleTag } from '../../../components/settings'
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
  saveFullBodyPoseCorrections,
  saveRigPoseCorrections,
  uploadFullBodyPortrait,
  uploadOutfitPortrait,
} from '../api'
import { commitRigPsdAsset, preflightRigPsdAsset } from '../assets/pipeline'
import RigCharacter from '../character/RigCharacter'
import { notifyFaceUpdated } from '../events/updates'
import { useRigPreviewMotionLifecycle } from '../motion/useRigMotionLifecycle'
import { applyOutfit, isFullBodyItem, sortWardrobe, wardrobeItemLabel } from '../persona/wardrobe'
import { decomposeWithTurnKeyforms } from '../turnKeyformsApi'
import Anime25DWorkbench from './Anime25DWorkbench'
import { FullBodyStageSwitch } from './FullBodyStageSwitch'
import { OutfitDetail } from './OutfitDetail'
import OutfitWardrobe from './OutfitWardrobe'
import { activeRigMode } from './rigMode'
import { useAddressee } from './useAddressee'
import { useAiExpressions } from './useAiExpressions'
import { useFullBodyStage } from './useFullBodyStage'
import { usePersonaWardrobe } from './usePersonaWardrobe'
import { useSeeThroughToken } from './useSeeThroughToken'
import { useSiteFace } from './useSiteFace'
import { useStickerAvatar } from './useStickerAvatar'
import { wardrobeOverview } from './wardrobeOverview'
import { WorkbenchOverview } from './WorkbenchOverview'
import { downloadPicture, reportMeropeError, toMeropeActivity } from './workbenchShared'
import './merope.css'
import './merope-motion-home.css'

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
  // The stage plays the worn bust, or any full-body set in the wardrobe.
  const fullBody = useFullBodyStage(wardrobeItems, persona.activeFullBodyId)
  const staged = fullBody.staged
  const managingId = personaTouring ? null : managingOutfitId
  const [studioHost, setStudioHost] = useState<HTMLDivElement | null>(null)
  const [importingPersona, setImportingPersona] = useState(false)
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
      options?: { aiExpressions?: boolean },
    ) => {
      if (!portraitUrl) throw new Error(t.merope.assetNeedsPortrait)
      return preflightRigPsdAsset(
        file,
        portraitUrl,
        onStage,
        generationFingerprint || undefined,
        signal,
        // The redrawn expressions belong to the enhanced rig only.
        options?.aiExpressions ? expressionReferences() : [],
      )
    },
    [expressionReferences, generationFingerprint, portraitUrl, t.merope.assetNeedsPortrait],
  )

  const decomposeRigPsd = useCallback(async (
    onStatus: (status: TurnKeyformsStatus) => void,
    signal: AbortSignal,
    fromArchive?: string,
  ) => {
    if (!portraitUrl) throw new Error(t.merope.assetNeedsPortrait)
    return decomposeWithTurnKeyforms(
      { sourceMasterAssetId: portraitUrl, sourceGenerationFingerprint: generationFingerprint || undefined },
      { fromArchive, onStatus, signal },
    )
  }, [generationFingerprint, portraitUrl, t.merope.assetNeedsPortrait])

  const decomposePlainRigPsd = useCallback(async () => {
    if (!portraitUrl) throw new Error(t.merope.assetNeedsPortrait)
    return decomposeSitePortraitWithSeeThrough({
      sourceMasterAssetId: portraitUrl,
      sourceGenerationFingerprint: generationFingerprint || undefined,
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

  const stagedSet = staged
    ? (fullBody.sets.find((item) => item.id === fullBody.stagedId) ?? null)
    : null
  const wornItem = wardrobeItems.find((item) => item.id === activeOutfitId)
  const wornLabel = wornItem
    ? wardrobeItemLabel(wornItem, o.clothingStyle, t.merope.wardrobeDefault)
    : t.merope.wardrobeDefault
  const saveFullBodyCorrections = async (corrections: PoseCorrection[]) => {
    const assetId = staged?.assetId
    if (!stagedSet || !assetId) throw new Error(t.merope.poseCorrection.conflict)
    await saveFullBodyPoseCorrections(stagedSet.id, assetId, corrections)
    // The set now names its new package; the stage loads it.
    await reload()
  }

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

  /** A new set made from the owner's own picture instead of a generated one. */
  const createOutfitFromUpload = async (item: WardrobeItem, file: File) => {
    // Added without being worn: a bust keeps the master portrait it has.
    await persona.addOutfit(item, { wear: false })
    if (isFullBodyItem(item)) await uploadFullBodyPortrait(item.id, file)
    else await uploadOutfitPortrait(item.id, file)
    await reload()
    persona.setManagingOutfitId(item.id)
  }

  const motionEnabled = Boolean(
    rigManifest?.anime25dPlayback &&
    isAnime25DPlayback(rigManifest.anime25dPlayback) &&
    rigManifest.textures[0]?.url,
  )

  const portraitStage = (
    <div
      className={`merope-motion-asset__preview${staged ? ' merope-motion-asset__preview--full-body' : ''}`}
    >
      {portraitUrl ? (
        <img
          className={`merope-motion-asset__still${motionEnabled ? ' is-behind' : ''}`}
          src={siteMediaUrl(staged?.portraitUrl ?? portraitUrl)}
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

  // With a persona already, the import box opens from the page title.
  const importFolded = Boolean(structuredPersona) && !importingPersona
  const personaCard = (
    <div className="merope-motion-persona">
      {importFolded ? null : (
        <PersonaImportPanel
          appearance="settings"
          name={personaSnapshot?.name.trim() || 'Arael'}
          disabled={generating}
          onImported={async (next) => {
            await persona.saveStructuredPersona(next, { resetVisual: true })
            setImportingPersona(false)
          }}
        />
      )}
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
      activeFullBodyId={persona.activeFullBodyId}
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
      onUpload={createOutfitFromUpload}
    />
    {visualIdentityView('character')}
    </div>
  )

  const managingOutfit = wardrobeItems.find(
    (item) => item.id === managingId,
  )
  const wearingManaged = managingOutfit?.id === activeOutfitId
  // A full body is worn apart from the bust.
  const wearingNow =
    managingOutfit && isFullBodyItem(managingOutfit)
      ? managingOutfit.id === persona.activeFullBodyId
      : wearingManaged
  const managingReference = managingOutfit?.referenceOutfitId
    ? wardrobeItems.find((item) => item.id === managingOutfit.referenceOutfitId)
    : undefined
  const outfitCard = managingOutfit ? (
    <OutfitDetail
      outfit={managingOutfit}
      wearing={wearingNow}
      reference={
        !managingOutfit.referenceOutfitId
          ? t.merope.fullBody.referenceNoneNote
          : managingReference &&
              (managingReference.portraitAssetId ||
                managingReference.id === activeOutfitId)
            ? format(t.merope.fullBody.referenceOf, {
                name: wardrobeItemLabel(
                  managingReference,
                  o.clothingStyle,
                  t.merope.wardrobeDefault,
                ),
              })
            : t.merope.fullBody.referenceGone
      }
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
      seeThroughTokenConfigured={seeThrough.configured}
      onSaveSeeThroughToken={seeThrough.save}
      // The server wrote the set's picture or rig.
      onFullBodyChanged={() => void persona.reload()}
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
            fallbackUrl={staged ? staged.portraitUrl : portraitUrl}
            manifest={staged ? staged.manifest : rigManifest}
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
          personaTitleExtra={
            structuredPersona ? (
              <SettingTitleTag
                disabled={generating}
                onClick={() => setImportingPersona((open) => !open)}
              >
                {importingPersona ? t.common.cancel : o.importPersona}
              </SettingTitleTag>
            ) : null
          }
          mindLead={<AgentMindPanel />}
          wardrobeLead={closetCard}
          outfitLead={outfitCard}
          outfitRig={wearingManaged}
          characterRef={rigCharacterRef}
          sourceMasterAssetId={portraitUrl || ''}
          sourceGenerationFingerprint={generationFingerprint || undefined}
          seeThroughTokenConfigured={seeThrough.configured}
          onSaveSeeThroughToken={seeThrough.save}
          onDecomposeRigPsd={decomposeRigPsd}
          onDecomposePlainPsd={decomposePlainRigPsd}
          onPreflightRigPsd={preflightRigPsd}
          onCommitRigPsd={commitRigPsd}
          aiExpressions={aiExpressions.ready}
          onGenerateAiExpressions={portraitUrl ? aiExpressions.generate : undefined}
          aiExpressionsReady={aiExpressions.ready.length > 0}
          activeRigMode={activeRigMode(rigManifest, portraitUrl)}
          motionEnabled={motionEnabled}
          stageSwitch={
            fullBody.sets.length > 0 ? (
              <FullBodyStageSwitch
                sets={fullBody.sets}
                stagedId={fullBody.stagedId}
                onChange={fullBody.stage}
              />
            ) : null
          }
          stageKey={staged ? (fullBody.stagedId ?? '') : 'bust'}
          // Corrections follow the figure on stage: the worn bust or a full body.
          correctionPlayback={staged ? (staged.manifest.anime25dPlayback ?? null) : (rigManifest?.anime25dPlayback ?? null)}
          correctionAssetId={staged ? staged.assetId : rigAssetId}
          correctionTarget={stagedSet
            ? format(t.merope.poseCorrection.targetFullBody, { name: wardrobeItemLabel(stagedSet, o.clothingStyle) })
            : format(t.merope.poseCorrection.targetBust, { name: wornLabel })}
          onSavePoseCorrections={stagedSet ? saveFullBodyCorrections : savePoseCorrections}
        />
      </div>
      {portraitUrl && motionEnabled && studioHost
        ? createPortal(studio, studioHost)
        : null}
    </div>
  )
}
