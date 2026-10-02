import { useI18n } from '../../../contexts/I18nContext'
import { FaceTabs } from './FaceTabs'

/** Switches the stage between the worn outfit's bust and its full figure. */
export function FullBodyStageSwitch({
  staging,
  onChange,
}: {
  staging: boolean
  onChange: (staging: boolean) => void
}) {
  const { t } = useI18n()
  const copy = t.merope.fullBody
  return (
    <FaceTabs
      ariaLabel={copy.stage}
      value={staging ? 'fullBody' : 'bust'}
      options={[
        { value: 'bust', label: copy.stageBust },
        { value: 'fullBody', label: copy.stageFullBody },
      ]}
      onChange={(value) => onChange(value === 'fullBody')}
    />
  )
}
