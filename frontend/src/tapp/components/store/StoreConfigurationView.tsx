import type { RemoteStoreSource } from '../../services/RemoteStoreService'
import {
  FaEdit,
  FaGlobe,
  FaPlus,
  FaSync,
  FaTimesCircle,
  FaTrash,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useState } from 'react'
import {
  InputItem,
  ManagedList,
  SettingGroup,
  SettingsButton,
  SettingTitleTag,
  ToggleSwitch,
} from '../../../components/settings'
import { Spinner } from '../../../components/Spinner'
import { useI18n } from '../../../contexts/I18nContext'
import { userFacingError } from '../../../utils/userFacingError'
import { OfficialVerifiedDot } from './storeAppMeta'
import '../../../components/ConfigForm.css'

interface SourceDraft {
  name: string
  url: string
}

export function StoreConfigurationView({
  sources,
  onToggle,
  onRemove,
  onAdd,
  onUpdate,
  onRefresh,
  refreshing,
  isAdmin,
}: {
  sources: RemoteStoreSource[]
  onToggle: (url: string, enabled: boolean) => void
  onRemove: (url: string) => void | Promise<void>
  onAdd: (
    source: Omit<RemoteStoreSource, 'id' | 'official'>,
  ) => void | Promise<void>
  onUpdate: (
    source: RemoteStoreSource,
    patch: { name: string; url: string },
  ) => void | Promise<void>
  onRefresh: () => void
  refreshing: boolean
  isAdmin: boolean
}) {
  const [formMode, setFormMode] = useState<'closed' | 'add' | 'edit'>('closed')
  const [editingSource, setEditingSource] = useState<RemoteStoreSource | null>(
    null,
  )
  const [draft, setDraft] = useState<SourceDraft>({ name: '', url: '' })
  const [formError, setFormError] = useState('')
  const [saving, setSaving] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<{
    url: string
    name: string
  } | null>(null)
  const [deleting, setDeleting] = useState(false)
  const { t } = useI18n()

  const closeForm = () => {
    setFormMode('closed')
    setEditingSource(null)
    setDraft({ name: '', url: '' })
    setFormError('')
  }

  const openAddForm = () => {
    setFormMode('add')
    setEditingSource(null)
    setDraft({ name: '', url: '' })
    setFormError('')
  }

  const openEditForm = (source: RemoteStoreSource) => {
    setFormMode('edit')
    setEditingSource(source)
    setDraft({ name: source.name, url: source.url })
    setFormError('')
  }

  const validateDraft = (): boolean => {
    if (!draft.url.trim() || !draft.name.trim()) {
      setFormError(t.tapp.fillNameAndUrl)
      return false
    }
    try {
      // eslint-disable-next-line no-new
      new URL(draft.url.trim())
    } catch {
      setFormError(t.tapp.invalidUrl)
      return false
    }
    return true
  }

  const handleSubmit = async () => {
    if (!validateDraft()) return

    try {
      setSaving(true)
      if (formMode === 'add') {
        await onAdd({
          name: draft.name.trim(),
          url: draft.url.trim(),
          enabled: true,
        })
      } else if (formMode === 'edit' && editingSource) {
        await onUpdate(editingSource, {
          name: draft.name.trim(),
          url: draft.url.trim(),
        })
      }
      closeForm()
    } catch (error) {
      setFormError(
        userFacingError(
          error,
          formMode === 'edit'
            ? t.tapp.updateSourceFailed
            : t.tapp.addSourceFailed,
        ),
      )
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="as-detail as-store-configuration">
      <header className="as-store-configuration__hero">
        <h3 className="as-store__page-title">{t.tapp.storeConfiguration}</h3>
      </header>

      <SettingGroup
        toc={false}
        title={t.tapp.storeSources}
        icon={<FaGlobe />}
        titleExtra={
          isAdmin ? (
            <span className="as-store-configuration__title-actions">
              <SettingTitleTag
                variant="muted"
                icon={
                  refreshing ? (
                    <Spinner size="xs" color="current" />
                  ) : (
                    <FaSync />
                  )
                }
                disabled={refreshing}
                onClick={onRefresh}
                title={t.tapp.refreshAllStores}
              >
                {t.tapp.refreshAllStores}
              </SettingTitleTag>
              <SettingTitleTag
                icon={formMode !== 'closed' ? <FaTimesCircle /> : <FaPlus />}
                onClick={() => {
                  if (formMode !== 'closed') closeForm()
                  else openAddForm()
                }}
                title={
                  formMode !== 'closed' ? t.tapp.cancel : t.tapp.addSource
                }
              >
                {formMode !== 'closed' ? t.tapp.cancel : t.tapp.addSource}
              </SettingTitleTag>
            </span>
          ) : undefined
        }
        className="as-store-configuration__group"
      >
        <AnimatePresence initial={false}>
          {isAdmin && formMode !== 'closed' && (
            <motion.div
              key="store-source-form"
              initial={{ opacity: 0, height: 0 }}
              animate={{
                opacity: 1,
                height: 'auto',
                transition: {
                  height: { duration: 0.28, ease: [0.22, 1, 0.36, 1] },
                  opacity: { duration: 0.2 },
                },
              }}
              exit={{
                opacity: 0,
                height: 0,
                transition: {
                  height: { duration: 0.22, ease: [0.4, 0, 1, 1] },
                  opacity: { duration: 0.14 },
                },
              }}
              className="managed-list-form as-store-configuration__form"
              style={{ overflow: 'hidden' }}
            >
              <div className="managed-list-form-body settings-stack">
                <p className="settings-text-3" style={{ margin: 0 }}>
                  {formMode === 'edit' ? t.tapp.editSource : t.tapp.addSource}
                </p>
                <InputItem
                  itemKey="tapp-store-source-name"
                  label={t.tapp.sourceName}
                  value={draft.name}
                  onChange={(value) => {
                    setDraft((prev) => ({ ...prev, name: value }))
                    setFormError('')
                  }}
                  placeholder={t.tapp.sourceName}
                  autoComplete="off"
                />
                <InputItem
                  itemKey="tapp-store-source-url"
                  label={t.tapp.sourceUrl}
                  value={draft.url}
                  onChange={(value) => {
                    setDraft((prev) => ({ ...prev, url: value }))
                    setFormError('')
                  }}
                  placeholder={t.tapp.sourceUrl}
                  inputType="url"
                  autoComplete="off"
                  error={formError || undefined}
                />
                <div className="managed-list-form-actions">
                  <SettingsButton
                    variant="primary"
                    size="sm"
                    icon={formMode === 'edit' ? <FaEdit /> : <FaPlus />}
                    loading={saving}
                    disabled={saving}
                    onClick={() => void handleSubmit()}
                  >
                    {formMode === 'edit' ? t.tapp.saveSource : t.tapp.addSource}
                  </SettingsButton>
                  <SettingsButton
                    variant="secondary"
                    size="sm"
                    disabled={saving}
                    onClick={closeForm}
                  >
                    {t.tapp.cancel}
                  </SettingsButton>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
        <ManagedList
          className="as-store-configuration__list"
          maxHeight={null}
          loading={refreshing && sources.length === 0}
          working={refreshing}
          items={sources.map((source) => ({
            id: source.id ?? source.url,
            title: (
              <>
                {source.name}
                {source.official ? (
                  <OfficialVerifiedDot label={t.tapp.official} />
                ) : null}
              </>
            ),
            subtitle: source.url,
            meta: source.description,
            badges: !source.enabled
              ? [{ label: t.tapp.disabled, tone: 'muted' as const }]
              : undefined,
            trailing: (
              <ToggleSwitch
                checked={source.enabled}
                disabled={!isAdmin}
                onChange={(enabled) => onToggle(source.url, enabled)}
                aria-label={source.enabled ? t.tapp.disable : t.tapp.enable}
                title={source.enabled ? t.tapp.disable : t.tapp.enable}
              />
            ),
            actions:
              isAdmin && !source.official
                ? [
                    {
                      key: 'edit',
                      label: t.tapp.editSource,
                      icon: <FaEdit />,
                      variant: 'secondary' as const,
                      onClick: () => openEditForm(source),
                    },
                    {
                      key: 'delete',
                      label: t.tapp.deleteSource,
                      icon: <FaTrash />,
                      variant: 'danger' as const,
                      onClick: () =>
                        setPendingDelete({
                          url: source.url,
                          name: source.name,
                        }),
                    },
                  ]
                : undefined,
          }))}
          emptyText={t.tapp.storeSources}
        />
      </SettingGroup>

      <AnimatePresence initial={false}>
        {pendingDelete && (
          <motion.div
            key="store-delete-confirm"
            className="as-store-configuration__confirm"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="as-store-delete-source-title"
            initial={{ opacity: 0, y: 10 }}
            animate={{
              opacity: 1,
              y: 0,
              transition: { duration: 0.24, ease: [0.22, 1, 0.36, 1] },
            }}
            exit={{
              opacity: 0,
              y: 8,
              transition: { duration: 0.16, ease: [0.4, 0, 1, 1] },
            }}
          >
          <div className="as-store-configuration__confirm-card glass glass-liquid">
            <h4 id="as-store-delete-source-title" className="as-detail__h">
              {t.tapp.deleteSource}
            </h4>
            <p className="settings-text-3" style={{ margin: '0.35rem 0 0.85rem' }}>
              {t.tapp.confirmDeleteSource}
            </p>
            <p
              className="as-store-configuration__confirm-name"
              title={pendingDelete.name}
            >
              {pendingDelete.name}
            </p>
            <div className="as-store-configuration__form-actions">
              <SettingsButton
                variant="danger"
                size="sm"
                disabled={deleting}
                onClick={async () => {
                  try {
                    setDeleting(true)
                    await onRemove(pendingDelete.url)
                    setPendingDelete(null)
                  } finally {
                    setDeleting(false)
                  }
                }}
              >
                {deleting ? t.tapp.uninstalling : t.tapp.deleteSource}
              </SettingsButton>
              <SettingsButton
                variant="secondary"
                size="sm"
                disabled={deleting}
                onClick={() => setPendingDelete(null)}
              >
                {t.tapp.cancel}
              </SettingsButton>
            </div>
          </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}
