import type { AuthoredExpressionKind } from '../rig/authoredExpression'
import type { RigImport, RigMode } from './useRigImport'
import { useRef } from 'react'
import {
  CheckboxItem,
  GitHubProjectBadge,
  InputItem,
  SettingsButton,
  SettingTitleTag,
} from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import {
  ANIME25D_PROJECT_NAME,
  ANIME25D_PROJECT_URL,
  PERSONA_UPSTREAM_THANKS,
} from '../anime25drig/credit'
import { FaceTabs } from './FaceTabs'
import { aiExpressionLabel, turnKeysProgress } from './rigImportCopy'
import { RigImportProgress } from './RigImportProgress'
import {
  SEE_THROUGH_PROJECT_NAME,
  SEE_THROUGH_PROJECT_URL,
} from './seeThroughProject'
import { SeeThroughSpaceField } from './SeeThroughSpaceField'
import { TurnArchiveList } from './TurnArchiveList'

export type RigPath = 'upload' | 'seeThrough'

interface Props {
  rig: RigImport
  rigPath: RigPath
  onRigPathChange: (path: RigPath) => void
  seeThroughTokenConfigured: boolean
  /** Expressions the image model has redrawn for the current portrait. */
  aiExpressions: readonly AuthoredExpressionKind[]
  canGenerateAiExpressions: boolean
  motionEnabled: boolean
  /** Whose kept turn-keys jobs to list: null (default) the worn bust, else that outfit. */
  archiveOutfitId?: string | null
  /** Names the two modes: bust / bust enhanced, or full body / full body enhanced. */
  figure?: 'bust' | 'fullBody'
}

