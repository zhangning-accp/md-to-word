#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

fn main() {
    let mut args = std::env::args_os().skip(1);
    if let Some(first) = args.next() {
        if first == "--convert" {
            let result = (|| {
                let input = PathBuf::from(args.next().ok_or("缺少 Markdown 文件路径")?);
                let output = PathBuf::from(args.next().ok_or("缺少 Word 文件路径")?);
                let remaining: Vec<_> = args.collect();
                if remaining.iter().any(|a| a != "--force") {
                    return Err("未知参数；可选参数为 --force".into());
                }
                md_to_word::convert::convert(
                    &input,
                    &output,
                    remaining.iter().any(|a| a == "--force"),
                )
            })();
            match result {
                Ok(report) => {
                    println!("{}", report.summary());
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
            return;
        }
        if first == "--help" || first == "-h" {
            println!(
                "墨转 Word\n双击打开桌面窗口。\n命令行：md-to-word --convert 输入.md 输出.docx [--force]"
            );
            return;
        }
        #[cfg(windows)]
        windows::run(Some(PathBuf::from(first)));
        #[cfg(target_os = "macos")]
        macos::run(Some(PathBuf::from(first)));
        return;
    }
    #[cfg(windows)]
    windows::run(None);
    #[cfg(target_os = "macos")]
    macos::run(None);
    #[cfg(not(any(windows, target_os = "macos")))]
    eprintln!("桌面界面支持 Windows 和 macOS；此平台请使用 --convert");
}
