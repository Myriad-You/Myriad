import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import {
  CHOICE_STEP,
  completedPersonaResumeStep,
  GUIDED_FIRST_STEP,
  GUIDED_LAST_STEP,
  IMPORT_STEP,
  previousOnboardingStep,
} from './onboardingTypes'

function read(name: string): string {
  return readFileSync(new URL(name, import.meta.url), 'utf8')
}

test('the fork comes first, then two lanes that never merge', () => {
  assert.equal(CHOICE_STEP, 0)
  assert.equal(IMPORT_STEP, 1)
  assert.equal(GUIDED_FIRST_STEP, 2)
  assert.equal(GUIDED_LAST_STEP, 6)
})

test('back from either lane returns to the fork, not to the other lane', () => {
  // 两条路从分岔口分出，不是前后关系；step-1 会把生成链第一页退到导入页。
  assert.equal(previousOnboardingStep(GUIDED_FIRST_STEP), CHOICE_STEP)
  assert.equal(previousOnboardingStep(IMPORT_STEP), CHOICE_STEP)
  assert.equal(previousOnboardingStep(CHOICE_STEP), null)
  for (let step = GUIDED_FIRST_STEP + 1; step <= GUIDED_LAST_STEP; step += 1) {
    assert.equal(
      previousOnboardingStep(step as Parameters<typeof previousOnboardingStep>[0]),
      step - 1,
    )
  }
})

test('resume lands on the guided chain, never on the fork or the import lane', () => {
  // 恢复只落到已持久化的引导阶段。导入是入口，不是可恢复的进度。
  for (const profile of [
    null,
    {},
    { gender: 'female' },
    { gender: 'female', visualIdentity: {} },
  ]) {
    const resume = completedPersonaResumeStep(profile)
    assert.notEqual(resume, CHOICE_STEP)
    assert.notEqual(resume, IMPORT_STEP)
    assert.ok(resume >= GUIDED_FIRST_STEP && resume <= GUIDED_LAST_STEP)
  }
})

test('title and lead arrays are indexed by step, not step - 1', () => {
  // 标题数组按 step 索引；step-1 会让导入页拿到 undefined。
  for (const name of ['PersonaOnboardingPage.tsx', 'OnboardingWizard.tsx']) {
    const source = read(`./${name}`)
    assert.match(source, /o\.choiceTitle,\n\s*o\.importTitle,\n\s*o\.step1Title,/)
    assert.doesNotMatch(
      source,
      /\]\[\s*step - 1\s*\]/,
      `${name} 仍在按 step - 1 取标题`,
    )
  }
})

