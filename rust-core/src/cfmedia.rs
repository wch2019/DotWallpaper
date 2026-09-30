// 直接声明 CoreFoundation / ImageIO 的 C 接口，用于缩略图与 JPEG 编码。
// 不引入 Rust 编解码库；系统框架不随应用打包。全部为纯 C 函数，线程安全。

#![allow(non_upper_case_globals, non_snake_case, dead_code)]

use std::ffi::c_void;

pub type CFTypeRef = *const c_void;

const kCFStringEncodingUTF8: u32 = 0x0800_0100;
const kCFNumberFloat64Type: u32 = 6;
const kCFURLPOSIXPathStyle: u32 = 0;

#[repr(C)]
pub struct OpaqueCallbacks([u8; 0]);

extern "C" {
    fn CFStringCreateWithCString(alloc: CFTypeRef, cStr: *const u8, encoding: u32) -> *mut c_void;
    fn CFURLCreateWithFileSystemPath(
        alloc: CFTypeRef,
        filePath: CFTypeRef,
        pathStyle: u32,
        isDirectory: bool,
    ) -> *mut c_void;
    fn CFDictionaryCreate(
        alloc: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        numValues: i64,
        callBacks: *const c_void,
        valueCallBacks: *const c_void,
    ) -> *mut c_void;
    fn CFNumberCreate(alloc: CFTypeRef, theType: u32, valuePtr: *const c_void) -> *mut c_void;
    fn CFRelease(cf: *mut c_void);
    static kCFTypeDictionaryKeyCallBacks: OpaqueCallbacks;
    static kCFTypeDictionaryValueCallBacks: OpaqueCallbacks;
    static kCFBooleanTrue: CFTypeRef;

    fn CGImageSourceCreateWithURL(imageURL: CFTypeRef, options: CFTypeRef) -> *mut c_void;
    fn CGImageSourceCopyPropertiesAtIndex(
        src: CFTypeRef,
        index: usize,
        options: CFTypeRef,
    ) -> *mut c_void;
    fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> *const c_void;
    fn CFNumberGetValue(number: CFTypeRef, theType: u32, valuePtr: *mut c_void) -> u8;
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CGImageSourceCreateThumbnailAtIndex(
        src: CFTypeRef,
        index: usize,
        options: CFTypeRef,
    ) -> *mut c_void;
    fn CGImageDestinationCreateWithURL(
        url: CFTypeRef,
        type_: CFTypeRef,
        count: usize,
        options: CFTypeRef,
    ) -> *mut c_void;
    fn CGImageDestinationAddImage(dst: CFTypeRef, image: CFTypeRef, properties: CFTypeRef);
    fn CGImageDestinationFinalize(dst: CFTypeRef) -> u8;
}

fn c_string(s: &str) -> *mut c_void {
    let mut bytes = s.as_bytes().to_vec();
    bytes.push(0);
    unsafe { CFStringCreateWithCString(std::ptr::null(), bytes.as_ptr(), kCFStringEncodingUTF8) }
}

fn url_for(path: &str) -> *mut c_void {
    let s = c_string(path);
    let url = unsafe {
        CFURLCreateWithFileSystemPath(
            std::ptr::null(),
            s as *const c_void,
            kCFURLPOSIXPathStyle,
            false,
        )
    };
    unsafe { CFRelease(s) };
    url
}

fn dictionary(pairs: &[(&str, *const c_void)]) -> *mut c_void {
    let keys: Vec<*mut c_void> = pairs.iter().map(|(k, _)| c_string(k)).collect();
    let values: Vec<*const c_void> = pairs.iter().map(|(_, v)| *v).collect();
    let dict = unsafe {
        CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr() as *const *const c_void,
            values.as_ptr(),
            pairs.len() as i64,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        )
    };
    for k in keys {
        unsafe { CFRelease(k) };
    }
    dict
}

fn number_f64(v: f64) -> *mut c_void {
    unsafe {
        CFNumberCreate(
            std::ptr::null(),
            kCFNumberFloat64Type,
            &v as *const _ as *const c_void,
        )
    }
}

