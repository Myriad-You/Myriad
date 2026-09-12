import { LuCheck as Check } from '@lib/icons'

import { cx } from './cx'

export function BrewPick({ on }: { on: boolean }) {
  return (
    <span className={cx('brew-pick', on && 'is-on')} aria-hidden>
      <Check />
    </span>
  )
}
