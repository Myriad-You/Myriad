import type { ClothingStyle } from '../../../components/agent/onboarding/onboardingTypes'
import type { TranslationKeys } from '../../../i18n'
import type { WardrobeItem } from '../persona/wardrobe'
import { wardrobeItemLabel } from '../persona/wardrobe'

type Format = (template: string, vars: Record<string, string | number>) => string

export function joinOverviewSentences(locale: string, parts: string[]): string {
  const cleaned = parts
    .map((part) => part.replaceAll(/[。．.]+$/gu, '').trim())
    .filter(Boolean)
  if (cleaned.length === 0) return ''
  if (locale.startsWith('en')) return `${cleaned.join('. ')}.`
  return `${cleaned.join('。')}。`
}

/** What the wardrobe holds and how much of it is ready to wear, in a few sentences. */
export function wardrobeOverview(
  rack: readonly WardrobeItem[],
  activeOutfitId: string | null,
  labels: TranslationKeys['merope'],
  styleNames: Record<ClothingStyle, string>,
  format: Format,
  locale: string,
): string {
  const wardrobeCount = rack.length
  if (wardrobeCount === 0) return labels.wardrobeEmpty
  const wearingItem = rack.find((item) => item.id === activeOutfitId)
  const wearingName = wearingItem
    ? wardrobeItemLabel(wearingItem, styleNames, labels.wardrobeDefault)
    : null
  const wardrobeSentences: string[] = []
  if (wearingName) {
    wardrobeSentences.push(
      format(labels.overviewWardrobeWearing, { name: wearingName }),
    )
  }
  if (wardrobeCount > 1) {
    wardrobeSentences.push(
      format(labels.overviewWardrobeCount, { n: wardrobeCount }),
    )
  }
  const portraitReady = rack.filter((item) => item.portraitAssetId).length
  const rigReady = rack.filter((item) => item.rigAssetId).length
  const allReady =
    portraitReady === wardrobeCount && rigReady === wardrobeCount
  if (!allReady) {
    if (wardrobeCount === 1) {
      if (portraitReady) {
        wardrobeSentences.push(labels.overviewWardrobeOnePortrait)
      } else if (rigReady) {
        wardrobeSentences.push(labels.overviewWardrobeOneRig)
      } else {
        wardrobeSentences.push(labels.overviewWardrobeNoneReady)
      }
    } else if (portraitReady === 0 && rigReady === 0) {
      wardrobeSentences.push(labels.overviewWardrobeNoneReady)
    } else if (portraitReady === wardrobeCount && rigReady === 0) {
      wardrobeSentences.push(labels.overviewWardrobeAllPortraits)
    } else if (rigReady === wardrobeCount && portraitReady === 0) {
      wardrobeSentences.push(labels.overviewWardrobeAllRigs)
    } else {
      wardrobeSentences.push(
        format(labels.overviewWardrobeMixed, {
          portrait: portraitReady,
          rig: rigReady,
        }),
      )
    }
  }
  return joinOverviewSentences(locale, wardrobeSentences)
}