/** The worn outfit's rig: get a layered PSD in, check it, and put it on. */
export function RigImportPanel({
  rig,
  rigPath,
  onRigPathChange,
  seeThroughTokenConfigured,
  aiExpressions,
  canGenerateAiExpressions,
  motionEnabled,
  archiveOutfitId = null,
  figure = 'bust',
}: Props) {
  const { t, format } = useI18n()
  const labels = t.merope
  const rigPsdInputRef = useRef<HTMLInputElement>(null)
  const rigPaths: Array<{ value: RigPath; label: string }> = [
    { value: 'upload', label: labels.rigPathUpload },
    { value: 'seeThrough', label: labels.rigPathSeeThrough },
  ]
  const { importing, operation } = rig
  const modes: Array<{ value: RigMode; label: string }> = [
    { value: 'plain', label: figure === 'bust' ? labels.rigModeBust : labels.rigModeFullBody },
    { value: 'enhanced', label: figure === 'bust' ? labels.rigModeBustEnhanced : labels.rigModeFullBodyEnhanced },
  ]
  const enhancing = rig.mode === 'enhanced'
  // The plain bust is an uploaded PSD only; See-through is the enhanced path's.
  const uploadOnly = !enhancing && figure === 'bust'
  const path: RigPath = uploadOnly ? 'upload' : rigPath
  const nothingChosen = enhancing && !rig.enhanceTurn && !(rig.expressionsOffered && rig.enhanceExpressions)
  const decomposeLabel = operation === 'decompose'
    ? (rig.decomposeStatus ? turnKeysProgress(labels, rig.decomposeStatus) : labels.motionSeeThroughGenerating)
    : enhancing ? labels.rigEnhanceGenerate : labels.motionSeeThroughGenerate
  const seeThroughSetup = (
    <>
      <InputItem
        itemKey="see-through-hf-token"
        label={labels.motionSeeThroughToken}
        labelAccessory={
          <SettingTitleTag
            onClick={() =>
              window.open(
                'https://huggingface.co/settings/tokens',
                '_blank',
                'noopener,noreferrer',
              )
            }
          >
            {labels.motionSeeThroughTokenCreate}
          </SettingTitleTag>
        }
        description={labels.motionSeeThroughTokenDescription}
        value={
          rig.tokenDraft ||
          (seeThroughTokenConfigured ? '••••••••' : '')
        }
        onChange={rig.editToken}
        inputType="password"
        autoComplete="off"
        placeholder="hf_…"
        variant="clickToEdit"
        emptyLabel={labels.motionSeeThroughTokenMissing}
        editLabel={labels.motionSeeThroughTokenEdit}
        saveLabel={labels.motionSeeThroughTokenSave}
        cancelLabel={labels.motionSeeThroughTokenCancel}
        onCommit={rig.saveToken}
        error={rig.tokenError}
        clearable={false}
      />
      <SeeThroughSpaceField />
    </>
  )
  const decomposeButton = (
    <SettingsButton
      type="button"
      size="sm"
      disabled={importing || !seeThroughTokenConfigured || nothingChosen}
      loading={operation === 'decompose'}
      onClick={() => void rig.decomposePsd()}
    >
      {nothingChosen ? labels.rigEnhanceNone : decomposeLabel}
    </SettingsButton>
  )
  return (
    <div className="merope-motion-rig">
      <FaceTabs
        className="merope-motion-rig__tabs"
        ariaLabel={labels.rigGroup}
        value={rig.mode}
        options={modes}
        onChange={(mode) => { if (!importing) rig.setMode(mode) }}
      />
      <p className="merope-motion-rig__hint">
        {enhancing
          ? labels.rigModeEnhancedHint
          : uploadOnly ? labels.rigModePlainBustHint : labels.rigModePlainHint}
      </p>
      {enhancing || uploadOnly ? null : (
        <FaceTabs
          className="merope-motion-rig__tabs"
          ariaLabel={labels.rigGroup}
          value={rigPath}
          options={rigPaths}
          onChange={onRigPathChange}
        />
      )}
      <div className="merope-character-home__credit">
        <p className="merope-character-home__credit-line">
          <GitHubProjectBadge
            url={SEE_THROUGH_PROJECT_URL}
            name={SEE_THROUGH_PROJECT_NAME}
          />
          {labels.anime25dSeeThroughCredit}
        </p>
        <p className="merope-character-home__credit-line">
          <GitHubProjectBadge
            url={ANIME25D_PROJECT_URL}
            name={ANIME25D_PROJECT_NAME}
          />
          {labels.anime25dRuntimeCredit}
        </p>
        <p className="merope-character-home__thanks">
          {labels.anime25dProjectThanks}
          {PERSONA_UPSTREAM_THANKS.map((person) => (
            <a
              key={person.handle}
              href={person.url}
              target="_blank"
              rel="noopener noreferrer"
            >
              @{person.handle}
            </a>
          ))}
        </p>
      </div>
      {enhancing ? (
        <section className="merope-motion-rig__path">
          {seeThroughSetup}
          <CheckboxItem
            itemKey="rig-enhance-turn"
            label={labels.rigEnhanceTurn}
            description={labels.rigEnhanceTurnHint}
            value={rig.enhanceTurn}
            onChange={rig.setEnhanceTurn}
            disabled={importing}
          />
          {rig.expressionsOffered ? (
            <CheckboxItem
              itemKey="rig-enhance-expressions"
              label={labels.rigEnhanceExpressions}
              description={labels.rigEnhanceExpressionsHint}
              value={rig.enhanceExpressions}
              onChange={rig.setEnhanceExpressions}
              disabled={importing}
            />
          ) : null}
          {rig.expressionsOffered && rig.enhanceExpressions ? (
            <p className="merope-motion-rig__hint">
              {aiExpressions.length > 0
                ? format(labels.aiExpressionsReady, {
                    kinds: aiExpressions
                      .map((kind) => aiExpressionLabel(labels, kind))
                      .join(' / '),
                  })
                : labels.aiExpressionsNone}
            </p>
          ) : null}
          {rig.aiExpressionsError ? (
            <p className="merope-motion-rig__hint" role="alert">
              {rig.aiExpressionsError}
            </p>
          ) : null}
          <span className="merope-turn-archives__actions">
            {decomposeButton}
            {canGenerateAiExpressions && rig.enhanceExpressions && aiExpressions.length > 0 ? (
              <SettingsButton
                type="button"
                size="sm"
                disabled={rig.aiExpressionsBusy || importing}
                loading={rig.aiExpressionsBusy}
                confirm={labels.aiExpressionsConfirm}
                onClick={() => void rig.generateAiExpressions()}
              >
                {rig.aiExpressionsBusy
                  ? labels.aiExpressionsGenerating
                  : labels.aiExpressionsRegenerate}
              </SettingsButton>
            ) : null}
          </span>
          {seeThroughTokenConfigured ? (
            <TurnArchiveList
              outfitId={archiveOutfitId}
              busy={importing}
              onRefit={(archiveId) => void rig.decomposePsd(archiveId)}
            />
          ) : null}
        </section>
      ) : path === 'upload' ? (
        <section className="merope-motion-rig__path">
          <SettingsButton
            type="button"
            size="sm"
            disabled={importing}
            loading={operation === 'manual'}
            onClick={() => rigPsdInputRef.current?.click()}
          >
            {operation === 'manual'
              ? labels.motionPsdUploading
              : labels.motionPsdUpload}
          </SettingsButton>
        </section>
      ) : (
        <section className="merope-motion-rig__path">
          {seeThroughSetup}
          {decomposeButton}
        </section>
      )}
      <input
        ref={rigPsdInputRef}
        type="file"
        accept=".psd,image/vnd.adobe.photoshop"
        hidden
        onChange={(event) => {
          const file = event.currentTarget.files?.[0]
          event.currentTarget.value = ''
          if (file) void rig.preflightPsd(file)
        }}
      />
      {operation === 'manual' && (
        <SettingsButton
          type="button"
          size="sm"
          onClick={rig.cancelPreflight}
        >
          {labels.motionSeeThroughTokenCancel}
        </SettingsButton>
      )}
      <RigImportProgress rig={rig} />
      {motionEnabled ? (
        <section className="merope-motion-rig__status">
          <strong>{labels.rigReadyTitle}</strong>
          <p className="merope-motion-rig__hint">
            {labels.rigReadyHint}
          </p>
        </section>
      ) : null}
    </div>
  )
}
