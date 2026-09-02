import type { AgentMessageStep } from './agentThinking'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import {
  BUBBLE_GROW_TAU,
  BUBBLE_SHRINK_TAU,
  formatStepDuration,
  messageHasAnswer,
  messageShowsThinking,
  nonemptyContent,
  peelThoughtFromContent,
  splitThinkContent,
  stepsWorthShowing,
  summarizeAgentSteps,
  THINKING_FOLD_MS,
  thinkingVisible,
} from './agentThinking'

function step(overrides: Partial<AgentMessageStep> = {}): AgentMessageStep {
  return {
    id: 's1',
    name: '查天气',
    status: 'done',
    ...overrides,
  }
}

test('报出卡在哪一步、跑完几步、一共多久', () => {
  const summary = summarizeAgentSteps([
    step({ id: 's1', status: 'done', durationMs: 400 }),
    step({ id: 's2', name: '整理结果', status: 'running' }),
    step({ id: 's3', name: '写回复', status: 'pending' }),
  ])
  assert.deepEqual(summary, {
    running: '整理结果',
    done: 1,
    total: 3,
    elapsedMs: 400,
    failed: false,
  })
})

test('全跑完之后没有「正在跑」的那一步', () => {
  const summary = summarizeAgentSteps([
    step({ id: 's1', status: 'done', durationMs: 400 }),
    step({ id: 's2', status: 'done', durationMs: 1_100 }),
  ])
  assert.equal(summary.running, null)
  assert.equal(summary.done, 2)
  assert.equal(summary.elapsedMs, 1_500)
})

test('失败的那步也算跑完，但要把失败标出来', () => {
  const summary = summarizeAgentSteps([
    step({ id: 's1', status: 'done', durationMs: 100 }),
    step({ id: 's2', status: 'error', durationMs: 50 }),
  ])
  assert.equal(summary.done, 2)
  assert.equal(summary.failed, true)
})

test('一步都没报过时长就不显示时长 —— 0.0s 看着像瞬间完成，其实是没数据', () => {
  const summary = summarizeAgentSteps([
    step({ status: 'running', durationMs: undefined }),
  ])
  assert.equal(summary.elapsedMs, null)
})

test('没有步骤时不炸，也不编数字', () => {
  assert.deepEqual(summarizeAgentSteps([]), {
    running: null,
    done: 0,
    total: 0,
    elapsedMs: null,
    failed: false,
  })
})

test('只有一步且已经跑完就不摆过程 —— 正文本身就是结果', () => {
  assert.equal(stepsWorthShowing([]), false)
  assert.equal(stepsWorthShowing([step({ status: 'done' })]), false)
  // 但还在跑的时候要说一声，否则等待期间界面上什么都没有
  assert.equal(stepsWorthShowing([step({ status: 'running' })]), true)
  // 失败的单步更要摆出来 —— 哪一步炸的比「失败了」这个结论有用
  assert.equal(stepsWorthShowing([step({ status: 'error' })]), true)
  assert.equal(
    stepsWorthShowing([step({ id: 's1' }), step({ id: 's2' })]),
    true,
  )
})

test('还在跑时即使没有步骤也要占一行', () => {
  assert.equal(thinkingVisible([], true), true)
  assert.equal(thinkingVisible([], false), false)
  assert.equal(thinkingVisible([step({ status: 'done' })], false), false)
  assert.equal(thinkingVisible([step({ status: 'done' })], true), true)
})

test('有判断说明、但还没有正文时才摆过程', () => {
  assert.equal(thinkingVisible([], false, '先查天气再写'), true)
  assert.equal(thinkingVisible([], false, '  '), false)
  assert.equal(thinkingVisible([], true, '先查天气再写', true), false)
  assert.equal(thinkingVisible([], false, '先查天气再写', true), false)
})

test('聊天档不画思考，做事档流式时要占一行', () => {
  assert.equal(
    messageShowsThinking({
      role: 'assistant',
      hasAnswer: false,
      streaming: true,
      hasProcess: true,
      hideThinking: true,
    }),
    false,
  )
  assert.equal(
    messageShowsThinking({
      role: 'assistant',
      hasAnswer: false,
      streaming: true,
      hasProcess: false,
    }),
    true,
  )
  assert.equal(
    messageShowsThinking({
      role: 'user',
      hasAnswer: false,
      streaming: false,
      hasProcess: true,
    }),
    false,
  )
})

