// 视频首帧提取：Rust 端为列表里的视频生成缩略图缓存。
//
// 为什么必须在 Rust 端做：
//   前端方案（<video> 解码 → canvas.drawImage → toDataURL）在 Tauri 下必然失败。
//   应用页面源是 http://localhost:1420（dev）或 tauri://localhost（release），
//   而视频经 convertFileSrc 得到的是 http://asset.localhost/... —— 跨源资源。
//   一旦把跨源 video 画进 canvas，canvas 即被标记为 tainted，
//   随后 toDataURL()/toBlob() 一律抛 SecurityError（已实测确认）。
//   即使设置 crossOrigin 也无法解除：asset 协议不返回 Access-Control-Allow-Origin。
//   因此视频首帧只能在后端生成（Rust 读文件无同源限制）。
//
// 实现：Windows Media Foundation（系统自带，无第三方依赖）
//   MFCreateSourceReaderFromURL 打开文件 → 请求 RGB32 输出 → 读第一帧 →
//   缩放编码为 JPEG 落盘，复用与图片缩略图相同的缓存目录与命名规则。

#![cfg(windows)]

use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MFCreateAttributes, MFCreateMediaType,
    MFCreateSourceReaderFromURL, MFStartup, MFMediaType_Video, MFVideoFormat_RGB32,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MFSTARTUP_NOSOCKET, MF_VERSION,
};

/// 缩略图最长边（与 thumbs.rs 的 THUMB_SIZE 保持一致）
const THUMB_SIZE: u32 = 256;

/// MF 初始化结果：进程内只初始化一次，跨线程共享。
static MF_INIT: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();

/// 确保 Media Foundation 可用（幂等）。MFStartup 必须调一次，否则所有 MF 调用失败。
fn ensure_mf() -> Result<(), String> {
    let r = MF_INIT.get_or_init(|| unsafe {
        // COM 可能已被其它库初始化（返回 RPC_E_CHANGED_MODE 属正常），忽略该结果
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
        MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).map_err(|e| format!("MFStartup 失败: {e}"))
    });
    r.clone()
}

/// 把 UTF-8 路径转为 UTF-16 宽字符串（含结尾 0），供 MF 使用
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// 读取 IMFMediaType 里的帧尺寸；取不到时返回 None。
///
/// MF_MT_FRAME_SIZE 是打包的 UINT64：高 32 位为宽，低 32 位为高。
/// windows 0.58 未暴露 MFGetAttributeSize 辅助函数，故直接取 UINT64 拆位。
fn frame_size(mt: &IMFMediaType) -> Option<(u32, u32)> {
    unsafe {
        let packed = mt.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
        let w = (packed >> 32) as u32;
        let h = (packed & 0xFFFF_FFFF) as u32;
        if w == 0 || h == 0 {
            None
        } else {
            Some((w, h))
        }
    }
}

/// 将 BGRA 缓冲缩放为最长边不超过 THUMB_SIZE 的 RGB 图并编码 JPEG
fn encode_thumb(bgra: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageBuffer, Rgb};

    let scale = (THUMB_SIZE as f32 / w.max(h) as f32).min(1.0);
    let tw = ((w as f32 * scale).round() as u32).max(1);
    let th = ((h as f32 * scale).round() as u32).max(1);

    // BGRA -> RGB（最近邻缩放；缩略图足够，且不需要额外图像依赖）
    let mut rgb = Vec::with_capacity((tw * th * 3) as usize);
    for y in 0..th {
        let sy = ((y as f32 / scale) as u32).min(h.saturating_sub(1));
        for x in 0..tw {
            let sx = ((x as f32 / scale) as u32).min(w.saturating_sub(1));
            let si = ((sy * w + sx) * 4) as usize;
            if si + 2 >= bgra.len() {
                rgb.extend_from_slice(&[0, 0, 0]);
                continue;
            }
            // MF 的 RGB32 在内存中为 BGRA 字节序
            rgb.push(bgra[si + 2]);
            rgb.push(bgra[si + 1]);
            rgb.push(bgra[si]);
        }
    }

    let Some(buf) = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(tw, th, rgb) else {
        return Err("构造图像缓冲失败".into());
    };
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, 85);
    enc.encode_image(&buf)
        .map_err(|e| format!("JPEG 编码失败: {e}"))?;
    Ok(out)
}

