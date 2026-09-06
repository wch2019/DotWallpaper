import {computed, ref} from 'vue'
import {defineStore} from 'pinia'
import type {Update} from '@tauri-apps/plugin-updater'

import {
    checkForUpdate,
    downloadAndInstallUpdate
} from '@/utils/updater'

export const useUpdaterStore = defineStore('updater', () => {
    /**
     * 当前更新对象
     */
    const update = ref<Update | null>(null)

    /**
     * 是否正在检查更新
     */
    const checking = ref(false)

    /**
     * 是否正在下载更新
     */
    const downloading = ref(false)

    /**
     * 下载进度 0 ~ 100
     */
    const progress = ref(0)

    /**
     * 已下载字节数
     */
    const downloaded = ref(0)

    /**
     * 总字节数
     */
    const total = ref(0)

    /**
     * 是否已经检查过更新
     */
    const checked = ref(false)

    /**
     * 错误信息
     */
    const error = ref<unknown>(null)

    /**
     * 更新弹窗是否可见（TitleBar 按钮与关于页检查共用同一弹窗）
     */
    const dialogVisible = ref(false)

    /**
     * 是否有新版本
     */
    const hasUpdate = computed(() => {
        return update.value !== null
    })

    /**
     * 最新版本
     */
    const latestVersion = computed(() => {
        return update.value?.version ?? ''
    })

    /**
     * 当前版本
     */
    const currentVersion = computed(() => {
        return update.value?.currentVersion ?? ''
    })

    /**
     * 更新日期
     */
    const updateDate = computed(() => {
        return update.value?.date ?? ''
    })

    /**
     * 更新说明
     */
    const updateNotes = computed(() => {
        return update.value?.body ?? ''
    })

    /**
     * 是否正在更新
     */
    const updating = computed(() => {
        return downloading.value
    })

    /**
     * 检查更新
     */
    async function checkUpdate(): Promise<Update | null> {
        // 防止重复检查
        if (checking.value) {
            return update.value
        }

        checking.value = true
        error.value = null

        try {
            const result = await checkForUpdate()

            update.value = result
            checked.value = true

            return result
        } catch (err) {
            console.error('[Updater] 检查更新失败:', err)

            error.value = err
            checked.value = true

            return null
        } finally {
            checking.value = false
        }
    }

    /**
     * 下载并安装更新
     */
    async function installUpdate(): Promise<boolean> {
        if (!update.value) {
            return false
        }

        // 防止重复下载
        if (downloading.value) {
            return false
        }

        downloading.value = true
        error.value = null

        progress.value = 0
        downloaded.value = 0
        total.value = 0

        try {
            await downloadAndInstallUpdate(
                update.value,
                (data) => {
                    downloaded.value = data.downloaded
                    total.value = data.total
                    progress.value = data.percent
                }
            )

            // 下载完成后安装流程接管（应用通常会自动重启），先收起弹窗
            closeUpdateDialog()

            return true
        } catch (err) {
            console.error('[Updater] 下载更新失败:', err)

            error.value = err

            return false
        } finally {
            downloading.value = false
        }
    }

    /**
     * 打开更新弹窗
     */
    function openUpdateDialog() {
        dialogVisible.value = true
    }

    /**
     * 关闭更新弹窗
     */
    function closeUpdateDialog() {
        // 下载中不允许关闭弹窗（安装过程即将接管界面）
        if (downloading.value) {
            return
        }
        dialogVisible.value = false
    }

    /**
     * 重置状态
     */
    function reset() {
        update.value = null

        checking.value = false
        downloading.value = false

        progress.value = 0
        downloaded.value = 0
        total.value = 0

        checked.value = false
        error.value = null
        dialogVisible.value = false
    }

    /**
     * 清除错误
     */
    function clearError() {
        error.value = null
    }

    return {
        // 状态
        update,
        checking,
        downloading,
        updating,
        progress,
        downloaded,
        total,
        checked,
        error,

        // 更新信息
        hasUpdate,
        latestVersion,
        currentVersion,
        updateDate,
        updateNotes,

        // 弹窗显隐
        dialogVisible,
        openUpdateDialog,
        closeUpdateDialog,

        // 方法
        checkUpdate,
        installUpdate,
        reset,
        clearError
    }
})
