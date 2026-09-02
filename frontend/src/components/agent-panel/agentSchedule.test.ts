import assert from 'node:assert/strict'
import test from 'node:test'
import {
  describeSchedule,
  isPlausibleCron,
  matchSchedulePreset,
  SCHEDULE_PRESETS,
} from './agentSchedule'

test('认出「每 n 分钟 / 每 n 小时 / 每小时 / 每天几点」', () => {
  assert.deepEqual(describeSchedule('*/15 * * * *'), {
    kind: 'everyMinutes',
    value: 15,
  })
  assert.deepEqual(describeSchedule('0 */6 * * *'), {
    kind: 'everyHours',
    value: 6,
  })
  assert.deepEqual(describeSchedule('0 * * * *'), { kind: 'hourly' })
  assert.deepEqual(describeSchedule('30 9 * * *'), {
    kind: 'dailyAt',
    time: '09:30',
  })
})

test('每天几点补零，不出现 9:5', () => {
  assert.deepEqual(describeSchedule('5 9 * * *'), {
    kind: 'dailyAt',
    time: '09:05',
  })
})

test('认不出来就原样交回那串 cron —— 猜错的说法比看不懂更危险', () => {
  // 只在周一跑，「每天」是错的
  assert.deepEqual(describeSchedule('0 9 * * 1'), {
    kind: 'raw',
    cron: '0 9 * * 1',
  })
  // 一天跑两次，「每天 09:00」会漏掉一次
  assert.deepEqual(describeSchedule('0 9,18 * * *'), {
    kind: 'raw',
    cron: '0 9,18 * * *',
  })
  // 时间段也不是「每天某一点」
  assert.deepEqual(describeSchedule('0 9-17 * * *'), {
    kind: 'raw',
    cron: '0 9-17 * * *',
  })
  assert.deepEqual(describeSchedule('乱写的'), {
    kind: 'raw',
    cron: '乱写的',
  })
})

test('每 0 分钟不是一个频率，当认不出来处理', () => {
  assert.equal(describeSchedule('*/0 * * * *').kind, 'raw')
})

test('前后空格不影响识别', () => {
  assert.deepEqual(describeSchedule('  0 * * * *  '), { kind: 'hourly' })
})

test('预设能对上，其余算自定义', () => {
  for (const preset of SCHEDULE_PRESETS) {
    assert.equal(matchSchedulePreset(preset.cron), preset.id)
  }
  assert.equal(matchSchedulePreset('0 9 * * 1'), 'custom')
})

test('字段数不对的 cron 不该提交', () => {
  assert.equal(isPlausibleCron('0 9 * * *'), true)
  assert.equal(isPlausibleCron('  0   9 * * *  '), true)
  assert.equal(isPlausibleCron('0 9 * *'), false)
  assert.equal(isPlausibleCron('0 9 * * * *'), false)
  assert.equal(isPlausibleCron(''), false)
})
