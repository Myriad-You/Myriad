import type { UpperBodyVisualIdentity } from '../../components/agent/onboarding/onboardingTypes'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import {
  applyOutfit,
  bindPortrait,
  DEFAULT_WARDROBE_ID,
  hydrateWardrobe,
  isDefaultWardrobeItem,
  parseWardrobe,
  parseWardrobeName,
  seedWardrobeFromIdentity,
  sortWardrobe,
  stampPortrait,
  syncActiveOutfit,
  wardrobeItemLabel,
  withCharacter,
  writeOutfit,
} from './wardrobe'

const outfitA = {
  upperBodySilhouette: '窄肩与清晰领口',
  outfitConstruction: '水手领内搭叠短外套',
  sleeveArmDesign: '宽松袖口包住局部前臂',
  materialPlan: '哑光布料为主',
  heroAccessory: '左侧星形发夹',
  paletteHint: '淡紫与白为主体',
  motif: '星轨集中在发饰',
}

const outfitB = {
  ...outfitA,
  outfitConstruction: '敞开领口内搭叠短风衣',
}

const identity: UpperBodyVisualIdentity = {
  character: {
    faceDesign: '女性化鹅蛋脸',
    eyeDesign: '中等偏大的紫色眼睛',
    hairShape: '银灰齐颌短发',
    hairLayerPlan: '后发、刘海、侧发',
  },
  outfit: outfitA,
}

test('wardrobe parse drops broken items and keeps valid ones', () => {
  const items = parseWardrobe([
    { id: 'w-a', clothingStyle: 'urban', outfit: outfitA },
    { id: 'w-a', clothingStyle: 'idol', outfit: outfitB },
    { id: 'w-b', clothingStyle: 'not-a-style', outfit: outfitB },
    { id: 'w-c', clothingStyle: 'idol', outfit: outfitB },
  ])
  assert.equal(items.length, 2)
  assert.equal(items[0].id, 'w-a')
  assert.equal(items[1].id, 'w-c')
  assert.equal(items[1].clothingStyle, 'idol')
})

test('hydrate seeds the master portrait as the default outfit', () => {
  const seeded = hydrateWardrobe(
    { gender: 'female', clothingStyle: 'urban', visualIdentity: identity },
    identity,
    '/uploads/face.png',
  )
  assert.equal(seeded.items.length, 1)
  assert.equal(seeded.items[0].id, DEFAULT_WARDROBE_ID)
  assert.equal(seeded.items[0].clothingStyle, 'urban')
  assert.equal(seeded.items[0].portraitAssetId, '/uploads/face.png')
  assert.equal(seeded.activeId, DEFAULT_WARDROBE_ID)
  assert.equal(seeded.items[0].name, undefined)

  const stored = hydrateWardrobe(
    {
      clothingStyle: 'idol',
      wardrobe: [
        { id: 'w-a', clothingStyle: 'urban', outfit: outfitA },
        { id: 'w-b', clothingStyle: 'idol', outfit: outfitB },
      ],
      activeOutfitId: 'w-b',
    },
    { ...identity, outfit: outfitB },
  )
  assert.equal(stored.activeId, 'w-b')
  assert.equal(stored.items.length, 2)
  assert.equal(stored.items[0].id, DEFAULT_WARDROBE_ID)
  assert.equal(stored.items[1].id, 'w-b')
  assert.equal(stored.items[0].clothingStyle, 'urban')
})

test('character edits keep the worn outfit, and outfit writes stay on one set', () => {
  const next = withCharacter(identity, {
    ...identity.character,
    hairShape: '银灰高马尾',
  })
  assert.equal(next.character.hairShape, '银灰高马尾')
  assert.equal(next.outfit.outfitConstruction, outfitA.outfitConstruction)
  const items = [
    { id: 'w-a', clothingStyle: 'urban' as const, outfit: outfitA },
    { id: 'w-b', clothingStyle: 'idol' as const, outfit: outfitB },
  ]
  const written = writeOutfit(items, 'w-b', outfitA)
  assert.equal(written[0].outfit.outfitConstruction, outfitA.outfitConstruction)
  assert.equal(written[1].outfit.outfitConstruction, outfitA.outfitConstruction)
  assert.equal(items[1].outfit.outfitConstruction, outfitB.outfitConstruction)
})

test('apply swaps only the outfit module', () => {
  const next = applyOutfit(identity, {
    id: 'w-b',
    clothingStyle: 'idol',
    outfit: outfitB,
  })
  assert.deepEqual(next.character, identity.character)
  assert.equal(next.outfit.outfitConstruction, outfitB.outfitConstruction)
})

test('editing the live identity writes back into the active wardrobe item', () => {
  const items = [
    { id: 'w-a', clothingStyle: 'urban' as const, outfit: outfitA },
  ]
  const edited = { ...identity, outfit: outfitB }
  const synced = syncActiveOutfit(items, 'w-a', edited)
  assert.equal(synced[0].outfit.outfitConstruction, outfitB.outfitConstruction)
})

