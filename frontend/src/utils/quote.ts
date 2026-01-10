import { API_URL } from '../config'

export interface QuoteData {
  text: string
  author?: string
}

/**
 * 获取一言警句
 */
export async function getRandomQuote(locale?: string): Promise<QuoteData | null> {
  try {
    // 从 localStorage 读取缓存
    const cachedQuote = localStorage.getItem('quote_cache')
    const cacheTime = localStorage.getItem('quote_cache_time')

    if (cachedQuote && cacheTime) {
      const cacheAge = Date.now() - Number.parseInt(cacheTime)
      // 缓存 10 分钟
      if (cacheAge < 10 * 60 * 1000) {
        return JSON.parse(cachedQuote)
      }
    }

    // 使用后端代理访问一言 API（解决 CORS 问题）
    const response = await fetch(`${API_URL}/api/proxy/hitokoto`, {
      signal: AbortSignal.timeout(10000),
    })

    if (!response.ok)
      throw new Error('Hitokoto API failed')

    const data = await response.json()

    const quoteData: QuoteData = {
      text: data.hitokoto,
      author: data.from,
    }

    // 缓存结果
    localStorage.setItem('quote_cache', JSON.stringify(quoteData))
    localStorage.setItem('quote_cache_time', Date.now().toString())

    return quoteData
  }
  catch (error) {
    console.warn('Failed to fetch quote:', error)
    // 返回本地备用句子
    return getLocalQuote(locale)
  }
}

/**
 * 本地备用句子库
 */
function getLocalQuote(locale?: string): QuoteData {
  const quotesZhCN = [
    { text: '代码如诗，优雅至上', author: '程序员格言' },
    { text: '简洁是可靠的前提', author: 'Edsger Dijkstra' },
    { text: '过早优化是万恶之源', author: 'Donald Knuth' },
    { text: '任何可以被编写成 JavaScript 的程序，最终都会被编写成 JavaScript', author: 'Atwood 定律' },
    { text: '好的代码本身就是最好的文档', author: 'Steve McConnell' },
    { text: '先让它运行起来，再让它变得更好', author: 'Kent Beck' },
    { text: '代码是写给人看的，顺便让机器执行', author: 'Harold Abelson' },
    { text: '测试不能证明程序没有 bug，只能证明 bug 的存在', author: 'Edsger Dijkstra' },
  ]

  const quotesEnUS = [
    { text: 'Code is like humor. When you have to explain it, it\'s bad.', author: 'Cory House' },
    { text: 'Simplicity is the soul of efficiency.', author: 'Austin Freeman' },
    { text: 'Make it work, make it right, make it fast.', author: 'Kent Beck' },
    { text: 'Talk is cheap. Show me the code.', author: 'Linus Torvalds' },
    { text: 'Software is eating the world.', author: 'Marc Andreessen' },
    { text: 'The best way to predict the future is to invent it.', author: 'Alan Kay' },
  ]

  const quotesJaJP = [
    { text: 'コードは詩のように、優雅であれ', author: 'プログラマーの格言' },
    { text: 'シンプルさは信頼性の前提条件である', author: 'Edsger Dijkstra' },
    { text: '早すぎる最適化は諸悪の根源', author: 'Donald Knuth' },
    { text: '動くようにしてから、正しくしてから、速くする', author: 'Kent Beck' },
    { text: '良いコードは最高のドキュメントである', author: 'Steve McConnell' },
    { text: '未来を予測する最良の方法は、それを発明することだ', author: 'Alan Kay' },
  ]

  let quotes: QuoteData[]
  switch (locale) {
    case 'en-US':
      quotes = quotesEnUS
      break
    case 'ja-JP':
      quotes = quotesJaJP
      break
    default:
      quotes = quotesZhCN
  }

  return quotes[Math.floor(Math.random() * quotes.length)]
}
