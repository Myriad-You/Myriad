import type {
  OnboardingHeaderChrome,
  PersonaGender,
  StructuredPersona,
} from '../onboardingTypes'
import { useLayoutEffect, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { generationFailureMessage } from '../generationError'
import { structuredPersonaIsComplete } from '../onboardingTypes'
import { ActionBar, PrimaryButton } from '../ui/Chrome'
import { ErrorNote } from '../ui/Feedback'
import { FieldGroup, TextInput } from '../ui/Field'
import GenderPicker from '../ui/GenderPicker'
import PersonaImportPanel from '../ui/PersonaImportPanel'
import PortraitImportButton from '../ui/PortraitImportButton'

interface Props {
  displayName: string
  gender: PersonaGender | null
  persona: StructuredPersona
  portraitUrl: string | null
  busy: boolean
  onDisplayName: (value: string) => void
  onGender: (value: PersonaGender) => void
  onImported: (persona: StructuredPersona) => void
  onPortrait: (portraitUrl: string) => void
  onHeaderChange: (chrome: OnboardingHeaderChrome) => void
  onSubmit: () => Promise<void>
}

/**
 * 现成的人设 + 现成的主立绘，一次落地。
 *
 * 这一步不产出视觉设定：上传主图走 `upload_portrait`，它不像出图那样要求
 * 完整的 `visualIdentity`，所以导入进来的形象可以直接当血统源头用，后面
 * 去动作工作台分层即可。
 */
export default function ImportStep({
  displayName,
  gender,
  persona,
  portraitUrl,
  busy,
  onDisplayName,
  onGender,
  onImported,
  onPortrait,
  onHeaderChange,
  onSubmit,
}: Props) {
  const { t } = useI18n()
  const o = t.agentPersona.onboarding
  const [error, setError] = useState('')

  // 不报 onBack：页面壳会问 previousOnboardingStep，导入的上一页就是分岔口。
  // 自己接这一手会盖掉那个判定——它曾经指向生成链第一页（分岔口还不存在时
  // 导入确实是从那儿岔出去的），加了分岔口之后就成了「返回跑去词条页」。
  useLayoutEffect(() => {
    onHeaderChange({ description: o.importLead })
  }, [o.importLead, onHeaderChange])

  const personaReady = structuredPersonaIsComplete(persona)

  return (
    <section className="merope-ob-import" aria-label={o.importTitle}>
      <div className="merope-ob-import__layout">
        <div className="merope-ob-import__form">
          <FieldGroup label={o.nameLabel}>
            <TextInput
              value={displayName}
              maxLength={40}
              placeholder={o.namePlaceholder}
              disabled={busy}
              onChange={(event) => onDisplayName(event.target.value)}
              aria-label={o.nameLabel}
            />
            <small className="merope-ob-field__hint">{o.nameHint}</small>
          </FieldGroup>

          <FieldGroup label={o.genderLabel}>
            <GenderPicker
              label={o.genderLabel}
              value={gender}
              labels={o.gender}
              disabled={busy}
              onChange={onGender}
            />
          </FieldGroup>

          <PersonaImportPanel
            name={displayName.trim() || 'Arael'}
            gender={gender ?? undefined}
            disabled={busy}
            ready={personaReady}
            onImported={(next) => {
              setError('')
              onImported(next)
            }}
          />

          {error ? <ErrorNote>{error}</ErrorNote> : null}
        </div>

        <PortraitImportButton
          disabled={busy}
          previewUrl={portraitUrl}
          onError={setError}
          onUploaded={(url) => {
            setError('')
            onPortrait(url)
          }}
        />
      </div>
      <ActionBar>
        <PrimaryButton
          label={o.importFinish}
          busy={busy}
          disabled={!personaReady || !gender}
          onClick={() => {
            setError('')
            if (!gender) {
              setError(o.genderRequired)
              return
            }
            void onSubmit().catch((reason) => {
              setError(
                generationFailureMessage(
                  reason,
                  o.createFailed,
                  o.generationTimeout,
                  { pro_unavailable: o.proUnavailable },
                ),
              )
            })
          }}
        />
      </ActionBar>
    </section>
  )
}
