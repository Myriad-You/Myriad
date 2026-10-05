import { useEffect, useState } from 'react'
import { InputItem } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { userFacingError } from '../../../utils/userFacingError'
import { getSeeThroughStatus, updateSeeThroughSpace } from '../api'

/**
 * Which See-through Space splits portraits: the public demo (left empty) or
 * the owner's copy. A copy serving the canvas endpoint splits at the
 * portrait's own proportions; the backend asks the Space each time.
 */
export function SeeThroughSpaceField() {
  const { t } = useI18n()
  const labels = t.merope
  const [space, setSpace] = useState('')
  const [defaultSpace, setDefaultSpace] = useState('')
  const [error, setError] = useState<string | undefined>()

  useEffect(() => {
    let cancelled = false
    void getSeeThroughStatus()
      .then((status) => {
        if (cancelled) return
        setDefaultSpace(status.defaultSpace)
        setSpace(status.space === status.defaultSpace ? '' : status.space)
      })
      .catch(() => {
        // The token field above reports a status that cannot be read.
      })
    return () => {
      cancelled = true
    }
  }, [])

  const save = async (next: string) => {
    setError(undefined)
    try {
      const status = await updateSeeThroughSpace(next)
      setSpace(status.space === status.defaultSpace ? '' : status.space)
    } catch (reason) {
      setError(userFacingError(reason, labels.motionSeeThroughSpaceFailed))
      throw reason
    }
  }

  return (
    <InputItem
      itemKey="see-through-space"
      label={labels.motionSeeThroughSpace}
      description={labels.motionSeeThroughSpaceDescription}
      value={space}
      onChange={(value) => {
        setSpace(value)
        setError(undefined)
      }}
      autoComplete="off"
      placeholder={defaultSpace || 'owner/name'}
      variant="clickToEdit"
      emptyLabel={`${labels.motionSeeThroughSpaceDefault}${defaultSpace ? ` · ${defaultSpace}` : ''}`}
      editLabel={labels.motionSeeThroughSpaceEdit}
      saveLabel={labels.motionSeeThroughSpaceSave}
      cancelLabel={labels.motionSeeThroughSpaceCancel}
      onCommit={save}
      error={error}
      clearable
    />
  )
}
