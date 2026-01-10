/**
 * 编辑模式组件
 */

import type { ImportProgress } from './types'
import {
  LuAlertCircle as AlertCircle,
  LuCheck as Check,
  LuCheckCircle as CheckCircle,
  LuCheckSquare as CheckSquare,
  LuDownload as Download,
  LuLoader2 as Loader2,
  LuMinusSquare as MinusSquare,
  LuRefreshCw as RefreshCw,
  LuSquare as Square,
  LuTrash2 as Trash2,
  LuUpload as Upload,
  LuX as X,
} from '@lib/icons'
import { AnimatePresence, motion } from 'framer-motion'
import { SPRING_SNAPPY } from './constants'

export interface EditModeProps {
  variant: 'mobile' | 'desktop'
  selectedIds: Set<number>
  totalCount: number
  isDeleting: boolean
  isRefreshing: boolean
  isAuthenticated: boolean
  onSelectAll?: () => void
  onBatchDelete?: () => void
  onBatchRefresh?: () => void
  onMarkAllSourcesRead?: () => void
  onClose: () => void
  // 导入导出
  onBrewExport?: () => void
  onBrewImportFile?: (e: React.ChangeEvent<HTMLInputElement>) => void
  importExportLoading?: boolean
  importProgress?: ImportProgress | null
  importExportSuccess?: string | null
  importExportError?: string | null
  brewExportInputRef?: React.RefObject<HTMLInputElement>
  sourcesCount?: number
  t: {
    selectAll: string
    deselectAll: string
    deleteSelected: string
    refreshAllSources: string
    markAllAsRead: string
    exitEdit: string
    exportBrewpack: string
    importBrewpack: string
  }
}