/// 只读文件属性拿像素尺寸，不解码位图。
///
/// 这里曾经用 `CGImageSourceCreateImageAtIndex` 取 `CGImageGetWidth`——那会把整幅图解码一遍
/// 只为问一个尺寸，于是每张稍大的图都要「全解码 + 再生成缩略图」两次，直接顶到
/// 「约 1000 张首屏 ≤ 2s」这条验收上。属性里的宽高不含 EXIF 旋转，但唯一调用方
/// （`thumbs.rs` 判断是否小图）用的是 `max(w, h)`，旋转与否不影响结论。
pub fn image_size(path: &str) -> Option<(usize, usize)> {
    const K_CF_NUMBER_SINT64_TYPE: u32 = 4;
    unsafe {
        let url = url_for(path);
        let src = CGImageSourceCreateWithURL(url as *const c_void, std::ptr::null());
        CFRelease(url);
        if src.is_null() {
            return None;
        }
        let props = CGImageSourceCopyPropertiesAtIndex(src as *const c_void, 0, std::ptr::null());
        CFRelease(src);
        if props.is_null() {
            return None;
        }
        // 属性字典的键就是这些常量字符串本身（kCGImagePropertyPixelWidth == "PixelWidth"），
        // 按 CFEqual 值比较命中；取到的是借用指针，不能释放。
        let read_dim = |key: &str| -> Option<usize> {
            let k = c_string(key);
            let v = CFDictionaryGetValue(props as *const c_void, k as *const c_void);
            CFRelease(k);
            if v.is_null() || CFGetTypeID(v) != CFNumberGetTypeID() {
                return None;
            }
            let mut n: i64 = 0;
            if CFNumberGetValue(v, K_CF_NUMBER_SINT64_TYPE, &mut n as *mut _ as *mut c_void) == 0 {
                return None;
            }
            (n > 0).then_some(n as usize)
        };
        let size = match (read_dim("PixelWidth"), read_dim("PixelHeight")) {
            (Some(w), Some(h)) => Some((w, h)),
            _ => None,
        };
        CFRelease(props);
        size
    }
}

/// 将 CGImage（不透明指针）编码为 JPEG 文件。
///
/// # Safety
/// `cg_image` must be a valid, retained `CGImageRef` pointer for the duration
/// of this call.
pub unsafe fn encode_jpeg(
    cg_image: *const c_void,
    dst_path: &str,
    quality: f64,
) -> Result<(), String> {
    if cg_image.is_null() {
        return Err("CGImage 为空".to_string());
    }
    unsafe {
        let url = url_for(dst_path);
        let kind = c_string("public.jpeg");
        let dst = CGImageDestinationCreateWithURL(
            url as *const c_void,
            kind as *const c_void,
            1,
            std::ptr::null(),
        );
        CFRelease(url);
        CFRelease(kind);
        if dst.is_null() {
            return Err("无法创建 JPEG 编码目标".to_string());
        }
        let q = number_f64(quality);
        let props = dictionary(&[(
            "kCGImageDestinationLossyCompressionQuality",
            q as *const c_void,
        )]);
        CFRelease(q);
        CGImageDestinationAddImage(dst as *const c_void, cg_image, props as *const c_void);
        if !props.is_null() {
            CFRelease(props);
        }
        let ok = CGImageDestinationFinalize(dst as *const c_void);
        CFRelease(dst);
        if ok == 0 {
            return Err("JPEG 编码失败".to_string());
        }
    }
    Ok(())
}

/// 从源图片文件生成等比缩略图 JPEG（最长边 max_px，自动应用 EXIF 方向）。
pub fn make_thumbnail_jpeg(src_path: &str, dst_path: &str, max_px: u32) -> Result<(), String> {
    unsafe {
        let url = url_for(src_path);
        let src = CGImageSourceCreateWithURL(url as *const c_void, std::ptr::null());
        CFRelease(url);
        if src.is_null() {
            return Err("ImageIO 无法读取图片".to_string());
        }
        // 键由 `dictionary` 从 `&str` 现场构造，这里不再自己建一遍 CFString
        let max_num = number_f64(max_px as f64);
        let opts = dictionary(&[
            (
                "kCGImageSourceCreateThumbnailFromImageAlways",
                kCFBooleanTrue,
            ),
            ("kCGImageSourceCreateThumbnailWithTransform", kCFBooleanTrue),
            (
                "kCGImageSourceThumbnailMaxPixelSize",
                max_num as *const c_void,
            ),
        ]);
        let thumb =
            CGImageSourceCreateThumbnailAtIndex(src as *const c_void, 0, opts as *const c_void);
        CFRelease(opts);
        CFRelease(max_num);
        if thumb.is_null() {
            CFRelease(src);
            return Err("缩略图生成失败".to_string());
        }
        let result = encode_jpeg(thumb as *const c_void, dst_path, 0.85);
        CFRelease(thumb);
        CFRelease(src);
        result
    }
}
