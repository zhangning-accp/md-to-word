//! Native Win32 controls: no web view, GPU renderer, or polling when idle.
#![allow(unsafe_op_in_unsafe_fn)]
use md_to_word::convert;
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::{null, null_mut},
    sync::mpsc::{self, Receiver},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{
        LibraryLoader::GetModuleHandleW,
        SystemServices::{SS_ENDELLIPSIS, SS_RIGHT},
    },
    UI::{
        Controls::{DRAWITEMSTRUCT, Dialogs::*, EM_SETLIMITTEXT, ShowScrollBar, WM_MOUSELEAVE},
        HiDpi::*,
        Input::KeyboardAndMouse::*,
        Shell::*,
        WindowsAndMessaging::*,
    },
};
mod view;

const SELECT: usize = 101;
const SAVE: usize = 102;
const CONVERT: usize = 103;
const OPEN: usize = 104;
const FILE_TAB: usize = 105;
const TEXT_TAB: usize = 106;
const TEXT_INPUT: usize = 107;
const FINISHED: u32 = WM_APP + 1;
#[derive(Clone, Copy, PartialEq, Eq)]
enum InputMode {
    File,
    Text,
}
enum Source {
    File(PathBuf),
    Text(String),
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn path_wide(p: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

struct App {
    mode: InputMode,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    input_label: HWND,
    input_name: HWND,
    file_hint: HWND,
    text_input: HWND,
    output_label: HWND,
    status_title: HWND,
    status: HWND,
    select: HWND,
    file_tab: HWND,
    text_tab: HWND,
    save: HWND,
    convert: HWND,
    open: HWND,
    font: HFONT,
    title_font: HFONT,
    heading_font: HFONT,
    small_font: HFONT,
    background: HBRUSH,
    surface: HBRUSH,
    scale: f64,
    hover: HWND,
    status_color: COLORREF,
    worker: Option<Receiver<convert::Result<convert::Report>>>,
}
impl App {
    unsafe fn pasted_text(&self) -> convert::Result<String> {
        let length = GetWindowTextLengthW(self.text_input);
        if length <= 0 {
            return Err("请先粘贴或输入 Markdown 内容".into());
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let count = GetWindowTextW(self.text_input, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16(&buffer[..count as usize])
            .map_err(|_| "粘贴的文字包含无效字符，请检查后重试".into())
    }
    unsafe fn selected(&mut self, path: PathBuf) {
        self.set_mode(InputMode::File);
        let output = path.with_extension("docx");
        self.input = Some(path);
        self.output = Some(output);
        SetWindowTextW(
            self.input_name,
            path_wide(std::path::Path::new(
                self.input.as_ref().unwrap().file_name().unwrap_or_default(),
            ))
            .as_ptr(),
        );
        SetWindowTextW(
            self.input_label,
            path_wide(self.input.as_ref().unwrap()).as_ptr(),
        );
        SetWindowTextW(
            self.output_label,
            path_wide(self.output.as_ref().unwrap()).as_ptr(),
        );
        self.show_status(
            "准备就绪",
            "原文换行和数学公式将保留到 Word 中。",
            view::ACCENT,
        );
        EnableWindow(self.convert, 1);
        EnableWindow(self.open, 0);
    }
    unsafe fn set_mode(&mut self, mode: InputMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        let file = mode == InputMode::File;
        for handle in [
            self.input_name,
            self.input_label,
            self.file_hint,
            self.select,
        ] {
            ShowWindow(handle, if file { SW_SHOW } else { SW_HIDE });
        }
        ShowWindow(self.text_input, if file { SW_HIDE } else { SW_SHOW });
        if file {
            self.output = self.input.as_ref().map(|p| p.with_extension("docx"));
            if let Some(path) = &self.output {
                SetWindowTextW(self.output_label, path_wide(path).as_ptr());
            } else {
                SetWindowTextW(
                    self.output_label,
                    wide("选择文件后，默认保存在源文件旁边").as_ptr(),
                );
            }
            self.show_status("文件模式", "选择或拖入 Markdown 文件。", view::ACCENT);
        } else {
            self.output = None;
            SetWindowTextW(self.output_label, wide("生成时选择 Word 保存位置").as_ptr());
            self.show_status(
                "文字模式",
                "在上方粘贴 Markdown，点击生成即可保存 Word。",
                view::ACCENT,
            );
            SetFocus(self.text_input);
        }
        EnableWindow(
            self.convert,
            (file && self.input.is_some() || !file && GetWindowTextLengthW(self.text_input) > 0)
                as i32,
        );
        EnableWindow(self.open, 0);
        InvalidateRect(self.file_tab, null(), 1);
        InvalidateRect(self.text_tab, null(), 1);
    }
    unsafe fn busy(&self, busy: bool) {
        EnableWindow(self.select, (!busy) as i32);
        EnableWindow(self.file_tab, (!busy) as i32);
        EnableWindow(self.text_tab, (!busy) as i32);
        EnableWindow(self.text_input, (!busy) as i32);
        EnableWindow(self.save, (!busy) as i32);
        let has_input = match self.mode {
            InputMode::File => self.input.is_some(),
            InputMode::Text => GetWindowTextLengthW(self.text_input) > 0,
        };
        EnableWindow(self.convert, (!busy && has_input) as i32);
        SetWindowTextW(
            self.convert,
            wide(if busy {
                "正在生成…"
            } else {
                "生成 Word 文档"
            })
            .as_ptr(),
        );
    }
    unsafe fn show_status(&mut self, title: &str, text: &str, color: COLORREF) {
        self.status_color = color;
        SetWindowTextW(self.status_title, wide(title).as_ptr());
        SetWindowTextW(self.status, wide(&text.replace('\n', "\r\n")).as_ptr());
        ShowScrollBar(self.status, SB_VERT, (text.lines().count() > 3) as i32);
        InvalidateRect(GetParent(self.status), null(), 0);
    }
}
// Thin wrapper around CreateWindowExW; keep its native geometry arguments together.
#[allow(clippy::too_many_arguments)]
unsafe fn control(
    parent: HWND,
    class: &str,
    text: &str,
    style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: usize,
    scale: f64,
    font: HFONT,
) -> HWND {
    let v = |n: i32| (n as f64 * scale).round() as i32;
    let handle = CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(text).as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        v(x),
        v(y),
        v(w),
        v(h),
        parent,
        id as *mut c_void,
        GetModuleHandleW(null()),
        null(),
    );
    SendMessageW(handle, WM_SETFONT, font as usize, 1);
    handle
}
unsafe fn dialog(hwnd: HWND, save: bool, default: Option<&std::path::Path>) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = vec![0u16; 32768];
    if let Some(path) = default {
        let s = path_wide(path);
        let len = s.len().min(buffer.len());
        buffer[..len].copy_from_slice(&s[..len]);
    }
    let filter = wide(if save {
        "Word 文档 (*.docx)\0*.docx\0\0"
    } else {
        "Markdown 文件 (*.md;*.markdown)\0*.md;*.markdown\0所有文件\0*.*\0\0"
    });
    let title = wide(if save {
        "保存 Word 文档"
    } else {
        "选择 Markdown 文件"
    });
    let ext = wide("docx");
    let mut ofn: OPENFILENAMEW = zeroed();
    ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = hwnd;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = buffer.as_mut_ptr();
    ofn.nMaxFile = buffer.len() as u32;
    ofn.lpstrTitle = title.as_ptr();
    ofn.lpstrDefExt = if save { ext.as_ptr() } else { null() };
    ofn.Flags = OFN_EXPLORER
        | OFN_PATHMUSTEXIST
        | OFN_NOCHANGEDIR
        | if save { 0 } else { OFN_FILEMUSTEXIST };
    let success = if save {
        GetSaveFileNameW(&mut ofn)
    } else {
        GetOpenFileNameW(&mut ofn)
    };
    if success != 0 {
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len])))
    } else {
        None
    }
}
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if !ptr.is_null() {
        let app = &mut *ptr;
        match msg {
            WM_COMMAND => {
                match w & 0xffff {
                    FILE_TAB if app.worker.is_none() => app.set_mode(InputMode::File),
                    TEXT_TAB if app.worker.is_none() => app.set_mode(InputMode::Text),
                    TEXT_INPUT if (w >> 16) as u32 == EN_CHANGE && app.worker.is_none() => {
                        if app.mode == InputMode::Text {
                            EnableWindow(
                                app.convert,
                                (GetWindowTextLengthW(app.text_input) > 0) as i32,
                            );
                            EnableWindow(app.open, 0);
                        }
                    }
                    SELECT if app.worker.is_none() => {
                        if let Some(p) = dialog(hwnd, false, None) {
                            app.selected(p);
                        }
                    }
                    SAVE if app.worker.is_none() => {
                        let default = app.output.as_deref().or_else(|| {
                            (app.mode == InputMode::Text)
                                .then_some(std::path::Path::new("未命名文档.docx"))
                        });
                        if let Some(p) = dialog(hwnd, true, default) {
                            app.output = Some(p);
                            EnableWindow(app.open, 0);
                            app.show_status(
                                "保存位置已更新",
                                "点击“生成 Word 文档”开始转换。",
                                view::ACCENT,
                            );
                            SetWindowTextW(
                                app.output_label,
                                path_wide(app.output.as_ref().unwrap()).as_ptr(),
                            );
                        }
                    }
                    CONVERT if app.worker.is_none() => {
                        let source = match app.mode {
                            InputMode::File => match app.input.clone() {
                                Some(path) => Source::File(path),
                                None => return 0,
                            },
                            InputMode::Text => match app.pasted_text() {
                                Ok(text) => Source::Text(text),
                                Err(error) => {
                                    app.show_status("请检查粘贴内容", &error, view::ERROR);
                                    return 0;
                                }
                            },
                        };
                        let output = match app.output.clone().or_else(|| {
                            dialog(hwnd, true, Some(std::path::Path::new("未命名文档.docx")))
                        }) {
                            Some(path) => path,
                            None => return 0,
                        };
                        app.output = Some(output.clone());
                        SetWindowTextW(app.output_label, path_wide(&output).as_ptr());
                        if output.exists() {
                            let message =
                                format!("文件已存在：\n{}\n\n是否替换？", output.display());
                            if MessageBoxW(
                                hwnd,
                                wide(&message).as_ptr(),
                                wide("保存 Word 文档").as_ptr(),
                                MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
                            ) != IDYES
                            {
                                return 0;
                            }
                        }
                        let (tx, rx) = mpsc::channel();
                        app.worker = Some(rx);
                        app.busy(true);
                        EnableWindow(app.open, 0);
                        app.show_status(
                            "正在生成 Word",
                            "正在处理正文、公式和图片，请稍候。",
                            view::ACCENT,
                        );
                        let handle = hwnd as isize;
                        std::thread::spawn(move || {
                            let result = match source {
                                Source::File(input) => convert::convert(&input, &output, true),
                                Source::Text(text) => convert::convert_text(&text, &output, true),
                            };
                            let _ = tx.send(result);
                            PostMessageW(handle as HWND, FINISHED, 0, 0);
                        });
                    }
                    OPEN => {
                        if let Some(path) = &app.output {
                            ShellExecuteW(
                                hwnd,
                                wide("open").as_ptr(),
                                path_wide(path).as_ptr(),
                                null(),
                                null(),
                                SW_SHOWNORMAL,
                            );
                        }
                    }
                    _ => {}
                }
                return 0;
            }
            FINISHED => {
                if let Some(rx) = app.worker.take() {
                    let result = rx
                        .try_recv()
                        .unwrap_or_else(|_| Err("转换任务意外结束，请重试".into()));
                    match result {
                        Ok(report) => {
                            app.show_status(
                                if report.warning_count > 0 {
                                    "转换完成，请检查提示"
                                } else {
                                    "转换完成"
                                },
                                &report.summary(),
                                if report.warning_count > 0 {
                                    view::WARNING
                                } else {
                                    view::SUCCESS
                                },
                            );
                            EnableWindow(app.open, 1);
                        }
                        Err(e) => {
                            app.show_status("未能完成转换", &e, view::ERROR);
                        }
                    }
                    app.busy(false);
                }
                return 0;
            }
            WM_DROPFILES => {
                let drop = w as HDROP;
                if app.worker.is_none() {
                    let len = DragQueryFileW(drop, 0, null_mut(), 0);
                    let mut path = vec![0u16; len as usize + 1];
                    DragQueryFileW(drop, 0, path.as_mut_ptr(), path.len() as u32);
                    use std::os::windows::ffi::OsStringExt;
                    let path = PathBuf::from(std::ffi::OsString::from_wide(&path[..len as usize]));
                    app.selected(path);
                }
                DragFinish(drop);
                return 0;
            }
            WM_CTLCOLORSTATIC => {
                return view::control_color(app, w as HDC, l as HWND);
            }
            WM_DRAWITEM => {
                view::draw_button(app, &*(l as *const DRAWITEMSTRUCT));
                return 1;
            }
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                let mut paint: PAINTSTRUCT = zeroed();
                let dc = BeginPaint(hwnd, &mut paint);
                view::paint(hwnd, app, dc);
                EndPaint(hwnd, &paint);
                return 0;
            }
            WM_CLOSE if app.worker.is_some() => {
                app.show_status("正在保存", "请等待转换完成后关闭。", view::ACCENT);
                return 0;
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let app = Box::from_raw(ptr);
                DeleteObject(app.font);
                DeleteObject(app.title_font);
                DeleteObject(app.heading_font);
                DeleteObject(app.small_font);
                DeleteObject(app.background);
                DeleteObject(app.surface);
                PostQuitMessage(0);
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, w, l)
}

