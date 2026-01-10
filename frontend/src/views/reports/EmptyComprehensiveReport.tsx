/**
 * 综合报告空状态占位组件
 */

import { motionShim as motion } from '@lib/motionShim'
import { useI18n } from '../../contexts/I18nContext'

interface EmptyComprehensiveReportProps {
  isAdmin: boolean
}

export function EmptyComprehensiveReport({ isAdmin }: EmptyComprehensiveReportProps) {
  const { t } = useI18n()

  return (
    <motion.div
      className="flex-shrink-0 snap-center w-[280px] lg:w-full"
      initial={{ opacity: 0, y: 20, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ duration: 0.4 }}
    >
      <div
        className="relative overflow-hidden rounded-2xl glass flex-shrink-0"
        style={{ aspectRatio: '2 / 1' }}
      >
        <div className="absolute -right-10 -top-10 w-40 h-40 rounded-full blur-3xl opacity-10 bg-gray-400" />
        <div className="absolute -left-10 -bottom-10 w-32 h-32 rounded-full blur-2xl opacity-10 bg-gray-400" />

        <div className="relative z-10 h-full flex items-center justify-between px-6 py-3">
          {/* 左侧：图标区域 */}
          <div className="flex-shrink-0 flex items-center justify-center">
            <div className="relative">
              <div className="absolute inset-0 rounded-xl blur-xl bg-gray-400 opacity-15" />
              <div
                className="relative w-24 h-24 rounded-xl backdrop-blur-sm flex items-center justify-center shadow-2xl overflow-hidden border-2 border-gray-400/20"
                style={{ background: 'var(--glass-bg)' }}
              >
                <div className="text-5xl text-gray-400 dark:text-gray-500 opacity-40">
                  ✨
                </div>
              </div>
            </div>
          </div>

          {/* 右侧：文本信息区域 */}
          <div className="flex-1 min-w-0 flex flex-col justify-center pl-5 pr-2">
            <h2 className="text-lg font-bold mb-1.5 truncate leading-tight text-gray-500 dark:text-gray-400">
              {t.reportsPage.noComprehensiveReport}
            </h2>
            <p className="text-xs text-gray-500 dark:text-gray-500 mb-2 leading-relaxed">
              {isAdmin
                ? t.reportsPage.useInputToGenerate
                : t.reportsPage.adminNotGenerated}
            </p>
            <div className="flex items-center">
              <div className="px-3 py-1.5 rounded-full backdrop-blur-sm font-medium text-xs text-gray-400 dark:text-gray-500 bg-gray-400/10">
                {t.reportsPage.waitingGenerate}
              </div>
            </div>
          </div>
        </div>
      </div>
    </motion.div>
  )
}
