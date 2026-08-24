import type {
  PersonaGender,
  NameStyle,
  OnboardingHeaderChrome,
} from '../onboardingTypes'
import { useLayoutEffect, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { agentService } from '../../../../services/agent'
import { generationFailureMessage } from '../generationError'
import { defaultNameStyle } from '../onboardingTypes'
import { ActionBar, PrimaryButton, StepBody } from '../ui/Chrome'
import { ErrorNote } from '../ui/Feedback'
import { Field, FieldGroup, TextArea, TextInput } from '../ui/Field'
import GenderPicker from '../ui/GenderPicker'
import NameStyleRoll from '../ui/NameStyleRoll'

interface Props {
  displayName: string
  gender: PersonaGender | null
  extraRequirements: string
  selectedTags: string[]
  busy: boolean
  onDisplayName: (value: string) => void
  onGender: (value: PersonaGender) => void
  onExtra: (value: string) => void
  onSubmit: () => Promise<void>
  onHeaderChange: (chrome: OnboardingHeaderChrome) => void
}

export default function BasicsStep({
  displayName,
  gender,
  extraRequirements,
  selectedTags,
  busy,
  onDisplayName,
  onGender,
  onExtra,
  onSubmit,
  onHeaderChange,
}: Props) {
  const { t, locale } = useI18n()
  const o = t.agentPersona.onboarding
  const [localError, setLocalError] = useState('')
  const [nameError, setNameError] = useState('')
  const [rollingName, setRollingName] = useState(false)
  const [nameStyle, setNameStyle] = useState<NameStyle>(() =>
    defaultNameStyle(locale),
  )

  useLayoutEffect(() => {
    onHeaderChange({ description: o.step2Lead })
  }, [o.step2Lead, onHeaderChange])

  const rollDisplayName = () => {
    if (busy || rollingName) return
    setNameError('')
    setRollingName(true)
    const tags = selectedTags
      .map((tag) => tag.trim())
      .filter(Boolean)
      .slice(0, 28)
    void agentService
      .suggestPersonaName({
        selectedTags: tags,
        gender: gender ?? undefined,
        avoidName: displayName.trim() || undefined,
        nameStyle,
        language: locale,
      })
      .then((response) => {
        if (!response.name?.trim()) {
          throw new Error(o.randomNameFailed)
        }
        onDisplayName(response.name)
        setNameError('')
      })
      .catch((reason) => {
        setNameError(
          generationFailureMessage(
            reason,
            o.randomNameFailed,
            o.generationTimeout,
            {
              standard_unavailable: o.standardUnavailable,
              name_suggest_failed: o.randomNameFailed,
              name_unusable: o.nameUnusable,
            },
          ),
        )
      })
      .finally(() => setRollingName(false))
  }

  return (
    <section aria-label={o.step2Title}>
      <StepBody>
        <FieldGroup label={o.nameLabel}>
          <div className="merope-ob-name-field">
            <TextInput
              value={displayName}
              maxLength={40}
              placeholder={o.namePlaceholder}
              disabled={busy || rollingName}
              onChange={(event) => onDisplayName(event.target.value)}
              aria-label={o.nameLabel}
            />
            <NameStyleRoll
              style={nameStyle}
              labels={o.nameStyle}
              styleLabel={o.nameStyleLabel}
              rollLabel={o.randomName}
              busyLabel={o.randomNameBusy}
              disabled={busy}
              rolling={rollingName}
              onStyle={setNameStyle}
              onRoll={rollDisplayName}
            />
          </div>
          {nameError ? (
            <small className="merope-ob-field__hint" role="alert">
              {nameError}
            </small>
          ) : (
            <small className="merope-ob-field__hint">{o.nameHint}</small>
          )}
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

        <Field label={o.extraLabel} optional optionalLabel={o.optional}>
          <TextArea
            value={extraRequirements}
            maxLength={500}
            rows={3}
            placeholder={o.extraPlaceholder}
            onChange={(event) => onExtra(event.target.value)}
          />
        </Field>

        {localError && <ErrorNote>{localError}</ErrorNote>}
      </StepBody>
      <ActionBar>
        <PrimaryButton
          label={o.next}
          busy={busy}
          disabled={!gender || rollingName}
          onClick={() => {
            setLocalError('')
            if (!gender) {
              setLocalError(o.genderRequired)
              return
            }
            void onSubmit().catch((reason) => {
              setLocalError(
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
