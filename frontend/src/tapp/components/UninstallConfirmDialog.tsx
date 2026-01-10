/**
 * Tapp 卸载确认对话框组件
 * 统一的卸载确认弹窗，支持保留数据选项
 */

import { FaTrash } from '@lib/icons'
import { AnimatePresenceShim as AnimatePresence, motionShim as motion } from '@lib/motionShim'
import { useCallback, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'

export interface UninstallConfirmDialogProps {
  /** 是否显示对话框 */
  isOpen: boolean
  /** 应用名称 */
  appName: string
  /** 关闭/取消回调 */
  onCancel: () => void
  /** 确认卸载回调，参数为是否保留数据 */
  onConfirm: (keepData: boolean) => Promise<void>
}

/**
 * 卸载确认对话框
 */
export function UninstallConfirmDialog({
  isOpen,
  appName,
  onCancel,
  onConfirm,
}: UninstallConfirmDialogProps) {
  const { t } = useI18n()
  const [keepData, setKeepData] = useState(false)
  const [uninstalling, setUninstalling] = useState(false)

  const handleConfirm = useCallback(async () => {
    setUninstalling(true)
    try {
      await onConfirm(keepData)
    }
    finally {
      setUninstalling(false)
      setKeepData(false)
    }
  }, [onConfirm, keepData])

  const handleCancel = useCallback(() => {
    if (!uninstalling) {
      setKeepData(false)
      onCancel()
    }
  }, [onCancel, uninstalling])

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          className="fixed inset-0 z-[60] flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm"
          onClick={handleCancel}
        >
          <motion.div
            initial={{ scale: 0.9, opacity: 0, y: 20 }}
            animate={{ scale: 1, opacity: 1, y: 0 }}
            exit={{ scale: 0.9, opacity: 0, y: 20 }}
            transition={{ type: 'spring', stiffness: 350, damping: 25 }}
            className="glass rounded-2xl p-6 max-w-md w-full shadow-2xl"
            onClick={(e: React.MouseEvent) => e.stopPropagation()}
          >
            {/* 对话框头部 */}
            <div className="flex items-center gap-3 mb-4">
              <div className="w-12 h-12 rounded-xl bg-gradient-to-br from-red-100 to-orange-100 dark:from-red-900/50 dark:to-orange-900/50 flex items-center justify-center">
                <FaTrash className="text-red-600 dark:text-red-400 text-lg" />
              </div>
              <div>
                <h3 className="text-lg font-bold text-gray-800 dark:text-gray-100">
                  {t.tapp.uninstall}
                </h3>
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  {appName}
                </p>
              </div>
            </div>

            {/* 确认消息 */}
            <p className="text-gray-600 dark:text-gray-300 mb-4">
              {t.tapp.confirmUninstall}
            </p>

            {/* 保留数据选项 */}
            <label className="flex items-start gap-3 p-3 rounded-lg bg-white/50 dark:bg-neutral-900/50 border border-gray-200/50 dark:border-neutral-700/50 cursor-pointer mb-4 hover:bg-white/70 dark:hover:bg-neutral-800/50 transition-colors">
              <input
                type="checkbox"
                checked={keepData}
                onChange={e => setKeepData(e.target.checked)}
                disabled={uninstalling}
                className="mt-0.5 w-4 h-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500 dark:border-neutral-600 dark:bg-neutral-700"
              />
              <div className="flex-1">
                <div className="font-medium text-gray-800 dark:text-gray-100 text-sm">
                  {t.tapp.keepDataOnUninstall}
                </div>
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                  {t.tapp.keepDataOnUninstallDesc}
                </p>
              </div>
            </label>

            {/* 操作按钮 */}
            <div className="flex gap-3">
              <button
                onClick={handleCancel}
                disabled={uninstalling}
                className="flex-1 px-4 py-2.5 font-medium rounded-lg transition-colors bg-gray-100 hover:bg-gray-200 text-gray-700 dark:bg-neutral-800 dark:hover:bg-neutral-700 dark:text-gray-300 disabled:opacity-50"
              >
                {t.tapp.cancel}
              </button>
              <button
                onClick={handleConfirm}
                disabled={uninstalling}
                className="flex-1 px-4 py-2.5 font-medium rounded-lg transition-colors bg-red-600 hover:bg-red-700 text-white disabled:opacity-50 flex items-center justify-center gap-2"
              >
                {uninstalling
                  ? (
                      <>
                        <span className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                        {t.tapp.uninstalling}
                      </>
                    )
                  : (
                      <>
                        <FaTrash className="w-4 h-4" />
                        {t.tapp.confirmUninstallBtn}
                      </>
                    )}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

export default UninstallConfirmDialog