test('an observed identity becomes the default wardrobe set', () => {
  const seeded = seedWardrobeFromIdentity(
    identity,
    'urban',
    '/uploads/face.png',
  )
  assert.equal(seeded.items.length, 1)
  assert.equal(seeded.items[0].id, DEFAULT_WARDROBE_ID)
  assert.equal(seeded.items[0].clothingStyle, 'urban')
  assert.equal(seeded.items[0].portraitAssetId, '/uploads/face.png')
  assert.equal(seeded.activeId, DEFAULT_WARDROBE_ID)
})

test('the default outfit cannot be renamed and stays first', () => {
  const items = parseWardrobe([
    {
      id: DEFAULT_WARDROBE_ID,
      clothingStyle: 'urban',
      outfit: outfitA,
      name: '想改的名字',
    },
    {
      id: 'w-b',
      clothingStyle: 'everyday',
      outfit: outfitB,
      name: '春装',
    },
  ])
  assert.equal(items[0].name, undefined)
  assert.equal(items[1].name, '春装')
  assert.equal(
    wardrobeItemLabel(items[0], { urban: '都市' } as Parameters<
      typeof wardrobeItemLabel
    >[1], '默认服装'),
    '默认服装',
  )
  assert.ok(isDefaultWardrobeItem(items[0]))
  assert.deepEqual(
    sortWardrobe([
      { id: 'w-b', clothingStyle: 'everyday', outfit: outfitB },
      {
        id: DEFAULT_WARDROBE_ID,
        clothingStyle: 'idol',
        outfit: outfitA,
      },
    ]).map((item) => item.id),
    [DEFAULT_WARDROBE_ID, 'w-b'],
  )
})

test('a custom name is kept and used as the label', () => {
  const items = parseWardrobe([
    {
      id: 'w-a',
      clothingStyle: 'urban',
      outfit: outfitA,
      name: '  冬日大衣  ',
    },
    {
      id: 'w-b',
      clothingStyle: 'idol',
      outfit: outfitB,
      name: '',
    },
  ])
  assert.equal(items[0].name, '冬日大衣')
  assert.equal(items[1].name, undefined)
  assert.equal(parseWardrobeName('  冬日大衣  '), '冬日大衣')
  const styles = { urban: '都市' } as Parameters<typeof wardrobeItemLabel>[1]
  assert.equal(wardrobeItemLabel(items[0], styles), '冬日大衣')
  assert.equal(wardrobeItemLabel({ clothingStyle: 'urban' }, styles), '都市')
})