export function EditMode({
  variant,
  selectedIds,
  totalCount,
  isDeleting,
  isRefreshing,
  isAuthenticated,
  onSelectAll,
  onBatchDelete,
  onBatchRefresh,
  onMarkAllSourcesRead,
  onClose,
  onBrewExport,
  onBrewImportFile,
  importExportLoading = false,
  importProgress,
  importExportSuccess,
  importExportError,
  brewExportInputRef,
  sourcesCount = 0,
  t,
}: EditModeProps) {
  const isMobile = variant === 'mobile'

  // 移动端版本 - 简化
  if (isMobile) {
    return (
      <motion.div
        key="edit-bar-mobile"
        initial={{ opacity: 0, y: -8, scale: 0.96 }}
        animate={{ opacity: 1, y: 0, scale: 1 }}
        exit={{ opacity: 0, y: -8, scale: 0.96 }}
        transition={SPRING_SNAPPY}
        className="flex items-center gap-1.5 px-2 py-2 rounded-2xl bg-gray-50/95 dark:bg-neutral-800/95 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10"
      >
        <button
          onClick={onSelectAll}
          className="p-2.5 text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-neutral-700/60 rounded-xl transition-colors disabled:opacity-50"
          title={selectedIds.size === totalCount ? t.deselectAll : t.selectAll}
          aria-label={selectedIds.size === totalCount ? t.deselectAll : t.selectAll}
        >
          {selectedIds.size === totalCount
            ? (
                <CheckSquare className="w-5 h-5" />
              )
            : selectedIds.size > 0
              ? (
                  <MinusSquare className="w-5 h-5" />
                )
              : (
                  <Square className="w-5 h-5" />
                )}
        </button>
        <span className="text-sm font-medium text-gray-600 dark:text-gray-300 min-w-[4.5rem] text-center">
          {selectedIds.size}
          {' '}
          /
          {totalCount}
        </span>
        <button
          onClick={onBatchDelete}
          disabled={isDeleting || selectedIds.size === 0}
          className="p-2.5 text-red-500 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-900/30 disabled:opacity-30 disabled:cursor-not-allowed rounded-xl transition-colors"
          title={t.deleteSelected}
          aria-label={t.deleteSelected}
        >
          <Trash2 className="w-5 h-5" />
        </button>
        <button
          onClick={onBatchRefresh}
          disabled={isRefreshing || totalCount === 0}
          className="p-2.5 text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-neutral-700/60 disabled:opacity-30 disabled:cursor-not-allowed rounded-xl transition-colors"
          title={t.refreshAllSources}
          aria-label={t.refreshAllSources}
        >
          <RefreshCw className={`w-5 h-5 ${isRefreshing ? 'animate-spin' : ''}`} />
        </button>
        {isAuthenticated && onMarkAllSourcesRead && (
          <button
            onClick={onMarkAllSourcesRead}
            className="p-2.5 text-gray-500 dark:text-gray-400 hover:text-green-600 dark:hover:text-green-400 hover:bg-green-50 dark:hover:bg-green-900/20 disabled:opacity-30 disabled:cursor-not-allowed rounded-xl transition-colors"
            title={t.markAllAsRead}
            aria-label={t.markAllAsRead}
          >
            <CheckCircle className="w-5 h-5" />
          </button>
        )}
        <button
          onClick={onClose}
          className="ml-auto p-2.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700/60 rounded-xl transition-colors"
          title={t.exitEdit}
          aria-label={t.exitEdit}
        >
          <X className="w-5 h-5" />
        </button>
      </motion.div>
    )
  }

  // 桌面端版本 - 完整功能
  return (
    <motion.div
      key="edit-bar"
      initial={{ opacity: 0, y: 8, scale: 0.96 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 8, scale: 0.96 }}
      transition={SPRING_SNAPPY}
      className="flex items-center gap-1.5 px-2 py-2 rounded-2xl bg-gray-50/95 dark:bg-neutral-800/95 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10"
    >
      {/* 全选按钮 */}
      <button
        onClick={onSelectAll}
        className="p-2.5 text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-neutral-700/60 rounded-xl transition-colors disabled:opacity-50"
        title={selectedIds.size === totalCount ? t.deselectAll : t.selectAll}
        aria-label={selectedIds.size === totalCount ? t.deselectAll : t.selectAll}
      >
        {selectedIds.size === totalCount
          ? (
              <CheckSquare className="w-5 h-5" />
            )
          : selectedIds.size > 0
            ? (
                <MinusSquare className="w-5 h-5" />
              )
            : (
                <Square className="w-5 h-5" />
              )}
      </button>

      {/* 选中数量 */}
      <span className="text-sm font-medium text-gray-600 dark:text-gray-300 min-w-[4.5rem] text-center">
        {selectedIds.size}
        {' '}
        /
        {totalCount}
      </span>

      {/* 删除按钮 */}
      <button
        onClick={onBatchDelete}
        disabled={isDeleting || selectedIds.size === 0}
        className="p-2.5 text-red-500 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-900/30 disabled:opacity-30 disabled:cursor-not-allowed rounded-xl transition-colors"
        title={t.deleteSelected}
        aria-label={t.deleteSelected}
      >
        <Trash2 className="w-5 h-5" />
      </button>

      {/* 全部刷新按钮 */}
      <button
        onClick={onBatchRefresh}
        disabled={isRefreshing || totalCount === 0}
        className="p-2.5 text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-neutral-700/60 disabled:opacity-30 disabled:cursor-not-allowed rounded-xl transition-colors"
        title={t.refreshAllSources}
        aria-label={t.refreshAllSources}
      >
        <RefreshCw className={`w-5 h-5 ${isRefreshing ? 'animate-spin' : ''}`} />
      </button>

      {/* 全部已读按钮 */}
      {isAuthenticated && onMarkAllSourcesRead && (
        <button
          onClick={onMarkAllSourcesRead}
          className="p-2.5 text-gray-500 dark:text-gray-400 hover:text-green-600 dark:hover:text-green-400 hover:bg-green-50 dark:hover:bg-green-900/20 disabled:opacity-30 disabled:cursor-not-allowed rounded-xl transition-colors"
          title={t.markAllAsRead}
          aria-label={t.markAllAsRead}
        >
          <CheckCircle className="w-5 h-5" />
        </button>
      )}

      {/* 分隔线 */}
      <div className="w-px h-6 bg-gray-200 dark:bg-neutral-700 mx-1" />

      {/* 导出按钮 */}
      {onBrewExport && (
        <button
          onClick={onBrewExport}
          disabled={importExportLoading || sourcesCount === 0}
          className="p-2.5 text-gray-500 dark:text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/20 disabled:opacity-30 disabled:cursor-not-allowed rounded-xl transition-colors"
          title={t.exportBrewpack}
          aria-label={t.exportBrewpack}
        >
          <Download className={`w-5 h-5 ${importExportLoading ? 'animate-pulse' : ''}`} />
        </button>
      )}

      {/* 导入按钮 */}
      {onBrewImportFile && (
        <label
          className={`p-2.5 text-gray-500 dark:text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/20 rounded-xl transition-colors ${importExportLoading ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}`}
          title={t.importBrewpack}
          aria-label={t.importBrewpack}
        >
          <Upload className={`w-5 h-5 ${importExportLoading ? 'animate-pulse' : ''}`} />
          <input
            ref={brewExportInputRef}
            type="file"
            accept=".brewpack,.zip"
            onChange={onBrewImportFile}
            className="hidden"
            disabled={importExportLoading}
            aria-label={t.importBrewpack}
          />
        </label>
      )}

      {/* 导入进度显示 */}
      <AnimatePresence>
        {importProgress && (
          <motion.div
            initial={{ opacity: 0, scale: 0.9, x: -10 }}
            animate={{ opacity: 1, scale: 1, x: 0 }}
            exit={{ opacity: 0, scale: 0.9, x: -10 }}
            transition={{ duration: 0.2 }}
            className="flex items-center gap-2 px-2.5 py-1.5 rounded-xl text-xs font-medium bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 border border-blue-200/50 dark:border-blue-800/50"
          >
            <Loader2 className="w-3.5 h-3.5 flex-shrink-0 animate-spin" />
            <span className="truncate max-w-[10rem]">{importProgress.step}</span>
            {importProgress.total > 0 && (
              <span className="flex-shrink-0 text-[10px] px-1.5 py-0.5 rounded-full bg-blue-100 dark:bg-blue-900/40">
                {importProgress.current}
                /
                {importProgress.total}
              </span>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {/* 导入/导出反馈提示 */}
      <AnimatePresence>
        {(importExportSuccess || importExportError) && !importProgress && (
          <motion.div
            initial={{ opacity: 0, scale: 0.9, x: -10 }}
            animate={{ opacity: 1, scale: 1, x: 0 }}
            exit={{ opacity: 0, scale: 0.9, x: -10 }}
            transition={{ duration: 0.2 }}
            className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-xl text-xs font-medium ${
              importExportSuccess
                ? 'bg-emerald-50 dark:bg-emerald-900/20 text-emerald-600 dark:text-emerald-400 border border-emerald-200/50 dark:border-emerald-800/50'
                : 'bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 border border-red-200/50 dark:border-red-800/50'
            }`}
          >
            {importExportSuccess
              ? (
                  <Check className="w-3.5 h-3.5 flex-shrink-0" />
                )
              : (
                  <AlertCircle className="w-3.5 h-3.5 flex-shrink-0" />
                )}
            <span className="truncate max-w-[12rem]">{importExportSuccess || importExportError}</span>
          </motion.div>
        )}
      </AnimatePresence>

      {/* 退出按钮 */}
      <button
        onClick={onClose}
        className="p-2.5 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-neutral-700/60 rounded-xl transition-colors"
        title={t.exitEdit}
        aria-label={t.exitEdit}
      >
        <X className="w-5 h-5" />
      </button>
    </motion.div>
  )
}
