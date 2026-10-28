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
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION,
    MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READER_MEDIASOURCE, MFSTARTUP_NOSOCKET,
    MF_VERSION,
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

/// 打开视频文件的 SourceReader（开启视频处理，让 MF 自动插入色彩/尺寸转换器）。
///
/// 抽出来是因为现在有两条路径要用它：生成首帧缩略图、以及**只读时长**的探测。
/// 后者用于"缩略图已缓存但时长还没采集过"的老缓存（见 `probe_duration`）。
fn open_reader(path: &str) -> Result<IMFSourceReader, String> {
    ensure_mf()?;
    let url = wide(path);
    unsafe {
        let attrs: IMFAttributes = {
            let mut a: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut a, 2).map_err(|e| format!("MFCreateAttributes 失败: {e}"))?;
            let a = a.ok_or("MFCreateAttributes 返回空")?;
            let _ = a.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);
            let _ = a.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1);
            a
        };
        MFCreateSourceReaderFromURL(PCWSTR(url.as_ptr()), &attrs)
            .map_err(|e| format!("打开视频失败（格式不支持或文件损坏）: {e}"))
    }
}

/// 读视频时长（秒）。取不到返回 None。
///
/// `MF_PD_DURATION` 是 PROPVARIANT，单位 **100 纳秒**（Windows FILETIME 刻度）。
///
/// 两条踩过的坑：
/// 1. windows-core 0.58 的 `PROPVARIANT` 是 **newtype 包装**（`PROPVARIANT(imp::PROPVARIANT)`），
///    内部联合体字段不对外暴露 —— 写 `pv.Anonymous.Anonymous.vt` 编译不过。
///    走官方转换 `u64::try_from(&pv)`（内部是 `PropVariantToUInt64`）即可，
///    它本身就能吃 VT_UI8 和 VT_UI4，不必自己读 vt 再拆联合体。
/// 2. **必须用 `MF_SOURCE_READER_MEDIASOURCE` 这个伪流索引**（-1，转成 u32 即 0xFFFFFFFF），
///    不是 `FIRST_VIDEO_STREAM`(0) —— 时长属于媒体源，挂在流上去问拿不到。
unsafe fn reader_duration_secs(reader: &IMFSourceReader) -> Option<u32> {
    let pv = reader
        .GetPresentationAttribute(
            MF_SOURCE_READER_MEDIASOURCE.0 as u32,
            &MF_PD_DURATION,
        )
        .ok()?;
    let ticks = u64::try_from(&pv).ok()?;
    if ticks == 0 {
        return None;
    }
    // 100ns → 秒；不足 1 秒的短片按 1 秒计（角标显示 0:00 没有意义）
    Some(((ticks / 10_000_000) as u32).max(1))
}

/// 时长入口：先问 Media Foundation，读不到再自己扫 MP4 容器。
///
/// 两条路的覆盖面正好互补：MF 通吃 webm/mkv/mov，但对 fMP4（Pixabay 下载的那种）
/// 只能报 0；容器解析只管 MP4 系列，却能把 fMP4 算准。所以是**或**的关系，不是二选一。
fn duration_of(src: &str, reader: &IMFSourceReader) -> Option<u32> {
    unsafe { reader_duration_secs(reader) }.or_else(|| mp4_duration_secs(src))
}

/// 只读时长、不解码任何帧。
///
/// 给"缩略图早已缓存、但时长还没采集过"的老缓存用（升级前的版本没有 `.dur` 边车）。
/// 打开 + 读属性通常 10~30ms，远低于解码首帧，可以放心放进后台线程池。
pub fn probe_duration(src: &str) -> Option<u32> {
    let reader = open_reader(src).ok()?;
    duration_of(src, &reader)
}

/// 从视频文件读取首帧位图（RGB32）、其尺寸，以及时长（秒，0 = 未知）
fn read_first_frame(path: &str) -> Result<(Vec<u8>, u32, u32, u32), String> {
    let reader = open_reader(path)?;

    unsafe {
        // 时长在打开后立刻可读，顺手带出去 —— 否则上层为了一个数字要再开一次文件
        let duration = duration_of(path, &reader).unwrap_or(0);

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
            return Ok((data, w, h, duration));
        }
        Err("未能从视频中取出有效帧".into())
    }
}