pub fn run(initial: Option<PathBuf>) {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
        let instance = GetModuleHandleW(null());
        let class = wide("MoZhuanWordNativeV3");
        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.hIcon = LoadIconW(instance, 1usize as *const u16);
        if RegisterClassW(&wc) == 0 {
            return;
        }
        let mut work: RECT = zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0);
        let scale = (GetDpiForSystem() as f64 / 96.0)
            .min((work.right - work.left - 48).max(600) as f64 / view::WIDTH as f64)
            .min((work.bottom - work.top - 64).max(500) as f64 / view::HEIGHT as f64);
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: (view::WIDTH as f64 * scale) as i32,
            bottom: (view::HEIGHT as f64 * scale) as i32,
        };
        AdjustWindowRectExForDpi(&mut bounds, style, 0, 0, GetDpiForSystem());
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        let hwnd = CreateWindowExW(
            WS_EX_ACCEPTFILES,
            class.as_ptr(),
            wide("墨转 Word · v3").as_ptr(),
            style,
            work.left + (work.right - work.left - width) / 2,
            work.top + (work.bottom - work.top - height) / 2,
            width,
            height,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        let mut app = view::build(hwnd, scale);
        if let Some(path) = initial {
            app.selected(path);
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(app)) as isize);
        ShowWindow(hwnd, SW_SHOWNORMAL);
        UpdateWindow(hwnd);
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            if message.message == WM_KEYDOWN && message.wParam == VK_RETURN as usize {
                let focus = GetFocus();
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const App;
                if !ptr.is_null() {
                    let app = &*ptr;
                    if focus != app.text_input {
                        let button = if [
                            app.select,
                            app.save,
                            app.convert,
                            app.open,
                            app.file_tab,
                            app.text_tab,
                        ]
                        .contains(&focus)
                        {
                            focus
                        } else {
                            app.convert
                        };
                        if IsWindowEnabled(button) != 0 {
                            SendMessageW(button, BM_CLICK, 0, 0);
                        }
                        continue;
                    }
                }
            }
            if IsDialogMessageW(hwnd, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}
