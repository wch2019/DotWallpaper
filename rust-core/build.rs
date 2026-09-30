fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // 显式链接系统框架（不随应用打包）
        for framework in ["ImageIO", "CoreMedia", "CoreGraphics", "AVFoundation"] {
            println!("cargo:rustc-link-framework={framework}");
        }
    }
}
