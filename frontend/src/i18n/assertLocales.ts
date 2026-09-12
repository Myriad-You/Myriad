import type { TranslationKeys } from './index'
import agentCapsDe from './agentCaps.de-DE.json' with { type: 'json' }
import agentCapsFr from './agentCaps.fr-FR.json' with { type: 'json' }
import agentCapsJa from './agentCaps.ja-JP.json' with { type: 'json' }
import agentCapsKo from './agentCaps.ko-KR.json' with { type: 'json' }
import agentCapsZh from './agentCaps.zh-CN.json' with { type: 'json' }
import agentCapsTw from './agentCaps.zh-TW.json' with { type: 'json' }
import { assembleLocale } from './assembleLocale'
import brewDe from './brew.de-DE.json' with { type: 'json' }
import brewFr from './brew.fr-FR.json' with { type: 'json' }
import brewJa from './brew.ja-JP.json' with { type: 'json' }
import brewKo from './brew.ko-KR.json' with { type: 'json' }
import brewZh from './brew.zh-CN.json' with { type: 'json' }
import brewTw from './brew.zh-TW.json' with { type: 'json' }
import configDe from './config.de-DE.json' with { type: 'json' }
import configFr from './config.fr-FR.json' with { type: 'json' }
import configJa from './config.ja-JP.json' with { type: 'json' }
import configKo from './config.ko-KR.json' with { type: 'json' }
import configZh from './config.zh-CN.json' with { type: 'json' }
import configTw from './config.zh-TW.json' with { type: 'json' }
import de from './de-DE.json' with { type: 'json' }
import errorsDe from './errors.de-DE.json' with { type: 'json' }
import errorsFr from './errors.fr-FR.json' with { type: 'json' }
import errorsJa from './errors.ja-JP.json' with { type: 'json' }
import errorsKo from './errors.ko-KR.json' with { type: 'json' }
import errorsZh from './errors.zh-CN.json' with { type: 'json' }
import errorsTw from './errors.zh-TW.json' with { type: 'json' }
import fr from './fr-FR.json' with { type: 'json' }
import ja from './ja-JP.json' with { type: 'json' }
import ko from './ko-KR.json' with { type: 'json' }
import meropeDe from './merope.de-DE.json' with { type: 'json' }
import meropeFr from './merope.fr-FR.json' with { type: 'json' }
import meropeJa from './merope.ja-JP.json' with { type: 'json' }
import meropeKo from './merope.ko-KR.json' with { type: 'json' }
import meropeZh from './merope.zh-CN.json' with { type: 'json' }
import meropeTw from './merope.zh-TW.json' with { type: 'json' }
import tappDe from './tapp.de-DE.json' with { type: 'json' }
import tappFr from './tapp.fr-FR.json' with { type: 'json' }
import tappJa from './tapp.ja-JP.json' with { type: 'json' }
import tappKo from './tapp.ko-KR.json' with { type: 'json' }
import tappZh from './tapp.zh-CN.json' with { type: 'json' }
import tappTw from './tapp.zh-TW.json' with { type: 'json' }
import zh from './zh-CN.json' with { type: 'json' }
import tw from './zh-TW.json' with { type: 'json' }

const _zh: TranslationKeys = assembleLocale(zh, {
  config: configZh,
  tapp: tappZh,
  brew: brewZh,
  merope: meropeZh,
  errors: errorsZh,
  agentCaps: agentCapsZh,
})
const _tw: TranslationKeys = assembleLocale(tw, {
  config: configTw,
  tapp: tappTw,
  brew: brewTw,
  merope: meropeTw,
  errors: errorsTw,
  agentCaps: agentCapsTw,
})
const _ja: TranslationKeys = assembleLocale(ja, {
  config: configJa,
  tapp: tappJa,
  brew: brewJa,
  merope: meropeJa,
  errors: errorsJa,
  agentCaps: agentCapsJa,
})
const _ko: TranslationKeys = assembleLocale(ko, {
  config: configKo,
  tapp: tappKo,
  brew: brewKo,
  merope: meropeKo,
  errors: errorsKo,
  agentCaps: agentCapsKo,
})
const _fr: TranslationKeys = assembleLocale(fr, {
  config: configFr,
  tapp: tappFr,
  brew: brewFr,
  merope: meropeFr,
  errors: errorsFr,
  agentCaps: agentCapsFr,
})
const _de: TranslationKeys = assembleLocale(de, {
  config: configDe,
  tapp: tappDe,
  brew: brewDe,
  merope: meropeDe,
  errors: errorsDe,
  agentCaps: agentCapsDe,
})

void _zh
void _tw
void _ja
void _ko
void _fr
void _de
