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
  // step - 1 会让生成链第一页后退到导入页。它们是分岔口的两条路，不是前后关系。
  assert.equal(previousOnboardingStep(GUIDED_FIRST_STEP), CHOICE_STEP)
  assert.equal(previousOnboardingStep(IMPORT_STEP), CHOICE_STEP)
  // 分岔口再往回就是离开引导页。
  assert.equal(previousOnboardingStep(CHOICE_STEP), null)
  // 生成链内部才是线性的。
  for (let step = GUIDED_FIRST_STEP + 1; step <= GUIDED_LAST_STEP; step += 1) {
    assert.equal(
      previousOnboardingStep(step as Parameters<typeof previousOnboardingStep>[0]),
      step - 1,
    )
  }
})

test('resume lands on the guided chain, never on the fork or the import lane', () => {
  // 恢复只会落到已经持久化的引导阶段。导入是入口，不是可恢复的进度：
  // 人设一存下来，下次进来就该接着引导链走。
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
  // 加了第 0 步之后还按 step - 1 取，导入页的标题会取到 undefined，
  // 而第 1 步会顶着导入的标题。
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
  // 去向锁在这里，「怎么去」（enterLane 的清场）由下面那条单独锁。
  assert.match(wizard, /onImport=\{\(\) => \w+\(IMPORT_STEP\)\}/)
  assert.match(wizard, /enterLane\(GUIDED_FIRST_STEP\)/)
  assert.match(wizard, /step === IMPORT_STEP && \(/)

  // 词条页曾经自己挂过一个「直接导入」，那个决定已经上移到分岔口。
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
  // upload_portrait 选文件时就落库了。这里再带 portraitAssetId 会把 Keep
  // 变成 Set/Clear，等于用前端的记忆去覆盖后端已经存下的那一张。
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
    '../../../i18n/zh-CN.ts',
    '../../../i18n/en-US.ts',
    '../../../i18n/ja-JP.ts',
    '../../../i18n/index.ts',
  ]
  for (const file of files) {
    const source = read(file)
    for (const key of keys) {
      assert.match(source, new RegExp(`\\b${key}\\b`), `${file} 缺 ${key}`)
    }
  }
})

test('generation and import are two flows, not one flow with import bolted on', () => {
  // 生成流程里再挂导入，就会出现「起草到一半贴一份进来」这种半成品状态：
  // 人设是导入的，视觉设定却是照着起草结果推的。分开之后每一条都自洽。
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

  // 反过来：导入流程必须两件都在，否则「直接导入人设和主视觉图」只做了一半。
  const importStep = read('./steps/ImportStep.tsx')
  assert.match(importStep, /PersonaImportPanel/)
  assert.match(importStep, /PortraitImportButton/)
})

test('no lane hijacks the back arrow away from the fork', () => {
  // 页面壳先问 header.onBack，再退到 previousOnboardingStep。所以一个步骤
  // 自己报 onBack 就等于盖掉那条判定。导入页曾经这么干过，里面调的是
  // onGuided —— 分岔口还不存在时那是对的，加了分岔口之后返回就跑去词条页了。
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

  // 换道走分岔口，导入页不再另挂一条去生成链的后悔按钮。
  const importStep = read('./steps/ImportStep.tsx')
  assert.doesNotMatch(importStep, /importBackToGuided|onGuided/)
})

test('finishing either lane hands off to the motion workbench', () => {
  // 后端的立绘流水线在出图之后还有分层 / 编译 / 激活，那一段在动作工作台。
  // 引导原本走完就退回上一层，站长不知道另一个子页存在的话，拿到的是一张
  // 不会动的立绘。
  const page = read('./PersonaOnboardingPage.tsx')
  assert.match(page, /onFinished: \(\) => void/)
  assert.match(page, /onFinished=\{onFinished\}/)
  assert.doesNotMatch(page, /onFinished=\{onBack\}/)

  const shell = read('../../config/AiConfigSection.tsx')
  assert.match(shell, /onFinished=\{\(\) => openAiSubpage\('merope'\)\}/)

  // 两条路的收尾按钮都得说清楚它把人送去哪，不然「完成」之后换了个页面
  // 会像是走错了。
  for (const locale of ['zh-CN', 'en-US', 'ja-JP']) {
    const copy = read(`../../../i18n/${locale}.ts`)
    const finishes = copy.match(/(portraitFinish|importFinish): '([^']+)'/g) ?? []
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
  // 流程条用各步骤自己的短名，不另写一份——改了步骤名这里跟着走，不会脱节。
  assert.match(choice, /o\.step1Short,[\s\S]*?o\.step5Short,/)
  assert.match(choice, /flow: \[o\.step3Short, o\.step5Short\]/)

  // 两张卡都要有各自的规模提示，否则「五步」和「一页」的差别看不出来。
  for (const key of ['choiceGuidedMeta', 'choiceImportMeta']) {
    assert.match(choice, new RegExp(`o\\.${key}`))
    for (const locale of ['zh-CN', 'en-US', 'ja-JP']) {
      assert.match(read(`../../../i18n/${locale}.ts`), new RegExp(`\\b${key}\\b`))
    }
  }
})

