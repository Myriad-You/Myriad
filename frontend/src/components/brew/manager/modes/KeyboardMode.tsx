import { useMemo } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { BREW_SHORTCUTS } from '../../../../hooks/useBrewKeyboard'
import {
  Sheet,
  SheetBody,
  SheetCount,
  SheetGroup,
  SheetGroupHead,
  SheetKbd,
  SheetKeys,
  SheetShortcut,
} from '../../ui/Sheet'

export function groupShortcuts<T extends { category: string }>(
  list: readonly T[],
) {
  const grouped = Object.groupBy(list, (item) => item.category)
  return {
    navigation: grouped.navigation ?? [],
    article: grouped.article ?? [],
    source: grouped.source ?? [],
    other: grouped.other ?? [],
  }
}

export function KeyboardMode() {
  const { t } = useI18n()
  const grouped = useMemo(() => groupShortcuts(BREW_SHORTCUTS), [])
  const labels = {
    navigation: t.brew.shortcutNavigation,
    article: t.brew.shortcutArticle,
    source: t.brew.shortcutSource,
    other: t.brew.shortcutOther,
  }

  return (
    <Sheet>
      <SheetBody>
        <SheetKeys>
          {Object.entries(grouped).map(([cat, shortcuts]) => {
            if (shortcuts.length === 0) return null
            return (
              <SheetGroup key={cat}>
                <SheetGroupHead>
                  <span>{labels[cat as keyof typeof labels]}</span>
                  <SheetCount>{shortcuts.length}</SheetCount>
                </SheetGroupHead>
                {shortcuts.map((shortcut) => (
                  <SheetShortcut key={shortcut.key}>
                    <span>{t.brew[shortcut.descriptionKey]}</span>
                    <SheetKbd>{shortcut.key.split(' / ')[0]}</SheetKbd>
                  </SheetShortcut>
                ))}
              </SheetGroup>
            )
          })}
        </SheetKeys>
      </SheetBody>
    </Sheet>
  )
}
