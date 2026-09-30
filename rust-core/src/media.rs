// 媒体扫描 / 校验 / 导入 / 删除。
// 首版仅支持本地静态图片与 MP4/MOV 视频；GIF、动态 HEIC、网页等一律拒绝。

use std::path::{Path, PathBuf};

use crate::settings;
use crate::types::MediaKind;

const SUPPORTED_IMAGE_EXTS: [&str; 6] = ["jpg", "jpeg", "png", "bmp", "webp", "heic"];
const SUPPORTED_VIDEO_EXTS: [&str; 2] = ["mp4", "mov"];

pub fn kind_of_ext(ext: &str) -> Option<MediaKind> {
    if SUPPORTED_IMAGE_EXTS
        .iter()
        .any(|s| s.eq_ignore_ascii_case(ext))
    {
        Some(MediaKind::Image)
    } else if SUPPORTED_VIDEO_EXTS
        .iter()
        .any(|s| s.eq_ignore_ascii_case(ext))
    {
        Some(MediaKind::Video)
    } else {
        None
    }
}

pub fn classify(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_str()?;
    kind_of_ext(ext)
}

fn canonical_dir(dir: &str) -> Option<PathBuf> {
    let p = PathBuf::from(dir.trim());
    let c = p.canonicalize().ok()?;
    c.is_dir().then_some(c)
}

/// 路径是否位于授权的壁纸目录（媒体库）内。删除与应用共用同一边界。
pub fn ensure_within_library(path: &Path) -> Result<(), String> {
    let library =
        canonical_dir(&settings::get().library_dir).ok_or_else(|| "壁纸目录不可用".to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "无法解析文件所在目录".to_string())?;
    let canon_parent = parent
        .canonicalize()
        .map_err(|_| "文件所在目录不存在".to_string())?;
    if canon_parent.starts_with(&library) {
        Ok(())
    } else {
        Err(format!("路径超出授权壁纸目录: {}", path.display()))
    }
}

/// 应用壁纸前的完整校验：规范化、存在性、文件类型与授权目录。
/// 返回规范化后的路径字符串。
pub fn validate_for_apply(path: &str, declared: MediaKind) -> Result<String, String> {
    let raw = PathBuf::from(path.trim());
    let canon = raw
        .canonicalize()
        .map_err(|_| format!("媒体文件不存在: {path}"))?;
    if !canon.is_file() {
        return Err(format!("不是文件: {}", canon.display()));
    }
    let actual = classify(&canon).ok_or_else(|| "不支持的媒体格式".to_string())?;
    if actual != declared {
        return Err(format!(
            "媒体类型不匹配: 声明 {declared:?}，实际 {actual:?}"
        ));
    }
    ensure_within_library(&canon)?;
    Ok(canon.to_string_lossy().to_string())
}

/// 扫描目录（递归），返回 (绝对路径, 类型) 列表。
pub fn scan(dir: &str) -> Result<Vec<(String, MediaKind)>, String> {
    let trimmed = dir.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let root = PathBuf::from(trimmed);
    if !root.exists() {
        return Err(format!("壁纸目录不存在或已被移动: {trimmed}"));
    }
    if !root.is_dir() {
        return Err(format!("壁纸目录不是文件夹: {trimmed}"));
    }
    let root = root
        .canonicalize()
        .map_err(|e| format!("壁纸目录无法解析: {trimmed}: {e}"))?;
    let mut out = Vec::new();
    let mut stack = vec![root];
    let mut is_root = true;
    while let Some(current) = stack.pop() {
        let at_root = is_root;
        is_root = false;
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(e) => {
                if at_root {
                    return Err(format!("无法读取壁纸目录 {}: {e}", current.display()));
                }
                continue;
            }
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                if let Some(kind) = classify(&path) {
                    if let Some(s) = path.to_str() {
                        out.push((s.to_string(), kind));
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| {
        let (ka, kb) = (a.0.to_lowercase(), b.0.to_lowercase());
        ka.cmp(&kb).then_with(|| a.0.cmp(&b.0))
    });
    Ok(out)
}

/// 导入拖入/选择的文件到壁纸目录：重名自动加序号，未支持格式进 skipped。
pub fn import(paths: &[String]) -> (Vec<String>, Vec<String>) {
    let library = canonical_dir(&settings::get().library_dir);
    let Some(save_dir) = library else {
        return (
            Vec::new(),
            paths
                .iter()
                .map(|p| format!("{p}: 壁纸目录不可用"))
                .collect(),
        );
    };

    let mut saved = Vec::new();
    let mut skipped = Vec::new();
    for p in paths {
        let src = match PathBuf::from(p.trim()).canonicalize() {
            Ok(c) => c,
            Err(_) => {
                skipped.push(format!("{p}: 文件不存在"));
                continue;
            }
        };
        if !src.is_file() {
            skipped.push(format!("{}: 不是文件", src.display()));
            continue;
        }
        if classify(&src).is_none() {
            skipped.push(format!(
                "{}: 不支持的格式",
                src.file_name().unwrap_or_default().to_string_lossy()
            ));
            continue;
        }
        let name = src
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let ext = name.rsplit('.').next().unwrap_or_default();
        let mut dest = save_dir.join(&name);
        let mut idx = 1u32;
        while dest.exists() {
            let stem = src
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "media".to_string());
            dest = save_dir.join(format!("{stem}_{idx}.{ext}"));
            idx += 1;
        }
        match std::fs::copy(&src, &dest) {
            Ok(_) => saved.push(dest.to_string_lossy().to_string()),
            Err(e) => skipped.push(format!("{name}: 复制失败 {e}")),
        }
    }
    (saved, skipped)
}

/// 删除媒体文件：仅允许作用于壁纸目录内的受支持文件。
pub fn delete(path: &str) -> Result<(), String> {
    let canon = PathBuf::from(path.trim())
        .canonicalize()
        .map_err(|_| "文件不存在或已被移动".to_string())?;
    if canon.is_dir() {
        return Err("不能删除目录".to_string());
    }
    ensure_within_library(&canon)?;
    if classify(&canon).is_none() {
        return Err("不支持的媒体文件类型".to_string());
    }
    let canon_str = canon.to_string_lossy().to_string();
    if crate::engine::is_path_in_use(&canon_str) {
        return Err("该文件正被某块显示器用作壁纸，请先停止或切换后再删除".to_string());
    }
    if crate::engine::is_path_displayed(&canon_str) {
        return Err("该文件仍是某块显示器当前展示的壁纸内容（视频停止后桌面留的是它的首帧海报，删文件会连海报一起清掉），请先在该显示器换用其他壁纸；若显示器已拔出，右侧状态里的「解除占用」就是停用".to_string());
    }
    let used_by_assignment = settings::get().assignments.values().any(|a| {
        PathBuf::from(&a.path)
            .canonicalize()
            .is_ok_and(|p| p == canon)
    });
    if used_by_assignment {
        return Err("该文件仍是某块显示器记住的壁纸配置，请先在该显示器换用其他壁纸（或点「解除占用」）后再删除".to_string());
    }
    std::fs::remove_file(&canon).map_err(|e| format!("删除失败: {e}"))?;
    if let Ok(cache) = crate::thumbs::cache_dir() {
        crate::thumbs::delete_thumb(&canon.to_string_lossy(), &cache);
    }
    Ok(())
}
