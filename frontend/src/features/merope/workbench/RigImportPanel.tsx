import type { AuthoredExpressionKind } from '../rig/authoredExpression'
import type { RigImport } from './useRigImport'
import { useRef } from 'react'
import {
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
import { aiExpressionLabel } from './rigImportCopy'
import { RigImportProgress } from './RigImportProgress'
import {
  SEE_THROUGH_PROJECT_NAME,
  SEE_THROUGH_PROJECT_URL,
} from './seeThroughProject'

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
}: Props) {
  const { t, format } = useI18n()
  const labels = t.merope
  const rigPsdInputRef = useRef<HTMLInputElement>(null)
  const rigPaths: Array<{ value: RigPath; label: string }> = [
    { value: 'upload', label: labels.rigPathUpload },
    { value: 'seeThrough', label: labels.rigPathSeeThrough },
  ]
  const { importing, operation } = rig
  return (
    <div className="merope-motion-rig">
      <FaceTabs
        className="merope-motion-rig__tabs"
        ariaLabel={labels.rigGroup}
        value={rigPath}
        options={rigPaths}
        onChange={onRigPathChange}
      />
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
      {rigPath === 'upload' ? (
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
          {seeThroughTokenConfigured ? (
            <p className="merope-motion-rig__token-ready">
              {labels.rigTokenReady}
            </p>
          ) : null}
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
          <SettingsButton
            type="button"
            size="sm"
            disabled={importing || !seeThroughTokenConfigured}
            loading={operation === 'decompose'}
            onClick={() => void rig.decomposePsd()}
          >
            {operation === 'decompose'
              ? labels.motionSeeThroughGenerating
              : labels.motionSeeThroughGenerate}
          </SettingsButton>
        </section>
      )}
      {canGenerateAiExpressions ? (
        <section className="merope-motion-rig__status">
          <strong>{labels.aiExpressionsTitle}</strong>
          <p className="merope-motion-rig__hint">
            {labels.aiExpressionsHint}
          </p>
          <p className="merope-motion-rig__hint">
            {aiExpressions.length > 0
              ? format(labels.aiExpressionsReady, {
                  kinds: aiExpressions
                    .map((kind) => aiExpressionLabel(labels, kind))
                    .join(' / '),
                })
              : labels.aiExpressionsNone}
          </p>
          {rig.aiExpressionsError ? (
            <p className="merope-motion-rig__hint" role="alert">
              {rig.aiExpressionsError}
            </p>
          ) : null}
          <SettingsButton
            type="button"
            size="sm"
            disabled={rig.aiExpressionsBusy}
            loading={rig.aiExpressionsBusy}
            confirm={labels.aiExpressionsConfirm}
            onClick={() => void rig.generateAiExpressions()}
          >
            {rig.aiExpressionsBusy
              ? labels.aiExpressionsGenerating
              : aiExpressions.length > 0
                ? labels.aiExpressionsRegenerate
                : labels.aiExpressionsGenerate}
          </SettingsButton>
        </section>
      ) : null}
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