/// 从视频文件读取首帧位图（RGB32）与其尺寸
fn read_first_frame(path: &str) -> Result<(Vec<u8>, u32, u32), String> {
    ensure_mf()?;
    let url = wide(path);

    unsafe {
        // 打开 source reader；开启视频处理让 MF 自动插入色彩/尺寸转换器
        let attrs: IMFAttributes = {
            let mut a: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut a, 2).map_err(|e| format!("MFCreateAttributes 失败: {e}"))?;
            let a = a.ok_or("MFCreateAttributes 返回空")?;
            let _ = a.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);
            let _ = a.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1);
            a
        };

        let reader: IMFSourceReader = MFCreateSourceReaderFromURL(PCWSTR(url.as_ptr()), &attrs)
            .map_err(|e| format!("打开视频失败（格式不支持或文件损坏）: {e}"))?;

        // 请求 RGB32 未压缩输出，避免依赖解码器的默认输出格式
        let out_type: IMFMediaType = {
            let t = MFCreateMediaType().map_err(|e| format!("MFCreateMediaType 失败: {e}"))?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(|e| format!("设置主类型失败: {e}"))?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
                .map_err(|e| format!("设置子类型失败: {e}"))?;
            t
        };
        reader
            .SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &out_type)
            .map_err(|e| format!("设置输出格式失败: {e}"))?;

        // 读取第一个视频样本；部分文件首帧为空样本，循环取到有效帧为止
        for _ in 0..30 {
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            reader
                .ReadSample(
                    MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )
                .map_err(|e| format!("读取帧失败: {e}"))?;

            let Some(sample) = sample else { continue };

            // 尺寸从"当前媒体类型"取（可能已被 video processor 改写）
            let cur = reader
                .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
                .map_err(|e| format!("获取当前格式失败: {e}"))?;
            let Some((w, h)) = frame_size(&cur) else {
                continue;
            };

            // 转为连续内存缓冲后取数据
            let buf = sample
                .ConvertToContiguousBuffer()
                .map_err(|e| format!("帧缓冲转换失败: {e}"))?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut cur_len = 0u32;
            buf.Lock(&mut ptr, None, Some(&mut cur_len))
                .map_err(|e| format!("锁定帧缓冲失败: {e}"))?;
            if ptr.is_null() || cur_len == 0 {
                let _ = buf.Unlock();
                continue;
            }
            let need = (w as usize) * (h as usize) * 4;
            let take = (cur_len as usize).min(need);
            let data = std::slice::from_raw_parts(ptr as *const u8, take).to_vec();
            let _ = buf.Unlock();

            if data.len() < need {
                continue; // 缓冲不足一帧，继续取
            }
            return Ok((data, w, h));
        }
        Err("未能从视频中取出有效帧".into())
    }
}

/// 为视频生成首帧缩略图，写入 `thumb_path`（JPEG）。成功返回 Ok(())。
pub fn generate_video_thumb(src: &str, thumb_path: &Path) -> Result<(), String> {
    let (data, w, h) = read_first_frame(src)?;
    let jpg = encode_thumb(&data, w, h)?;
    if let Some(parent) = thumb_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建缓存目录失败: {e}"))?;
    }
    std::fs::write(thumb_path, jpg).map_err(|e| format!("写入缩略图失败: {e}"))?;
    Ok(())
}

/// 该扩展名是否为可用 MF 解码的视频格式
pub fn is_video_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "m4v"
    )
}
