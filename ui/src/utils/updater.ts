import { check, type Update } from '@tauri-apps/plugin-updater'

/**
 * 更新信息
 */
export interface UpdateInfo {
    version: string
    date?: string
    body?: string
    currentVersion?: string
}

/**
 * 检查更新
 */
export async function checkForUpdate(): Promise<Update | null> {
    try {
        return await check()
    } catch (error) {
        console.error('[Updater] 检查更新失败:', error)
        throw error
    }
}

/**
 * 下载并安装更新
 *
 * @param update 更新对象
 * @param onProgress 下载进度回调
 */
export async function downloadAndInstallUpdate(
    update: Update,
    onProgress?: (progress: {
        downloaded: number
        total: number
        percent: number
    }) => void
): Promise<void> {
    let downloaded = 0
    let total = 0

    await update.downloadAndInstall((event) => {
        switch (event.event) {
            case 'Started': {
                total = event.data.contentLength ?? 0
                downloaded = 0

                onProgress?.({
                    downloaded,
                    total,
                    percent: 0
                })

                break
            }

            case 'Progress': {
                downloaded += event.data.chunkLength

                const percent =
                    total > 0
                        ? Math.min(100, Math.round((downloaded / total) * 100))
                        : 0

                onProgress?.({
                    downloaded,
                    total,
                    percent
                })

                break
            }

            case 'Finished': {
                onProgress?.({
                    downloaded: total,
                    total,
                    percent: 100
                })

                break
            }
        }
    })
}

/**
 * 获取更新说明
 */
export function getUpdateInfo(update: Update): UpdateInfo {
    return {
        version: update.version,
        date: update.date,
        body: update.body,
        currentVersion: update.currentVersion
    }
}