test('the master portrait is the outfit, not a separate hanger', () => {
  const items = parseWardrobe([
    {
      id: 'w-a',
      clothingStyle: 'urban',
      outfit: outfitA,
      portraitAssetId: '/uploads/urban.png',
    },
    {
      id: 'w-b',
      clothingStyle: 'idol',
      outfit: outfitB,
      portraitAssetId: 'https://cdn.example.com/x.png',
    },
  ])
  assert.equal(items[0].portraitAssetId, '/uploads/urban.png')
  assert.equal(items[1].portraitAssetId, undefined)

  const bound = bindPortrait(
    [{ id: 'w-a', clothingStyle: 'urban', outfit: outfitA }],
    'w-a',
    '/uploads/face.png',
  )
  assert.equal(bound[0].portraitAssetId, '/uploads/face.png')
  assert.equal(bindPortrait(bound, 'w-a', '/uploads/other.png'), bound)
  const stamped = stampPortrait(
    [
      {
        id: 'w-a',
        clothingStyle: 'urban',
        outfit: outfitA,
        portraitAssetId: '/uploads/face.png',
        rigAssetId:
          'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      },
    ],
    'w-a',
    '/uploads/next.png',
  )
  assert.equal(stamped[0].portraitAssetId, '/uploads/next.png')
  assert.equal(stamped[0].rigAssetId, undefined)
  assert.equal(stamped[0].generationFingerprint, undefined)
  assert.equal(
    stampPortrait(
      [
        {
          id: 'w-a',
          clothingStyle: 'urban',
          outfit: outfitA,
          portraitAssetId: '/uploads/old.png',
        },
      ],
      'w-a',
      '/uploads/next.png',
      'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
    )[0].generationFingerprint,
    'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
  )
  assert.equal(
    stampPortrait(
      [
        {
          id: 'w-a',
          clothingStyle: 'urban',
          outfit: outfitA,
          portraitAssetId: '/uploads/face.png',
          rigAssetId:
            'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
        },
      ],
      'w-a',
      '/uploads/face.png',
    )[0].rigAssetId,
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
  )

  const withRig = parseWardrobe([
    {
      id: 'w-a',
      clothingStyle: 'urban',
      outfit: outfitA,
      rigAssetId:
        'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB',
    },
    {
      id: 'w-b',
      clothingStyle: 'idol',
      outfit: outfitB,
      rigAssetId: 'nope',
    },
  ])
  assert.equal(
    withRig[0].rigAssetId,
    'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
  )
  assert.equal(withRig[1].rigAssetId, undefined)

  const closet = [
    {
      id: DEFAULT_WARDROBE_ID,
      clothingStyle: 'urban' as const,
      outfit: outfitA,
      portraitAssetId: '/uploads/face.png',
    },
    { id: 'w-b', clothingStyle: 'idol' as const, outfit: outfitB },
  ]
  assert.equal(
    bindPortrait(closet, 'w-b', '/uploads/face.png')[1].portraitAssetId,
    undefined,
  )
  assert.equal(
    bindPortrait(closet, DEFAULT_WARDROBE_ID, '/uploads/face.png'),
    closet,
  )
  const leftover = [
    { id: DEFAULT_WARDROBE_ID, clothingStyle: 'urban' as const, outfit: outfitA },
    { id: 'w-b', clothingStyle: 'idol' as const, outfit: outfitB },
  ]
  assert.equal(
    bindPortrait(leftover, DEFAULT_WARDROBE_ID, '/uploads/idol.png')[0]
      .portraitAssetId,
    undefined,
  )
  assert.equal(
    bindPortrait(leftover, 'w-b', '/uploads/idol.png')[1].portraitAssetId,
    undefined,
  )
})

test('wardrobe sets are ordered by clothing family', () => {
  const ordered = sortWardrobe([
    { id: 'w-2', clothingStyle: 'idol', outfit: outfitA },
    { id: 'w-1', clothingStyle: 'everyday', outfit: outfitA },
    { id: 'w-3', clothingStyle: 'urban', outfit: outfitA },
  ])
  assert.deepEqual(
    ordered.map((item) => item.clothingStyle),
    ['everyday', 'urban', 'idol'],
  )
})

test('the wardrobe lives in settings, not in the onboarding wizard', () => {
  const workbench = readFileSync(
    new URL('./SiteMotionWorkbench.tsx', import.meta.url),
    'utf8',
  )
  const wizard = readFileSync(
    new URL(
      '../../components/agent/onboarding/OnboardingWizard.tsx',
      import.meta.url,
    ),
    'utf8',
  )
  const tabs = readFileSync(
    new URL('./anime25drig/Anime25DWorkbench.tsx', import.meta.url),
    'utf8',
  )
  const closet = readFileSync(
    new URL('./OutfitWardrobe.tsx', import.meta.url),
    'utf8',
  )
  assert.match(workbench, /OutfitWardrobe/)
  assert.match(workbench, /show="outfit"/)
  assert.match(workbench, /visualIdentityView\('character'\)/)
  assert.match(workbench, /saveCharacter/)
  assert.match(workbench, /saveOutfitDesign/)
  assert.match(workbench, /withCharacter/)
  assert.match(workbench, /writeOutfit/)
  assert.match(workbench, /fillVisualFromPortrait/)
  assert.match(workbench, /observeVisualFromPortrait/)
  assert.match(workbench, /applyVisualFromPortrait\(url\)/)
  assert.match(workbench, /character: visualIdentity.character/)
  assert.match(workbench, /outfit: observedIdentity.outfit/)
  assert.match(workbench, /activeId: activeOutfitId/)
  assert.match(workbench, /generationFingerprint/)
  assert.doesNotMatch(workbench, /onCreated=\{async \(item, nextIdentity\)/)
  assert.match(workbench, /outfitLead=\{outfitCard\}/)
  assert.match(workbench, /outfitRig=\{wearingManaged\}/)
  assert.match(workbench, /wardrobeWear/)
  assert.match(workbench, /wearOutfit\(managingOutfit\)/)
  assert.match(workbench, /generatePortrait\(managingOutfit\)/)
  assert.match(workbench, /wearingManaged \|\| !outfitPicture/)
  assert.match(workbench, /portraitAssetId: nextItem.portraitAssetId \?\? null/)
  assert.doesNotMatch(
    workbench,
    /bindPortrait\(current, activeOutfitId, portraitUrl\)/,
  )
  assert.doesNotMatch(
    workbench,
    /if \(item.id !== activeOutfitId\) await wearOutfit\(item\)/,
  )
  assert.match(workbench, /section-header-back/)
  assert.match(workbench, /merope-wardrobe-page__portrait/)
  assert.doesNotMatch(wizard, /OutfitWardrobe/)
  assert.doesNotMatch(closet, /merope-wardrobe__hanger/)
  assert.doesNotMatch(closet, /merope-wardrobe__rail/)
  assert.match(closet, /currentPicture/)
  assert.match(closet, /isDefaultWardrobeItem\(current\) \? portraitUrl/)
  assert.match(closet, /merope-wardrobe__garment--empty/)
  assert.match(closet, /onManage/)
  assert.match(closet, /isDefaultWardrobeItem/)
  assert.match(workbench, /isDefaultWardrobeItem/)
  assert.match(workbench, /wardrobeDefault/)
  assert.match(workbench, /commitRigPsdAsset/)
  assert.match(workbench, /agentService.getPersona/)
  assert.match(tabs, /value: 'wardrobe'/)
  assert.match(tabs, /outfitLead/)
  assert.match(tabs, /outfitRig/)
  assert.doesNotMatch(tabs, /onOutfitBack/)
  assert.doesNotMatch(tabs, /value: 'portrait'/)
  assert.doesNotMatch(tabs, /value: 'rig'/)
})