test('只有空白不算正文，否则思考会被卸掉、气泡里剩一圈空垫', () => {
  assert.equal(nonemptyContent('\n\n'), '')
  assert.equal(nonemptyContent('  你好'), '  你好')
  assert.equal(messageHasAnswer({ content: '\n' }), false)
  assert.equal(messageHasAnswer({ content: '答' }), true)
  assert.equal(messageHasAnswer({ content: '', imageUrls: ['/a.png'] }), true)
})

test('思考过程本文要折行，不能被步骤名那套 ellipsis 裁掉', () => {
  const css = readFileSync(
    new URL('./agent-panel.css', import.meta.url),
    'utf8',
  )
  const message = readFileSync(
    new URL('./AgentPanelMessage.tsx', import.meta.url),
    'utf8',
  )
  assert.match(
    css,
    /\.agent-panel-thinking-thought \{[\s\S]*white-space:\s*pre-wrap/,
  )
  assert.match(css, /@keyframes agent-panel-thought-sweep/)
  assert.match(
    css,
    /\.agent-panel-thinking-sweep \{[\s\S]*background-clip:\s*text/,
  )
  assert.match(
    css,
    /\.agent-panel-thinking-sweep \{[\s\S]*box-decoration-break:\s*clone/,
  )
  assert.doesNotMatch(
    css,
    /@keyframes agent-panel-thought-sweep \{[\s\S]*?-100%/,
  )
  assert.doesNotMatch(
    css,
    /\.agent-panel-thinking-thought\[data-live='true'\]::after/,
  )
  const liveThought = css.match(
    /\.agent-panel-thinking-thought\[data-live='true'\] \{[^}]*\}/,
  )?.[0]
  assert.ok(liveThought)
  assert.doesNotMatch(liveThought, /background-clip/)
  assert.match(css, /\.agent-panel-tools li/)
  assert.doesNotMatch(
    css,
    /\.agent-panel-question \{[\s\S]{0,180}border-radius:\s*24px/,
  )
  assert.match(
    css,
    /\.agent-panel-message-answer[\s\S]*:last-child:not\(\s*\.agent-panel-question/,
  )
  assert.match(css, /\.agent-panel-thinking-slot\[data-open='false'\]/)
  assert.match(
    css,
    /\.agent-panel-thinking-slot\[data-open='false'\] \{[\s\S]*position:\s*absolute/,
  )
  assert.doesNotMatch(css, /grid-template-rows:\s*0fr/)
  assert.equal(THINKING_FOLD_MS, 400)
  assert.ok(BUBBLE_SHRINK_TAU > BUBBLE_GROW_TAU)
  assert.match(message, /requestAnimationFrame/)
  assert.match(message, /currentH/)
  assert.match(message, /ResizeObserver/)
  assert.match(message, /growRef/)
  assert.match(message, /THINKING_FOLD_MS/)
  assert.doesNotMatch(message, /setMoving/)
  assert.doesNotMatch(
    css,
    /\.agent-panel-message-body \{[\s\S]*?interpolate-size/,
  )
  assert.doesNotMatch(
    css,
    /\.agent-panel-thinking \{[\s\S]*?transition:\s*height/,
  )
  assert.match(css, /opacity 400ms/)
  const thinking = readFileSync(
    new URL('./AgentPanelThinking.tsx', import.meta.url),
    'utf8',
  )
  assert.match(thinking, /data-live="true"/)
  assert.match(thinking, /agent-panel-tools/)
  assert.match(thinking, /agent-panel-thinking-sweep/)
  assert.match(
    thinking,
    /note \? \(\s*<p className="agent-panel-thinking-thought" data-live="true">/,
  )
  assert.match(
    message,
    /agent-panel-message-body[\s\S]*showsThinking[\s\S]*AgentPanelThinking/,
  )
  assert.match(message, /messageShowsThinking/)
  assert.match(message, /hideThinking: mode === 'chat'/)
  assert.match(message, /agent-panel-message-grow/)
  assert.match(message, /agent-panel-thinking-slot/)
  assert.match(message, /agent-panel-tag-text/)
})

test('说明性的短句都是贴：操作、附件、收藏、脚注不再另起一套', () => {
  const css = readFileSync(
    new URL('./agent-panel.css', import.meta.url),
    'utf8',
  )
  const message = readFileSync(
    new URL('./AgentPanelMessage.tsx', import.meta.url),
    'utf8',
  )
  const action = readFileSync(
    new URL('./AgentPanelActionCard.tsx', import.meta.url),
    'utf8',
  )
  const composer = readFileSync(
    new URL('./AgentPanelComposer.tsx', import.meta.url),
    'utf8',
  )
  const face = readFileSync(
    new URL('./AgentPanelFace.tsx', import.meta.url),
    'utf8',
  )
  const panel = readFileSync(
    new URL('./AgentPanel.tsx', import.meta.url),
    'utf8',
  )
  const manage = readFileSync(
    new URL('./AgentPanelManage.tsx', import.meta.url),
    'utf8',
  )
  const sessions = readFileSync(
    new URL('./AgentPanelSessions.tsx', import.meta.url),
    'utf8',
  )
  const intention = readFileSync(
    new URL('./AgentPanelIntention.tsx', import.meta.url),
    'utf8',
  )
  assert.doesNotMatch(css, /\.agent-panel-attach-chip/)
  assert.doesNotMatch(css, /\.agent-panel-saved-remove/)
  assert.doesNotMatch(css, /\.agent-panel-action-risk/)
  assert.doesNotMatch(css, /\.agent-panel-action-label \{[\s\S]*text-transform/)
  assert.doesNotMatch(
    css,
    /\.agent-panel-action-section \{[\s\S]{0,160}border-radius:\s*16px/,
  )
  assert.doesNotMatch(css, /#6366f1/)
  assert.match(css, /\.agent-panel-thinking:last-child/)
  assert.match(css, /\.agent-panel-mode/)
  assert.match(css, /\.agent-panel-face \{/)
  assert.match(css, /--agent-face-width:\s*calc\(128px \* 2\.5\)/)
  assert.match(css, /\* 4 \/ 5\)/)
  assert.match(css, /--agent-face-fade/)
  assert.match(css, /\.agent-panel-face-slot \{[\s\S]*?position:\s*absolute/)
  assert.match(css, /backdrop-filter:\s*blur/)
  assert.match(
    css,
    /\.agent-panel-face-rig \.merope-rig img \{[\s\S]*object-fit:\s*contain/,
  )
  assert.match(
    css,
    /@media \(hover: hover\) \{[\s\S]*\.agent-panel-message-footer/,
  )
  assert.match(action, /className="agent-panel-tag"/)
  assert.match(action, /agent-panel-tag-text/)
  assert.match(action, /agent-panel-action-step-note/)
  assert.match(message, /agent-panel-tag-text/)
  assert.doesNotMatch(message, /agent-panel-attach-chip/)
  assert.match(composer, /agent-panel-tag-dismiss/)
  assert.match(composer, /agent-panel-saved-open/)
  assert.match(composer, /agent-panel-mode/)
  assert.match(composer, /AgentPanelFace/)
  assert.match(composer, /agent-panel-face-slot/)
  assert.match(composer, /agent-panel-field-stage/)
  assert.match(composer, /agent-panel-work-chrome/)
  assert.match(composer, /useTagStripScroll/)
  assert.match(composer, /subscribeComposerFavorites/)
  assert.match(message, /addToFavorites[\s\S]*?invalidateComposerFavorites/)
  assert.match(composer, /agent-panel-tag-mid/)
  assert.match(
    css,
    /\.agent-panel-work-chrome \{[\s\S]*?overflow:\s*auto hidden/,
  )
  assert.match(css, /\.agent-panel-work-chrome \{[\s\S]*?flex:\s*1 1 auto/)
  assert.match(css, /\[data-overflow='end'\]/)
  assert.match(composer, /agent-panel-mode-labels/)
  assert.match(composer, /agent-panel-end-slot/)
  assert.match(composer, /agent-panel-end-layer/)
  assert.match(css, /\[data-mode='chat'\] \.agent-panel-face-slot/)
  assert.match(css, /\[data-mode='chat'\] \.agent-panel-work-chrome/)
  assert.match(css, /\.agent-panel-intention \{/)
  assert.match(
    css,
    /\.agent-panel-intention-copy > small\[data-tone='danger'\] \{/,
  )
  assert.doesNotMatch(
    css,
    /\.agent-panel-intention-copy > small \{\s*color:\s*var\(--color-danger/,
  )
  assert.match(intention, /data-tone="danger"/)
  assert.match(intention, /dispatchAgentPanelSubmit|onAccept/)
  assert.match(css, /\.agent-panel-face-slot \{[\s\S]*?position:\s*absolute/)
  assert.match(css, /--agent-face-reserve/)
  assert.match(css, /--agent-face-lift/)
  assert.match(css, /--agent-mode-lag/)
  assert.match(
    css,
    /\.agent-panel-composer\[data-mode='chat'\] \{[\s\S]*?padding-top:\s*var\(--agent-face-reserve\)/,
  )
  assert.match(
    composer,
    /agent-panel-composer-row[\s\S]*?agent-panel-face-slot[\s\S]*?agent-panel-field-stage/,
  )
  assert.match(css, /\.agent-panel-mode-labels/)
  assert.match(
    css,
    /\.agent-panel-mode-labels > span \{[\s\S]*?grid-area:\s*1 \/ 1/,
  )
  assert.match(
    css,
    /\.agent-panel-end-slot \{[\s\S]*?place-items:\s*center end/,
  )
  assert.match(
    css,
    /\.agent-panel-composer-tags \{[\s\S]*?grid-template-columns:\s*auto minmax\(0, 1fr\) auto/,
  )
  assert.match(
    css,
    /\[data-mode='chat'\] \.agent-panel-work-chrome \{[^}]*opacity:\s*0/,
  )
  assert.doesNotMatch(
    css,
    /\[data-mode='chat'\] \.agent-panel-work-chrome \{[^}]*width:\s*0/,
  )
  assert.doesNotMatch(composer, /AgentSwap\s+id=\{mode\}/)
  assert.doesNotMatch(composer, /id=\{chatting \? 'mood' : 'trail'\}/)
  assert.match(composer, /mode\.shortcut/)
  assert.match(composer, /MoodTag/)
  assert.match(composer, /agent-panel-mood/)
  assert.match(composer, /agent-panel-work-tags/)
  assert.match(composer, /agent-panel-tag-cluster/)
  assert.doesNotMatch(composer, /role="radiogroup"/)
  assert.doesNotMatch(face, /className=\{?['"][^'"]*glass/)
  assert.match(panel, /shouldCaptureModeTab/)
  assert.match(panel, /cycleAgentPanelMode/)
  assert.doesNotMatch(composer, /agent-panel-saved-remove/)
  assert.match(manage, /agent-panel-tag agent-panel-manage-row/)
  assert.match(manage, /agent-panel-tag-text/)
  assert.match(sessions, /data-block="true"/)
  assert.match(
    sessions,
    /className="agent-panel-tag"[\s\S]*data-block="true"[\s\S]*data-tone="alert"/,
  )
  assert.doesNotMatch(sessions, /agent-panel-session glass" data-tone/)
})

test('正文里的 think 标签拆成过程和答案', () => {
  assert.deepEqual(splitThinkContent('<think>先算\n再答</think>\n\n答案是 4'), {
    thought: '先算\n再答',
    content: '答案是 4',
  })
  assert.deepEqual(splitThinkContent('<think>还在想'), {
    thought: '还在想',
    content: '',
  })
  assert.deepEqual(splitThinkContent('就是答案'), {
    thought: '',
    content: '就是答案',
  })
})

test('正文若把思考链抄一遍就剥掉，短开场白不动', () => {
  assert.equal(
    peelThoughtFromContent(
      '这是一段足够长的思考过程，用来判断怎么回答。\n\n你好',
      '这是一段足够长的思考过程，用来判断怎么回答。',
    ),
    '你好',
  )
  assert.equal(
    peelThoughtFromContent('好的，今天天气不错', '好的，'),
    '好的，今天天气不错',
  )
})

test('时长按量级换单位，不出现 0.4s 和 83.0s 这种读法', () => {
  assert.equal(formatStepDuration(400), '400ms')
  assert.equal(formatStepDuration(1_500), '1.5s')
  assert.equal(formatStepDuration(59_400), '59.4s')
  assert.equal(formatStepDuration(83_000), '1m23s')
  assert.equal(formatStepDuration(600_000), '10m00s')
})
