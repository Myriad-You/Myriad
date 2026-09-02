import type { StylizedExpressionTargets } from './expressionRegistry'
import assert from 'node:assert/strict'
import test from 'node:test'
import { ANIME25D_LAYER_DESCRIPTORS } from '../rig/anime25d'
import {
  resolveStylizedExpressionTargets,
  STYLIZED_EXPRESSION_DEFINITIONS,
} from './expressionRegistry'

test('registers every stylized fade on a semantic layer role', () => {
  const registeredFades = new Set(
    Object.values(ANIME25D_LAYER_DESCRIPTORS).flatMap((entry) =>
      entry.fade ? [entry.fade] : [],
    ),
  )
  for (const definition of Object.values(STYLIZED_EXPRESSION_DEFINITIONS)) {
    for (const fade of definition.fades) assert.ok(registeredFades.has(fade))
  }
})

test('applies overlap precedence in one allocation-free pass', () => {
  const input: StylizedExpressionTargets = {
    anger: 1,
    speechless: 1,
    maniac: 0,
    silly: 1,
    lovestruck: 1,
  }
  const output: StylizedExpressionTargets = {
    anger: 0,
    speechless: 0,
    maniac: 0,
    silly: 0,
    lovestruck: 0,
  }
  resolveStylizedExpressionTargets(input, output)
  assert.equal(output.silly, 1)
  assert.equal(output.lovestruck, 0)
  assert.equal(output.anger, 0)
  assert.equal(output.speechless, 0)
})
