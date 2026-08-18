import { useCallback, useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { invalidatePublicConfigCache } from '../../utils/requestDedup'
import {
  ActionBar,
  Aurora,
  BackButton,
  BrandTag,
  Field,
  PrimaryButton,
  StepBody,
  StepHero,
  StepTopBar,
  TextInput,
} from '../setup/SetupChrome'
import { uniqPersonaTags } from './personaTags'
import '../SetupWizard.css'

interface Props {
  open: boolean
  seedTags: string[]
  onClose: () => void
  onSaved: (name: string) => void
}

type Step = 1 | 2 | 3

export default function PersonaOnboarding({
  open,
  seedTags,
  onClose,
  onSaved,
}: Props) {
  const { t } = useI18n()
  const o = t.life.onboarding
  const [step, setStep] = useState<Step>(1)
  const [tags, setTags] = useState<string[]>([])
  const [selected, setSelected] = useState<string[]>([])
  const [name, setName] = useState('')
  const [personality, setPersonality] = useState('')
  const [saving, setSaving] = useState(false)
  const [drafting, setDrafting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    setStep(1)
    setSelected([])
    setName('')
    setPersonality('')
    setError(null)
    setDrafting(false)
  }, [open])

  useEffect(() => {
    if (!open) return
    let cancelled = false
    void (async () => {
      const remote = await agentService.getPersonaSignals()
      if (cancelled) return
      setTags(
        uniqPersonaTags([
          ...(remote?.tags.map((tag) => tag.label) ?? []),
          ...seedTags,
        ]),
      )
    })()
    return () => {
      cancelled = true
    }
  }, [open, seedTags])

  const toggle = (tag: string) => {
    setSelected((prev) =>
      prev.includes(tag) ? prev.filter((item) => item !== tag) : [...prev, tag],
    )
  }

  const composePersonality = useCallback(() => {
    if (personality.trim()) return personality.trim()
    return selected.join('、')
  }, [personality, selected])

  const save = async () => {
    setSaving(true)
    setError(null)
    try {
      const saved = await agentService.putPersona({
        name: name.trim(),
        personality: composePersonality(),
      })
      invalidatePublicConfigCache()
      window.dispatchEvent(new CustomEvent('arael-persona-updated'))
      onSaved(saved.name || 'Arael')
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : o.createFailed)
    } finally {
      setSaving(false)
    }
  }

  if (!open) return null

  const stepName =
    step === 1 ? o.step1Short : step === 2 ? o.step2Short : o.step3Short
  const title = step === 1 ? o.step1Title : step === 2 ? o.step2Title : o.step3Title
  const lead = step === 1 ? o.step1Lead : step === 2 ? o.step2Lead : o.step3Lead

  return (
    <div className="fixed inset-0 z-[100000] flex items-center justify-center p-4 bg-black/45">
      <section className="setup-ob" style={{ width: 'min(100%, 520px)', height: 'min(86dvh, 640px)' }}>
        <Aurora />
        <div className="setup-ob__card">
          <StepTopBar
            back={
              step === 1 ? (
                <BrandTag label={o.title} />
              ) : (
                <BackButton
                  label={o.backTo.replace('{step}', step === 2 ? o.step1Short : o.step2Short)}
                  destination={step === 2 ? o.step1Short : o.step2Short}
                  onClick={() => setStep((step - 1) as Step)}
                />
              )
            }
            stepName={stepName}
            current={step}
            total={3}
            progressText={o.stepOf
              .replace('{current}', String(step))
              .replace('{total}', '3')}
          />
          <div className="setup-ob__viewport">
            <div className="setup-ob__pane">
              <StepHero title={title} lead={lead} titleId="persona-ob-title" />
              <StepBody>
                {step === 1 && (
                  <>
                    {tags.length === 0 ? (
                      <p className="text-sm opacity-70">{o.noReports}</p>
                    ) : (
                      <div className="flex flex-wrap gap-2">
                        {tags.map((tag) => {
                          const on = selected.includes(tag)
                          return (
                            <button
                              key={tag}
                              type="button"
                              className={`px-3 py-1.5 rounded-full text-xs border transition ${
                                on
                                  ? 'border-transparent text-white'
                                  : 'border-[color:var(--sob-line)]'
                              }`}
                              style={
                                on
                                  ? { background: 'var(--sob-accent, var(--color-primary))' }
                                  : undefined
                              }
                              onClick={() => toggle(tag)}
                            >
                              {tag}
                            </button>
                          )
                        })}
                      </div>
                    )}
                    <p className="text-xs opacity-60 mt-3">
                      {selected.length
                        ? o.selectedCount.replace('{count}', String(selected.length))
                        : o.selectNothingYet}
                    </p>
                  </>
                )}
                {step === 2 && (
                  <Field label={o.nameLabel} hint={o.nameHint}>
                    <TextInput
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                      placeholder={o.namePlaceholder}
                    />
                  </Field>
                )}
                {step === 3 && (
                  <Field label={o.personaGroupCharacter}>
                    <textarea
                      className="setup-ob-input"
                      rows={7}
                      value={personality}
                      onChange={(e) => setPersonality(e.target.value)}
                      placeholder={selected.join('、') || o.extraPlaceholder}
                    />
                  </Field>
                )}
                {error ? <p className="text-xs text-red-500 mt-2">{error}</p> : null}
              </StepBody>
              <ActionBar>
                {step < 3 ? (
                  <PrimaryButton
                    label={step === 1 && tags.length === 0 ? o.skipTags : o.next}
                    busy={drafting}
                    onClick={() => {
                      if (step === 2) {
                        void (async () => {
                          setDrafting(true)
                          try {
                            const draft = await agentService.draftPersona({
                              name,
                              tags: selected,
                            })
                            if (!personality.trim()) {
                              setPersonality(
                                draft?.personality || selected.join('、'),
                              )
                            }
                            setStep(3)
                          } catch (e) {
                            setError(
                              e instanceof Error ? e.message : o.createFailed,
                            )
                            setPersonality((prev) => prev || selected.join('、'))
                            setStep(3)
                          } finally {
                            setDrafting(false)
                          }
                        })()
                        return
                      }
                      setStep((step + 1) as Step)
                    }}
                  />
                ) : (
                  <PrimaryButton
                    label={saving ? o.creating : o.createAndContinue}
                    busy={saving}
                    onClick={() => void save()}
                  />
                )}
              </ActionBar>
            </div>
          </div>
        </div>
        <button
          type="button"
          className="absolute top-3 right-4 text-xs opacity-60"
          onClick={onClose}
        >
          {t.common.cancel}
        </button>
      </section>
    </div>
  )
}
