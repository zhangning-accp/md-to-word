# 墨转 Word

用 Rust 编写的轻量桌面工具，将 Markdown 转为 `.docx`，重点支持数学教学文档中的可编辑公式。Windows 使用 Win32 原生控件，macOS 使用 AppKit 原生控件。

v3 新增粘贴 Markdown 文字生成 Word。正文中的每一次手动换行都会保留到 Word 中；空行仍分段，无需在行尾添加两个空格。本仓库只保存源码和示例，不在提交后自动打包或发布可执行文件。

## 直接使用

Windows：在本机双击 `dist/墨转 Word.exe`。可选择或拖入 Markdown 文件；也可切换到“粘贴文字”，在多行框中直接粘贴 Markdown。点击“生成 Word 文档”时，如尚未设置保存位置，会出现保存窗口。完成后可点击“打开生成文件”。克隆本仓库的用户可按下文从源码自行构建。

macOS：源码界面也支持通过“粘贴文字”输入 Markdown；需要时在 Mac 上自行构建。Apple 芯片要求 macOS 11.0 或更新，Intel 芯片要求 macOS 10.14 或更新。

转换器不需要安装 Word、Pandoc、Python、Node.js 或浏览器运行环境。查看或编辑生成的文件需要兼容 `.docx` 的软件。

## 支持内容

- 标题、中文正文、加粗、斜体、删除线、引用、分隔线。
- 原文单次换行直接转换成 Word 换行，空行按段落保留。
- 有序列表、无序列表、嵌套列表、任务勾选框、超链接。
- Markdown 表格及列对齐、代码块、行内代码。
- 使用相对路径或绝对路径引用的本地 PNG、JPG 图片；重复图片只存一份。粘贴文字时，相对图片路径以 Word 保存文件夹为基准。
- 行内和独立公式，支持 `$…$`、`$$…$$`、`\(…\)`、`\[…\]`。
- 常用公式包括分数、根号、上下标、希腊字母、比较符号、求和、积分、矩阵、数学字体、文本和单位。

公式写成 Word 的 OMML 结构，可以在 Word 中继续编辑。不同办公软件和字体可能显示出排版差异。

## 范围与限制

第一版支持常用 LaTeX 数学语法，不能完整替代 LaTeX 排版系统。自定义宏、复杂环境及未支持的命令会保留为 LaTeX 原文，并显示提示；代码块中的公式定界符不会被转换。提示最多展示前 20 条。

HTML 以原始文本保留。远程图片、SVG、Mermaid、脚注排版、自定义 Word 模板和批量转换尚未实现。远程图片引用会保留并提示，不会自动下载。

源文件使用 UTF-8 编码。为控制内存，单个 Markdown 文件上限为 32 MiB；单个公式上限约 16 KiB，另有嵌套深度限制。目标文件先写入同目录临时文件，成功后才替换正式文件。覆盖已有文件前，桌面界面会要求确认。

## 内存设计

窗口空闲时等待系统消息，无持续刷新动画，也不保留文档预览。Markdown 事件直接写入压缩的 Word 文件，不在内存中构建整份 Word 文档树。图片只读取尺寸信息，文件内容按流复制，不解码成大幅位图。每次只转换一个文档。

解析器仍需读取整个 Markdown 源文件；粘贴模式还需让系统文字框持有输入内容，内存会随文字长度、列表及链接数量增加。输入上限为 32 MiB；Windows 粘贴框最多接受约 800 万个 UTF-16 字符。上述限制不是内存上限。实测数据和测量口径见 `PERFORMANCE.md`。

## 从源码构建

需要 Rust 1.86 或更新版本，第一次构建需要下载依赖。Windows 需要 Visual Studio C++ 构建工具，Mac 需要 Xcode Command Line Tools。

```sh
cargo build --release --locked
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

Windows 生成：`target/release/md-to-word.exe`。

Mac 打包，在 Mac 电脑上执行：

```sh
# Apple 芯片
rustup target add aarch64-apple-darwin
sh scripts/package-macos.sh aarch64-apple-darwin

# Intel 芯片
rustup target add x86_64-apple-darwin
sh scripts/package-macos.sh x86_64-apple-darwin
```

打包脚本只在个人 Mac 上手动运行时生成文件。GitHub 仓库没有自动打包工作流，也不上传 Windows 或 Mac 的执行程序。本机 Windows 图标资源可用 `python scripts/generate-icon.py` 重新生成；只有重新制作图标时才需要 Pillow，使用程序不需要 Python。

脚本使用本地临时签名，没有 Apple Developer ID 签名与公证；在普通 Mac 上首次打开时可能出现系统安全提示。当前尚未在 Mac 桌面上手动验证窗口操作；正式对外分发前仍需完成桌面实测与 Apple 签名公证。

命令行转换：

```sh
md-to-word --convert input.md output.docx
md-to-word --convert input.md output.docx --force
```

命令行默认不覆盖已有文件；`--force` 表示允许替换。

Windows 性能测试：

```powershell
./scripts/measure-windows.ps1
```

## 代码入口

- `src/convert.rs`：Markdown 事件转换、Word 文件流式写入、图片和列表。
- `src/math.rs`：LaTeX 校验、数学文本与符号处理、MathML 到 OMML。
- `src/windows.rs`、`src/macos.rs`：系统原生窗口。
- `tests/conversion.rs`：完整文件、公式回退、列表、图片、覆盖保护等测试。
- `examples/数学示例.md`：可用于核对常见排版和公式的示例。

项目代码采用 MIT 许可证。第三方依赖各自的许可信息见 `THIRD_PARTY_LICENSES.txt`。