/// 为视频生成首帧缩略图，写入 `thumb_path`（JPEG）。
///
/// 返回**时长（秒）**，0 表示读不到 —— 上层顺手落进 `.dur` 边车，
/// 这样列表后续只需读一个小文件，不必再打开视频。
pub fn generate_video_thumb(src: &str, thumb_path: &Path) -> Result<u32, String> {
    let (data, w, h, duration) = read_first_frame(src)?;
    let jpg = encode_thumb(&data, w, h)?;
    if let Some(parent) = thumb_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建缓存目录失败: {e}"))?;
    }
    std::fs::write(thumb_path, jpg).map_err(|e| format!("写入缩略图失败: {e}"))?;
    Ok(duration)
}

/// 该扩展名是否为可用 MF 解码的视频格式
pub fn is_video_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "m4v"
    )
}

// ---------- fMP4 时长兜底（Media Foundation 读不到时用） ----------
//
// 为什么必须兜底：Pixabay 的 CDN 给的是 **fragmented MP4**（`ftyp iso5`），
// 这种容器的 `mvhd.duration` 恒为 **0** —— 真实时长散落在各个 `moof` 的样本表里，
// 于是 `MF_PD_DURATION` 也报 0。实测本机 10 个视频里恰好 3 个是这种，
// **全部来自 Pixabay 下载**（正是用户最常看的那批），不兜底就等于
// "下载回来的视频永远没有时长角标"。
//
// 兜底办法：自己扫容器。累加每个 `moof` → `traf` → `trun` 的样本时长即可。
// 纯字节解析、不解码；`mdat` 靠 seek 跳过，代价与分片数成正比（每片只读几 KB）。
// 真值已用 Chrome 的 `<video>.duration` 校准过（5.333 / 20.4 / 30.083 → 5 / 20 / 30）。

/// 遍历一层 box（4 字节 size + 4 字节 type，size==1 时后跟 8 字节 64 位长度）。
/// 负载（去掉头部）传给回调；遇到非法长度立即停止，绝不 panic。
fn each_box<'a>(buf: &'a [u8], f: &mut dyn FnMut(&'a [u8], &'a [u8])) {
    let mut off = 0usize;
    while off + 8 <= buf.len() {
        let size32 = u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) as u64;
        let typ = &buf[off + 4..off + 8];
        let (size, hdr) = if size32 == 1 {
            if off + 16 > buf.len() {
                return;
            }
            let mut a = [0u8; 8];
            a.copy_from_slice(&buf[off + 8..off + 16]);
            (u64::from_be_bytes(a), 16usize)
        } else if size32 == 0 {
            // size 0 = 一直到文件/缓冲末尾
            ((buf.len() - off) as u64, 8usize)
        } else {
            (size32, 8usize)
        };
        let size = size as usize;
        if size < hdr || off + size > buf.len() {
            return;
        }
        f(typ, &buf[off + hdr..off + size]);
        off += size;
    }
}

/// 在一层 box 里找第一个指定类型，返回其负载
fn find_box<'a>(buf: &'a [u8], name: &[u8; 4]) -> Option<&'a [u8]> {
    let mut found: Option<&'a [u8]> = None;
    each_box(buf, &mut |t, p| {
        if found.is_none() && t == name {
            found = Some(p);
        }
    });
    found
}