test('the fork cards stay clickable as a whole', () => {
  const choice = read('./steps/ChoiceStep.tsx')
  assert.match(choice, /<button[\s\S]*?className="merope-ob-choice__lane"/)

  // 整张卡是一个 button，所以卡「内部」只能放短语内容——<div>/<ol>/<p> 会让
  // HTML 失效。外层容器不在此列，所以只切 button 那一段来看。
  const inside = choice.slice(
    choice.indexOf('<button'),
    choice.indexOf('</button>'),
  )
  assert.ok(inside.length > 0, 'lane button not found')
  assert.doesNotMatch(inside, /<(div|ol|ul|li|p)[\s>]/)
})

test('switching lanes at the fork wipes what belongs to the other lane', () => {
  const wizard = read('./OnboardingWizard.tsx')
  // 分岔口的两个出口都必须过 enterLane，不能直接 onStepChange。
  assert.match(wizard, /enterLane\(GUIDED_FIRST_STEP\)/)
  assert.match(wizard, /onImport=\{\(\) => enterLane\(IMPORT_STEP\)\}/)

  const enter = wizard.slice(
    wizard.indexOf('const enterLane'),
    wizard.indexOf('const claimPersona'),
  )
  assert.ok(enter.length > 0, 'enterLane not found')
  // 起草出来的人设、起草闩、导入的立绘缩略图都归清场管。
  assert.match(enter, /invalidatePersonaAndVisual\(\)/)
  assert.match(enter, /setImportedPortraitUrl\(null\)/)
  // 名字和性别是身份，两条路都要，清场不该碰。
  assert.doesNotMatch(enter, /setDisplayName|setGender\b/)
})

test('the import lane edits identity without invalidating the imported persona', () => {
  const wizard = read('./OnboardingWizard.tsx')
  const importPane = wizard.slice(
    wizard.indexOf('<ImportStep'),
    wizard.indexOf('<TagBubblesStep'),
  )
  assert.ok(importPane.length > 0, 'import pane not found')
  // updateDisplayName / updateGender 里带着 invalidatePersonaAndVisual：
  // 在导入页的名字框敲一个字，刚导入的人设就没了。
  assert.match(importPane, /onDisplayName=\{setDisplayName\}/)
  assert.match(importPane, /onGender=\{setGender\}/)
  assert.doesNotMatch(importPane, /updateDisplayName|updateGender/)
  // 起草闩归生成链，导入不碰。
  assert.doesNotMatch(importPane, /claimedAuto\.current\.persona = true/)

  // 生成链那边仍然要用会作废草稿的那一套：改了起草输入，草稿就该作废。
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
  // 有主图时按图读出视觉特征；没有主图才写 null。词条和补充要求仍要显式清空，
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
  // 入口卡曾经用报告数挡住整个设定页，导入也被一起拦了。
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
  // 第 4 步的自动起草走它自己的 regenBusy，不是向导的 run()，所以起草在飞
  // 的时候后退不被拦：连按两次返回到分岔口再选导入，那份 AI 草稿会晚一步
  // 落进导入页。draftPersona 的回包按这个序号判断自己过没过期。
  assert.match(enter, /personaWriteSeq\.current \+= 1/)

  const draft = wizard.slice(
    wizard.indexOf('const draftPersona'),
    wizard.indexOf('const confirmedVisualProfile'),
  )
  assert.match(draft, /const seq = \+\+personaWriteSeq\.current/)
  assert.match(draft, /if \(seq !== personaWriteSeq\.current\) return/)
})
