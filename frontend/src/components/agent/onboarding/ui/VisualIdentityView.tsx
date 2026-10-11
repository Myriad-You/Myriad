import type { ReactNode } from 'react'
import type {
  UpperBodyVisualIdentity,
  UpperBodyVisualIdentityKey,
} from '../onboardingTypes'
import { LuCheck, LuClock, LuEdit3, LuX } from '@lib/icons'
import { useState } from 'react'
import {
  CHARACTER_VISUAL_KEYS,
  OUTFIT_VISUAL_KEYS,
  UPPER_BODY_VISUAL_IDENTITY_LIMITS,
  visualField,
  withVisualField,
} from '../onboardingTypes'
import { CharMeter, TextArea } from './Field'
import '../../PersonaOnboarding.css'

interface Props {
  identity: UpperBodyVisualIdentity
  labels: Record<UpperBodyVisualIdentityKey, string>
  characterTitle: string
  outfitTitle: string
  editLabel: string
  cancelLabel: string
  saveLabel: string
  busy?: boolean
  show?: 'all' | 'character' | 'outfit'
  onIdentity: (identity: UpperBodyVisualIdentity) => void
  onEditingChange?: (editing: boolean) => void
  /** The label of the history button; no button without it. */
  historyLabel?: string
  /** What her visual settings held before, shown under the first title while open. */
  renderHistory?: (close: () => void) => ReactNode
}

export default function VisualIdentityView({
  identity,
  labels,
  characterTitle,
  outfitTitle,
  editLabel,
  cancelLabel,
  saveLabel,
  busy = false,
  show = 'all',
  onIdentity,
  onEditingChange,
  historyLabel,
  renderHistory,
}: Props) {
  const [editingField, setEditingField] =
    useState<UpperBodyVisualIdentityKey | null>(null)
  const [draft, setDraft] = useState('')
  const [historyOpen, setHistoryOpen] = useState(false)

  const startEdit = (key: UpperBodyVisualIdentityKey) => {
    setEditingField(key)
    setDraft(visualField(identity, key))
    onEditingChange?.(true)
  }

  const cancelEdit = () => {
    setEditingField(null)
    setDraft('')
    onEditingChange?.(false)
  }

  const commitEdit = () => {
    if (!editingField) return
    const next = draft.trim()
    if (!next) {
      cancelEdit()
      return
    }
    onIdentity(withVisualField(identity, editingField, next))
    setEditingField(null)
    setDraft('')
    onEditingChange?.(false)
  }

  const groups: Array<{
    title: string
    keys: readonly UpperBodyVisualIdentityKey[]
  }> = []
  if (show !== 'outfit') {
    groups.push({ title: characterTitle, keys: CHARACTER_VISUAL_KEYS })
  }
  if (show !== 'character') {
    groups.push({ title: outfitTitle, keys: OUTFIT_VISUAL_KEYS })
  }

  return (
    <div className="merope-ob-visual merope-ob-persona-groups">
      {groups.map(({ title, keys }, index) => (
        <section
          key={title}
          className="merope-ob-persona-group"
          aria-label={title}
        >
          {index === 0 && historyLabel && renderHistory ? (
            <>
              <div className="merope-ob-persona-group__head">
                <h2 className="merope-ob-persona-group__title">{title}</h2>
                <button
                  type="button"
                  className="merope-ob-persona-view__edit"
                  disabled={busy}
                  title={historyLabel}
                  aria-label={`${historyLabel} · ${title}`}
                  aria-expanded={historyOpen}
                  onClick={() => setHistoryOpen((open) => !open)}
                >
                  <LuClock aria-hidden />
                </button>
              </div>
              {historyOpen ? renderHistory(() => setHistoryOpen(false)) : null}
            </>
          ) : (
            <h2 className="merope-ob-persona-group__title">{title}</h2>
          )}
          <dl className="merope-ob-persona-view">
            {keys.map((key) => {
              const isEditing = editingField === key
              return (
                <div
                  key={key}
                  className={`merope-ob-persona-view__row${isEditing ? ' is-editing' : ''}`}
                >
                  {isEditing ? (
                    <div className="merope-ob-persona-view__editor">
                      <dt>{labels[key]}</dt>
                      <TextArea
                        rows={4}
                        maxLength={UPPER_BODY_VISUAL_IDENTITY_LIMITS[key]}
                        value={draft}
                        autoFocus
                        onChange={(event) => setDraft(event.target.value)}
                        onKeyDown={(event) => {
                          if (
                            (event.metaKey || event.ctrlKey) &&
                            event.key === 'Enter'
                          ) {
                            event.preventDefault()
                            commitEdit()
                          }
                          if (event.key === 'Escape') {
                            event.preventDefault()
                            cancelEdit()
                          }
                        }}
                      />
                      <div className="merope-ob-persona-view__actions">
                        <CharMeter
                          value={draft}
                          max={UPPER_BODY_VISUAL_IDENTITY_LIMITS[key]}
                        />
                        <button
                          type="button"
                          className="merope-ob-persona-view__action is-cancel"
                          disabled={busy}
                          title={cancelLabel}
                          aria-label={cancelLabel}
                          onClick={cancelEdit}
                        >
                          <LuX aria-hidden />
                        </button>
                        <button
                          type="button"
                          className="merope-ob-persona-view__action is-save"
                          disabled={busy}
                          title={saveLabel}
                          aria-label={saveLabel}
                          onClick={commitEdit}
                        >
                          <LuCheck aria-hidden />
                        </button>
                      </div>
                    </div>
                  ) : (
                    <>
                      <div className="merope-ob-persona-view__copy">
                        <dt>{labels[key]}</dt>
                        <dd>{visualField(identity, key)}</dd>
                      </div>
                      <button
                        type="button"
                        className="merope-ob-persona-view__edit"
                        disabled={busy}
                        title={editLabel}
                        aria-label={`${editLabel} · ${labels[key]}`}
                        onClick={() => startEdit(key)}
                      >
                        <LuEdit3 aria-hidden />
                      </button>
                    </>
                  )}
                </div>
              )
            })}
          </dl>
        </section>
      ))}
    </div>
  )
}