fn be32(buf: &[u8], off: usize) -> Option<u32> {
    let b = buf.get(off..off + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn be64(buf: &[u8], off: usize) -> Option<u64> {
    let b = buf.get(off..off + 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(b);
    Some(u64::from_be_bytes(a))
}

/// `mvhd` → (timescale, duration)。v0/v1 两种布局偏移不同。
fn mvhd_info(buf: &[u8]) -> Option<(u32, u64)> {
    match *buf.first()? {
        1 => Some((be32(buf, 20)?, be64(buf, 24)?)),
        _ => Some((be32(buf, 12)?, be32(buf, 16)? as u64)),
    }
}

/// `mehd` → fragment_duration（fMP4 的可选总时长字段，Pixabay 的文件里通常没有）
fn mehd_duration(buf: &[u8]) -> Option<u64> {
    match *buf.first()? {
        1 => be64(buf, 4),
        _ => Some(be32(buf, 4)? as u64),
    }
}

/// `mdhd` → 轨道媒体 timescale。
///
/// ⚠️ 这是 **fMP4 求和最容易错的一步**：`trun` 里的 sample_duration 是
/// **轨道媒体时基**的刻度，不是 `mvhd` 的影片时基。音轨（44100/48000）和
/// 视频轨（常 12800/90000）时基不同，若把两条轨的样本时长加在一起、
/// 再统一除以影片 timescale，会得到一个**大十几倍的假时长**
/// （实测 5.3s 算成 81s）。正确做法是**分轨累加、各除各的时基，最后取最大**。
fn mdhd_timescale(buf: &[u8]) -> Option<u32> {
    match *buf.first()? {
        1 => be32(buf, 20),
        _ => be32(buf, 12),
    }
}

/// `tkhd` → track_ID（v0 在偏移 12，v1 在偏移 20）
fn tkhd_track_id(buf: &[u8]) -> Option<u32> {
    match *buf.first()? {
        1 => be32(buf, 20),
        _ => be32(buf, 12),
    }
}

/// `tfhd` → track_ID
fn tfhd_track_id(buf: &[u8]) -> Option<u32> {
    be32(buf, 4)
}

/// `tfhd` 里的 default_sample_duration（有才返回）
fn tfhd_default_duration(buf: &[u8]) -> Option<u32> {
    let flags = be32(buf, 0)? & 0x00FF_FFFF;
    let mut off = 8usize; // version+flags(4) + track_ID(4)
    if flags & 0x00_0001 != 0 {
        off += 8; // base_data_offset
    }
    if flags & 0x00_0002 != 0 {
        off += 4; // sample_description_index
    }
    if flags & 0x00_0008 != 0 {
        return be32(buf, off);
    }
    None
}

/// `trun` → 该片段所有样本的时长之和（**轨道时基**刻度）。
/// 每个样本的时长可能逐条给出，也可能全部走 tfhd 的默认值。
fn trun_ticks(buf: &[u8], default_dur: u32) -> u64 {
    let flags = match be32(buf, 0) {
        Some(v) => v & 0x00FF_FFFF,
        None => return 0,
    };
    let count = match be32(buf, 4) {
        Some(c) => c,
        None => return 0,
    };
    let mut off = 8usize;
    if flags & 0x00_0001 != 0 {
        off += 4; // data_offset
    }
    if flags & 0x00_0004 != 0 {
        off += 4; // first_sample_flags
    }
    // 每条样本记录的字段数由标志位决定
    let mut per = 0usize;
    for bit in [0x00_0100u32, 0x00_0200, 0x00_0400, 0x00_0800] {
        if flags & bit != 0 {
            per += 4;
        }
    }
    if per == 0 {
        return count as u64 * default_dur as u64;
    }
    let mut total = 0u64;
    for i in 0..count as usize {
        let d = if flags & 0x00_0100 != 0 {
            be32(buf, off + i * per).unwrap_or(0)
        } else {
            default_dur
        };
        total += d as u64;
    }
    total
}

/// 一个 `moof` 的样本时长：**按 track_ID 分别累加**到 `acc`
fn moof_accumulate(moof: &[u8], acc: &mut std::collections::HashMap<u32, u64>) {
    each_box(moof, &mut |t, traf| {
        if t != b"traf" {
            return;
        }
        let mut track_id = 0u32;
        let mut default_dur = 0u32;
        let mut truns: Vec<&[u8]> = Vec::new();
        each_box(traf, &mut |t2, p2| {
            if t2 == b"tfhd" {
                track_id = tfhd_track_id(p2).unwrap_or(0);
                if let Some(d) = tfhd_default_duration(p2) {
                    default_dur = d;
                }
            } else if t2 == b"trun" {
                truns.push(p2);
            }
        });
        if track_id == 0 {
            return;
        }
        let sum: u64 = truns.into_iter().map(|tr| trun_ticks(tr, default_dur)).sum();
        *acc.entry(track_id).or_insert(0) += sum;
    });
}

/// 直接从 MP4 容器算时长（秒）。
/// 渐进式 mp4 读 `mvhd`；fMP4 分轨累加 `moof` 的样本时长后取最长的一条轨。
/// 非 MP4（webm/mkv…）会很快返回 None。
fn mp4_duration_secs(path: &str) -> Option<u32> {
    use std::collections::HashMap;
    use std::io::{Read, Seek, SeekFrom};

    // 单个 box 的读取上限：正常 moov < 1MB、moof 几 KB。
    // 超过这个量级只可能是文件损坏（长度字段被写坏），直接放弃而不是分配几个 GB。
    const MAX_BOX: u64 = 32 * 1024 * 1024;

    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut pos = 0u64;
    let mut movie_timescale = 0u32;
    let mut movie_ticks = 0u64; // mvhd / mehd 给出的总时长（影片时基）
    let mut track_ts: HashMap<u32, u32> = HashMap::new(); // track_ID → 媒体时基
    let mut track_ticks: HashMap<u32, u64> = HashMap::new();
    let mut header = [0u8; 16];

    while pos + 8 <= len {
        f.seek(SeekFrom::Start(pos)).ok()?;
        f.read_exact(&mut header[..8]).ok()?;
        let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
        let typ: [u8; 4] = [header[4], header[5], header[6], header[7]];
        let (size, hdr) = if size32 == 1 {
            f.read_exact(&mut header[8..16]).ok()?;
            let mut a = [0u8; 8];
            a.copy_from_slice(&header[8..16]);
            (u64::from_be_bytes(a), 16u64)
        } else if size32 == 0 {
            (len - pos, 8u64)
        } else {
            (size32, 8u64)
        };
        if size < hdr || pos + size > len {
            break;
        }
        if &typ == b"moov" || &typ == b"moof" {
            let body = size - hdr;
            if body > MAX_BOX {
                break;
            }
            let mut buf = vec![0u8; body as usize];
            f.read_exact(&mut buf).ok()?;
            if &typ == b"moov" {
                if let Some(m) = find_box(&buf, b"mvhd") {
                    if let Some((ts, dur)) = mvhd_info(m) {
                        movie_timescale = ts;
                        if dur > 0 {
                            movie_ticks = dur; // 渐进式 mp4：这就是答案
                        }
                    }
                }
                if movie_ticks == 0 {
                    if let Some(mvex) = find_box(&buf, b"mvex") {
                        if let Some(mehd) = find_box(mvex, b"mehd") {
                            movie_ticks = mehd_duration(mehd).unwrap_or(0);
                        }
                    }
                }
                // 每个 trak 记下自己的 track_ID 与媒体时基
                each_box(&buf, &mut |t, trak| {
                    if t != b"trak" {
                        return;
                    }
                    let id = find_box(trak, b"tkhd").and_then(tkhd_track_id);
                    let ts = find_box(trak, b"mdia")
                        .and_then(|mdia| find_box(mdia, b"mdhd"))
                        .and_then(mdhd_timescale);
                    if let (Some(id), Some(ts)) = (id, ts) {
                        if ts > 0 {
                            track_ts.insert(id, ts);
                        }
                    }
                });
                if movie_ticks > 0 {
                    break; // 已知，不必再扫后面的分片
                }
            } else {
                moof_accumulate(&buf, &mut track_ticks);
            }
        }
        pos += size;
    }

    if movie_ticks > 0 && movie_timescale > 0 {
        let secs = movie_ticks / movie_timescale as u64;
        return (secs > 0).then(|| secs.min(u32::MAX as u64) as u32);
    }

    // fMP4：每条轨各按自己的时基换算，取最长的那条（音轨通常短一点点）
    let mut best = 0u64;
    for (id, ticks) in &track_ticks {
        let ts = track_ts.get(id).copied().unwrap_or(movie_timescale);
        if ts == 0 || *ticks == 0 {
            continue;
        }
        best = best.max(ticks / ts as u64);
    }
    (best > 0).then(|| best.min(u32::MAX as u64) as u32)
}


#[cfg(test)]
mod tests {
    /// 时长探测的**手工**验证：单元测试跑不了"随便找一个真视频"，
    /// 且 MF 需要真实解码器，故用环境变量喂路径，不设则直接跳过（CI 安全）。
    ///
    /// ```bash
    /// DW_TEST_VIDEO="C:\path\a.mp4" cargo test --bins duration_probe -- --nocapture
    /// ```
    ///
    /// 为什么值得单独验：`MF_PD_DURATION` 的伪流索引、PROPVARIANT 类型、
    /// 100ns 换算三处任一写错都不会编译失败，只会**静默返回 None**，
    /// 表现就是"本地视频永远没有时长角标"，从 UI 上完全看不出是哪一环错。
    #[test]
    fn duration_probe() {
        let Ok(path) = std::env::var("DW_TEST_VIDEO") else {
            return;
        };
        let secs = super::probe_duration(&path).expect("读不到时长");
        println!("{path} -> {secs}s");
        // 合理性区间：1 秒 ~ 1 小时。真值靠人肉对照播放器，
        // 这里只保证"不是 0、不是天文数字"。
        assert!(secs >= 1 && secs <= 3600, "时长不合理: {secs}s");
    }

    /// 手工构造一个 box（4 字节长度 + 4 字节类型 + 负载）
    fn bx(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
        v.extend_from_slice(typ);
        v.extend_from_slice(payload);
        v
    }

    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.clone()).collect()
    }

    /// `mvhd` v0：version+flags(4) creation(4) modification(4) timescale(4) duration(4)
    fn mvhd_v0(timescale: u32, duration: u32) -> Vec<u8> {
        let mut p = vec![0u8; 4];
        p.extend_from_slice(&0u32.to_be_bytes()); // creation
        p.extend_from_slice(&0u32.to_be_bytes()); // modification
        p.extend_from_slice(&timescale.to_be_bytes());
        p.extend_from_slice(&duration.to_be_bytes());
        bx(b"mvhd", &p)
    }

    /// `tkhd` v0：version+flags(4) creation(4) modification(4) track_ID(4)
    fn tkhd_v0(track_id: u32) -> Vec<u8> {
        let mut p = vec![0u8; 12];
        p.extend_from_slice(&track_id.to_be_bytes());
        bx(b"tkhd", &p)
    }

    /// `mdhd` v0：version+flags(4) creation(4) modification(4) timescale(4)
    fn mdhd_v0(timescale: u32) -> Vec<u8> {
        let mut p = vec![0u8; 12];
        p.extend_from_slice(&timescale.to_be_bytes());
        bx(b"mdhd", &p)
    }

    fn trak_v0(track_id: u32, timescale: u32) -> Vec<u8> {
        let mdia = bx(b"mdia", &mdhd_v0(timescale));
        bx(b"trak", &cat(&[tkhd_v0(track_id), mdia]))
    }

    fn tfhd_v0(track_id: u32, default_dur: u32) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&0x0000_0008u32.to_be_bytes()); // version0 + default_sample_duration 存在
        p.extend_from_slice(&track_id.to_be_bytes());
        p.extend_from_slice(&default_dur.to_be_bytes());
        bx(b"tfhd", &p)
    }

    /// `trun`：count 条样本，时长逐条给出
    fn trun_v0(count: u32, each: u32) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&0x0000_0100u32.to_be_bytes()); // sample_duration 逐条给出
        p.extend_from_slice(&count.to_be_bytes());
        for _ in 0..count {
            p.extend_from_slice(&each.to_be_bytes());
        }
        bx(b"trun", &p)
    }

    fn write_tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    /// 渐进式 MP4：`mvhd.duration` 直接给出答案
    #[test]
    fn mp4_duration_progressive() {
        let moov = bx(b"moov", &mvhd_v0(1000, 2500)); // 2.5s
        let ftyp = bx(b"ftyp", b"mp42\0\0\0\0mp42");
        let mdat = bx(b"mdat", &vec![0u8; 4096]);
        let p = write_tmp("dw_prog.mp4", &cat(&[ftyp, mdat, moov]));
        assert_eq!(super::mp4_duration_secs(p.to_str().unwrap()), Some(2));
    }

    /// fMP4（`mvhd.duration` 恒为 0）：分轨累加 `moof` 的样本时长。
    ///
    /// 关键点：视频轨 90000 时基、音频轨 44100 时基，**必须各除各的**。
    /// 曾经把两轨 tick 相加再除以影片 timescale(1000)，5.3 秒的片子算成 81 秒。
    #[test]
    fn mp4_duration_fragmented_per_track() {
        let moov = bx(
            b"moov",
            &cat(&[
                mvhd_v0(1000, 0),
                trak_v0(1, 90_000),  // 视频轨
                trak_v0(2, 44_100),  // 音频轨
                bx(b"mvex", &[]),
            ]),
        );
        // 每个分片：视频 20 × 4500 = 90000 ticks（1s），两个分片合计 2.0s
        let v_traf = bx(b"traf", &cat(&[tfhd_v0(1, 0), trun_v0(20, 4500)]));
        // 每个分片：音频 22 × 1024 = 22528 / 44100 ≈ 0.51s，合计 ≈1.02s（应被忽略，取最长轨）
        let a_traf = bx(b"traf", &cat(&[tfhd_v0(2, 0), trun_v0(22, 1024)]));
        let moof = bx(b"moof", &cat(&[v_traf, a_traf]));
        let ftyp = bx(b"ftyp", b"iso5\0\0\0\0iso5");
        let mdat = bx(b"mdat", &vec![0u8; 1024]);
        let bytes = cat(&[ftyp, moov, moof.clone(), mdat.clone(), moof, mdat]);
        let p = write_tmp("dw_frag.mp4", &bytes);
        assert_eq!(super::mp4_duration_secs(p.to_str().unwrap()), Some(2));
    }

    /// 走 tfhd 默认值（trun 不逐条给时长）也要算对
    #[test]
    fn mp4_duration_fragmented_default_duration() {
        let moov = bx(b"moov", &cat(&[mvhd_v0(1000, 0), trak_v0(1, 1000)]));
        let mut trun_payload = Vec::new();
        trun_payload.extend_from_slice(&0u32.to_be_bytes()); // 无标志位
        trun_payload.extend_from_slice(&3u32.to_be_bytes()); // 3 个样本，全走默认 1000
        let trun = bx(b"trun", &trun_payload);
        let moof = bx(b"moof", &bx(b"traf", &cat(&[tfhd_v0(1, 1000), trun])));
        let ftyp = bx(b"ftyp", b"iso5\0\0\0\0iso5");
        let bytes = cat(&[ftyp, moov, moof]);
        let p = write_tmp("dw_frag2.mp4", &bytes);
        assert_eq!(super::mp4_duration_secs(p.to_str().unwrap()), Some(3));
    }

    /// 不是 MP4（或长度字段被写坏）必须安全返回 None，不能 panic、不能死循环
    #[test]
    fn mp4_duration_garbage() {
        let p = write_tmp("dw_garbage.mp4", &[0xFFu8; 64]);
        assert_eq!(super::mp4_duration_secs(p.to_str().unwrap()), None);
        let p2 = write_tmp("dw_short.mp4", &[0u8; 3]);
        assert_eq!(super::mp4_duration_secs(p2.to_str().unwrap()), None);
        assert_eq!(super::mp4_duration_secs("C:\\definitely\\missing.mp4"), None);
    }
}