test('the fork owns the choice, and no later step re-offers it', () => {
  const wizard = read('./OnboardingWizard.tsx')
  assert.match(wizard, /step === CHOICE_STEP && \(/)
  // 去向锁在这里；enterLane 清场由下面那条单独锁。
  assert.match(wizard, /onImport=\{\(\) => \w+\(IMPORT_STEP\)\}/)
  assert.match(wizard, /enterLane\(GUIDED_FIRST_STEP\)/)
  assert.match(wizard, /step === IMPORT_STEP && \(/)

  // 「直接导入」只在分岔口，词条页不得再挂。
  const tags = read('./steps/TagBubblesStep.tsx')
  assert.doesNotMatch(tags, /onImport|importEnter/)
})

test('import saves the persona without touching the uploaded portrait', () => {
  const wizard = read('./OnboardingWizard.tsx')
  const importPane = wizard.slice(
    wizard.indexOf('step === IMPORT_STEP'),
    wizard.indexOf('{step === 1 &&'),
  )
  assert.ok(importPane.length > 0, 'import pane not found')
  assert.match(importPane, /agentService\.putPersona\(/)
  // 注释里会提到这个字段名，断言的是真写进去的载荷。
  const code = importPane
    .split('\n')
    .filter((line) => !line.trim().startsWith('//'))
    .join('\n')
  // upload_portrait 选文件时就落库。再带 portraitAssetId 会把 Keep 变成 Set/Clear。
  assert.doesNotMatch(code, /portraitAssetId/)
  assert.match(importPane, /visualProfile: \{/)
  assert.match(importPane, /onFinished\(\)/)
})

test('every locale carries the import copy', () => {
  const keys = [
    'choiceTitle',
    'choiceLead',
    'choiceIntroTitle',
    'choiceIntroSub',
    'choiceShort',
    'choiceGuidedTitle',
    'choiceGuidedDetail',
    'choiceImportTitle',
    'choiceImportDetail',
    'importShort',
    'importTitle',
    'importLead',
    'importPersonaReady',
    'importPortraitHint',
    'importPortraitReplace',
    'importVisualFailed',
    'importFinish',
  ]
  const files = [
    '../../../i18n/zh-CN.json',
    '../../../i18n/en-US.json',
    '../../../i18n/ja-JP.json',
  ]
  for (const file of files) {
    const source = read(file)
    for (const key of keys) {
      assert.match(
        source,
        new RegExp(`\\b${RegExp.escape(key)}\\b`),
        `${file} 缺 ${key}`,
      )
    }
  }
})

test('generation and import are two flows, not one flow with import bolted on', () => {
  // 生成步骤不得挂导入件，否则人设是导入的、视觉却按起草推。
  const generationSteps = [
    './steps/TagBubblesStep.tsx',
    './steps/BasicsStep.tsx',
    './steps/PersonaEditStep.tsx',
    './steps/CharacterVisualDesignStep.tsx',
    './steps/MasterPortraitStep.tsx',
  ]
  for (const step of generationSteps) {
    const source = read(step)
    assert.doesNotMatch(
      source,
      /PersonaImportPanel|PortraitImportButton/,
      `${step} 属于生成流程，导入件应当只在导入流程里`,
    )
  }

  // 导入流程必须两件都在。
  const importStep = read('./steps/ImportStep.tsx')
  assert.match(importStep, /PersonaImportPanel/)
  assert.match(importStep, /PortraitImportButton/)
})

test('no lane hijacks the back arrow away from the fork', () => {
  // 壳先问 header.onBack，再 previousOnboardingStep。步骤自己报 onBack 会盖掉。
  const page = read('./PersonaOnboardingPage.tsx')
  assert.match(page, /if \(header\.onBack\?\.\(\)\) return/)

  for (const lane of ['./steps/ImportStep.tsx', './steps/ChoiceStep.tsx']) {
    const source = read(lane)
    assert.doesNotMatch(
      source,
      /onBack:/,
      `${lane} 不该自己接后退，上一页由 previousOnboardingStep 说了算`,
    )
  }

  // 换道走分岔口；导入页不另挂去生成链的按钮。
  const importStep = read('./steps/ImportStep.tsx')
  assert.doesNotMatch(importStep, /importBackToGuided|onGuided/)
})

test('finishing either lane hands off to the motion workbench', () => {
  // 出图之后还有分层/编译/激活，在动作工作台。收尾必须送去那里，不能退回上一层。
  const page = read('./PersonaOnboardingPage.tsx')
  assert.match(page, /onFinished: \(\) => void/)
  assert.match(page, /onFinished=\{onFinished\}/)
  assert.doesNotMatch(page, /onFinished=\{onBack\}/)

  const shell = read('../../config/AiConfigSection.tsx')
  assert.match(shell, /onFinished=\{\(\) => openAiSubpage\('merope'\)\}/)

  // 两条路的收尾按钮都得说清楚送去哪。
  for (const locale of [
    'zh-CN',
    'zh-TW',
    'en-US',
    'ja-JP',
    'ko-KR',
    'fr-FR',
    'de-DE',
  ]) {
    const copy = read(`../../../i18n/${locale}.json`)
    const finishes =
      copy.match(/"(portraitFinish|importFinish)": "([^"]+)"/g) ?? []
    assert.equal(finishes.length, 2, `${locale} 缺收尾按钮文案`)
    for (const line of finishes) {
      assert.doesNotMatch(
        line,
        /完成设定|完成导入|Finish setup|Finish import|設定を完了|取り込みを完了/,
        `${locale} 的收尾按钮仍写着「完成」，但它其实是去下一段`,
      )
    }
  }
})

test('the fork shows what each lane actually produces', () => {
  const choice = read('./steps/ChoiceStep.tsx')
  // 流程条用各步骤自己的短名，不另写一份。
  assert.match(choice, /o\.step1Short,[\s\S]*?o\.step5Short,/)
  assert.match(choice, /flow: \[o\.step3Short, o\.step5Short\]/)

  // 两张卡都要有各自的规模提示。
  for (const key of ['choiceGuidedMeta', 'choiceImportMeta']) {
    assert.match(choice, new RegExp(`o\\.${RegExp.escape(key)}`))
    for (const locale of [
      'zh-CN',
      'zh-TW',
      'en-US',
      'ja-JP',
      'ko-KR',
      'fr-FR',
      'de-DE',
    ]) {
      assert.match(read(`../../../i18n/${locale}.json`), new RegExp(`\\b${RegExp.escape(key)}\\b`))
    }
  }
})

test('the fork cards stay clickable as a whole', () => {
  const choice = read('./steps/ChoiceStep.tsx')
  assert.match(choice, /<button[\s\S]*?className="merope-ob-choice__lane"/)

  // 整张卡是 button，内部只能放短语；div/ol/p 会让 HTML 失效。只切 button 那段。
  const inside = choice.slice(
    choice.indexOf('<button'),
    choice.indexOf('</button>'),
  )
  assert.ok(inside.length > 0, 'lane button not found')
  assert.doesNotMatch(inside, /<(div|ol|ul|li|p)[\s>]/)
})

test('switching lanes at the fork wipes what belongs to the other lane', () => {
  const wizard = read('./OnboardingWizard.tsx')
  // 分岔口两个出口都必须过 enterLane，不能直接 onStepChange。
  assert.match(wizard, /enterLane\(GUIDED_FIRST_STEP\)/)
  assert.match(wizard, /onImport=\{\(\) => enterLane\(IMPORT_STEP\)\}/)

  const enter = wizard.slice(
    wizard.indexOf('const enterLane'),
    wizard.indexOf('const claimPersona'),
  )
  assert.ok(enter.length > 0, 'enterLane not found')
  // 起草人设、起草闩、导入立绘缩略图都归清场。名字和性别是身份，清场不该碰。
  assert.match(enter, /invalidatePersonaAndVisual\(\)/)
  assert.match(enter, /setImportedPortraitUrl\(null\)/)
  assert.doesNotMatch(enter, /setDisplayName|setGender\b/)
})

test('the import lane edits identity without invalidating the imported persona', () => {
  const wizard = read('./OnboardingWizard.tsx')
  const importPane = wizard.slice(
    wizard.indexOf('<ImportStep'),
    wizard.indexOf('<TagBubblesStep'),
  )
  assert.ok(importPane.length > 0, 'import pane not found')
  // updateDisplayName / updateGender 会 invalidatePersonaAndVisual，导入页不能用。
  assert.match(importPane, /onDisplayName=\{setDisplayName\}/)
  assert.match(importPane, /onGender=\{setGender\}/)
  assert.doesNotMatch(importPane, /updateDisplayName|updateGender/)
  // 起草闩归生成链，导入不碰。
  assert.doesNotMatch(importPane, /claimedAuto\.current\.persona = true/)

  // 生成链仍用会作废草稿的那一套。
  const basics = wizard.slice(
    wizard.indexOf('<BasicsStep'),
    wizard.indexOf('<PersonaEditStep'),
  )
  assert.match(basics, /onDisplayName=\{updateDisplayName\}/)
  assert.match(basics, /onGender=\{updateGender\}/)
})

test('the import lane does not inherit the guided run visual profile', () => {
  const wizard = read('./OnboardingWizard.tsx')
  const importPane = wizard.slice(
    wizard.indexOf('<ImportStep'),
    wizard.indexOf('<TagBubblesStep'),
  )
  // 有主图按图读视觉特征；没有才写 null。词条和补充要求仍要显式清空，
  // 不然 merge_visual_profile 会把上一次生成链的值补回来。
  assert.match(importPane, /observeVisualFromPortrait/)
  assert.match(importPane, /parseUpperBodyVisualIdentity/)
  assert.match(importPane, /seedWardrobeFromIdentity/)
  assert.match(importPane, /importedPortraitUrl/)
  assert.match(importPane, /visualIdentity: observedIdentity/)
  assert.match(importPane, /clothingStyle: observedStyle/)
  assert.match(importPane, /wardrobe: seeded\.items/)
  assert.match(importPane, /sourceTags: \[\]/)
  assert.match(importPane, /personaExtraRequirements: ''/)
})

test('missing reports lock generate, not import', () => {
  // 缺报告只锁生成，不锁导入。
  const card = read('../../config/AiConfigSection.tsx')
  assert.match(card, /openAiSubpage\('merope-setup'\)/)
  assert.doesNotMatch(card, /reportCount >= 3/)

  const choice = read('./steps/ChoiceStep.tsx')
  assert.match(choice, /GUIDED_MIN_REPORTS/)
  assert.match(choice, /disabled=\{Boolean\(lane\.locked\)\}/)
  assert.match(choice, /t\.config\.agentPersonaNeedsReports/)

  const wizard = read('./OnboardingWizard.tsx')
  assert.match(wizard, /if \(reportCount < GUIDED_MIN_REPORTS\) return/)
  assert.match(wizard, /onImport=\{\(\) => enterLane\(IMPORT_STEP\)\}/)
})

test('entering a lane cancels a draft that is still in flight', () => {
  const wizard = read('./OnboardingWizard.tsx')
  const enter = wizard.slice(
    wizard.indexOf('const enterLane'),
    wizard.indexOf('const claimPersona'),
  )
  // 自动起草走 regenBusy，后退拦不住。enterLane 必须 bump personaWriteSeq，
  // 否则草稿会晚一步写进导入页。
  assert.match(enter, /personaWriteSeq\.current \+= 1/)

  const draft = wizard.slice(
    wizard.indexOf('const draftPersona'),
    wizard.indexOf('const confirmedVisualProfile'),
  )
  assert.match(draft, /const seq = \+\+personaWriteSeq\.current/)
  assert.match(draft, /if \(seq !== personaWriteSeq\.current\) return/)
})
