//! Small native GDI view. No retained bitmap, browser, or animation loop.
#![allow(unsafe_op_in_unsafe_fn)]
use super::*;
use windows_sys::Win32::UI::Controls::{ODS_DISABLED, ODS_FOCUS, ODS_SELECTED};

pub(super) const WIDTH: i32 = 760;
pub(super) const HEIGHT: i32 = 800;
const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}
const BG: COLORREF = rgb(246, 248, 252);
const WHITE: COLORREF = rgb(255, 255, 255);
const TEXT: COLORREF = rgb(29, 41, 63);
const MUTED: COLORREF = rgb(109, 121, 142);
const EDGE: COLORREF = rgb(226, 231, 240);
const TINT: COLORREF = rgb(237, 242, 255);
pub(super) const ACCENT: COLORREF = rgb(60, 91, 222);
pub(super) const SUCCESS: COLORREF = rgb(31, 130, 106);
pub(super) const WARNING: COLORREF = rgb(166, 107, 30);
pub(super) const ERROR: COLORREF = rgb(190, 59, 70);
const TITLE: usize = 200;
const SUBTITLE: usize = 201;
const INPUT_TITLE: usize = 202;
const OUTPUT_TITLE: usize = 203;
const FEATURES: usize = 204;
const NAME: usize = 205;
const STATUS_TITLE: usize = 206;
const VERSION: usize = 207;

fn rect(x: i32, y: i32, w: i32, h: i32, s: f64) -> RECT {
    let px = |v: i32| (v as f64 * s).round() as i32;
    RECT {
        left: px(x),
        top: px(y),
        right: px(x + w),
        bottom: px(y + h),
    }
}
unsafe fn font(size: i32, weight: i32, scale: f64) -> HFONT {
    CreateFontW(
        -(size as f64 * scale).round() as i32,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        0,
        0,
        CLEARTYPE_QUALITY as u32,
        0,
        wide("Microsoft YaHei UI").as_ptr(),
    )
}
unsafe fn rounded(dc: HDC, r: RECT, fill: COLORREF, border: COLORREF, radius: i32) {
    let brush = CreateSolidBrush(fill);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, pen);
    RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    DeleteObject(pen);
    DeleteObject(brush);
}
unsafe fn text(dc: HDC, value: &str, mut r: RECT, font: HFONT, color: COLORREF, flags: u32) {
    let old = SelectObject(dc, font);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color);
    let value = wide(value);
    DrawTextW(
        dc,
        value.as_ptr(),
        (value.len() - 1) as i32,
        &mut r,
        flags | DT_NOPREFIX,
    );
    SelectObject(dc, old);
}

pub(super) unsafe fn paint(hwnd: HWND, app: &App, dc: HDC) {
    let s = app.scale;
    let mut client: RECT = zeroed();
    GetClientRect(hwnd, &mut client);
    FillRect(dc, &client, app.background);
    // Soft edges and one-pixel outlines separate the file cards from the canvas.
    for (y, h) in [(142, 310), (468, 112), (688, 94)] {
        rounded(
            dc,
            rect(32, y + 2, 696, h, s),
            rgb(236, 239, 247),
            rgb(236, 239, 247),
            (16.0 * s) as i32,
        );
        rounded(dc, rect(32, y, 696, h, s), WHITE, EDGE, (16.0 * s) as i32);
    }
    rounded(
        dc,
        rect(34, 35, 50, 54, s),
        ACCENT,
        ACCENT,
        (14.0 * s) as i32,
    );
    let pen = CreatePen(PS_SOLID, (2.0 * s).round() as i32, WHITE);
    let old = SelectObject(dc, pen);
    for (y, width) in [(51, 22), (60, 22), (69, 14)] {
        let line = rect(48, y, width, 0, s);
        MoveToEx(dc, line.left, line.top, null_mut());
        LineTo(dc, line.right, line.top);
    }
    SelectObject(dc, old);
    DeleteObject(pen);
    rounded(dc, rect(52, 160, 32, 24, s), TINT, TINT, (8.0 * s) as i32);
    text(
        dc,
        "01",
        rect(52, 160, 32, 24, s),
        app.small_font,
        ACCENT,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    let color = app.status_color;
    rounded(dc, rect(53, 707, 9, 9, s), color, color, (9.0 * s) as i32);
}

pub(super) unsafe fn control_color(app: &App, dc: HDC, handle: HWND) -> LRESULT {
    let id = GetDlgCtrlID(handle) as usize;
    let outside = matches!(id, TITLE | SUBTITLE | FEATURES | VERSION);
    let bg = if outside { BG } else { WHITE };
    let color = if handle == app.status_title {
        app.status_color
    } else if matches!(id, SUBTITLE | FEATURES | VERSION)
        || handle == app.input_label
        || handle == app.file_hint
        || handle == app.output_label
        || handle == app.status
    {
        MUTED
    } else {
        TEXT
    };
    SetBkMode(dc, TRANSPARENT as i32);
    SetBkColor(dc, bg);
    SetTextColor(dc, color);
    if outside {
        app.background as isize
    } else {
        app.surface as isize
    }
}

pub(super) unsafe fn draw_button(app: &App, item: &DRAWITEMSTRUCT) {
    let dc = item.hDC;
    let id = item.CtlID as usize;
    let disabled = item.itemState & ODS_DISABLED != 0;
    let pressed = item.itemState & ODS_SELECTED != 0;
    let hot = app.hover == item.hwndItem && !disabled;
    FillRect(
        dc,
        &item.rcItem,
        if matches!(id, SELECT | SAVE | FILE_TAB | TEXT_TAB) {
            app.surface
        } else {
            app.background
        },
    );
    let (fill, border, ink) = match id {
        CONVERT if disabled => (rgb(223, 229, 248), rgb(223, 229, 248), rgb(130, 145, 188)),
        CONVERT if pressed => (rgb(41, 66, 177), rgb(41, 66, 177), WHITE),
        CONVERT if hot => (rgb(74, 105, 234), rgb(74, 105, 234), WHITE),
        CONVERT => (ACCENT, ACCENT, WHITE),
        _ if disabled => (rgb(241, 244, 249), rgb(230, 235, 243), rgb(161, 172, 189)),
        FILE_TAB | TEXT_TAB if (id == FILE_TAB) == (app.mode == InputMode::File) => {
            (TINT, ACCENT, ACCENT)
        }
        FILE_TAB | TEXT_TAB => (WHITE, EDGE, MUTED),
        SELECT => (
            if pressed {
                rgb(222, 232, 255)
            } else if hot {
                rgb(228, 237, 255)
            } else {
                TINT
            },
            rgb(213, 223, 253),
            ACCENT,
        ),
        _ => (
            if pressed {
                rgb(227, 233, 244)
            } else if hot {
                rgb(239, 243, 251)
            } else {
                WHITE
            },
            EDGE,
            TEXT,
        ),
    };
    let mut r = item.rcItem;
    r.right -= 1;
    r.bottom -= 1;
    rounded(dc, r, fill, border, (12.0 * app.scale).round() as i32);
    if item.itemState & ODS_FOCUS != 0 && !disabled {
        let mut focus = r;
        let inset = (3.0 * app.scale).round() as i32;
        InflateRect(&mut focus, -inset, -inset);
        DrawFocusRect(dc, &focus);
    }
    let mut buffer = [0u16; 128];
    let len = GetWindowTextW(item.hwndItem, buffer.as_mut_ptr(), buffer.len() as i32);
    let title = String::from_utf16_lossy(&buffer[..len.max(0) as usize]);
    text(
        dc,
        &title,
        r,
        if id == CONVERT {
            app.heading_font
        } else {
            app.font
        },
        ink,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let ptr = GetWindowLongPtrW(GetParent(hwnd), GWLP_USERDATA) as *mut App;
    if !ptr.is_null() {
        let app = &mut *ptr;
        match message {
            WM_MOUSEMOVE if IsWindowEnabled(hwnd) != 0 => {
                if app.hover != hwnd {
                    let old = app.hover;
                    app.hover = hwnd;
                    if !old.is_null() {
                        InvalidateRect(old, null(), 0);
                    }
                    InvalidateRect(hwnd, null(), 0);
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut track);
                }
            }
            WM_MOUSELEAVE => {
                if app.hover == hwnd {
                    app.hover = null_mut();
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            WM_SETCURSOR if IsWindowEnabled(hwnd) != 0 => {
                SetCursor(LoadCursorW(null_mut(), IDC_HAND));
                return 1;
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(button_proc), 1);
            }
            _ => {}
        }
    }
    DefSubclassProc(hwnd, message, w, l)
}

pub(super) unsafe fn build(hwnd: HWND, scale: f64) -> App {
    let regular = font(15, 400, scale);
    let title = font(30, 600, scale);
    let heading = font(16, 600, scale);
    let small = font(13, 400, scale);
    let c = |class: &str, label: &str, style: u32, x, y, w, h, id, font| {
        control(hwnd, class, label, style, x, y, w, h, id, scale, font)
    };
    c("STATIC", "墨转 Word", 0, 103, 31, 480, 46, TITLE, title);
    c(
        "STATIC",
        "保留原文换行，数学公式可继续编辑",
        0,
        104,
        81,
        550,
        24,
        SUBTITLE,
        small,
    );
    c("STATIC", "v3", SS_RIGHT, 654, 45, 66, 24, VERSION, small);
    c(
        "STATIC",
        "输入 Markdown",
        0,
        94,
        159,
        540,
        28,
        INPUT_TITLE,
        heading,
    );
    let file_tab = c(
        "BUTTON",
        "文件",
        WS_TABSTOP | BS_OWNERDRAW as u32,
        52,
        199,
        142,
        36,
        FILE_TAB,
        regular,
    );
    let text_tab = c(
        "BUTTON",
        "粘贴文字",
        WS_TABSTOP | BS_OWNERDRAW as u32,
        206,
        199,
        142,
        36,
        TEXT_TAB,
        regular,
    );
    let input_name = c(
        "STATIC",
        "还没有选择文件",
        SS_ENDELLIPSIS,
        52,
        267,
        480,
        28,
        NAME,
        heading,
    );
    let input_label = c(
        "EDIT",
        "支持 .md、.markdown，或直接拖入文件",
        ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
        52,
        312,
        480,
        25,
        0,
        small,
    );
    let select = c(
        "BUTTON",
        "选择文件",
        WS_TABSTOP | BS_OWNERDRAW as u32,
        551,
        290,
        153,
        42,
        SELECT,
        regular,
    );
    let file_hint = c(
        "STATIC",
        "选择 .md / .markdown 文件，也可以直接拖入窗口。",
        0,
        52,
        390,
        620,
        24,
        0,
        small,
    );
    let text_input = c(
        "EDIT",
        "",
        WS_TABSTOP
            | WS_BORDER
            | WS_VSCROLL
            | ES_MULTILINE as u32
            | ES_AUTOVSCROLL as u32
            | ES_WANTRETURN as u32,
        52,
        249,
        652,
        179,
        TEXT_INPUT,
        regular,
    );
    SendMessageW(text_input, EM_SETLIMITTEXT, 8 * 1024 * 1024, 0);
    ShowWindow(text_input, SW_HIDE);
    c(
        "STATIC",
        "Word 保存位置",
        0,
        52,
        487,
        470,
        27,
        OUTPUT_TITLE,
        heading,
    );
    let output_label = c(
        "EDIT",
        "默认保存在源文件所在文件夹",
        ES_READONLY as u32 | ES_AUTOHSCROLL as u32,
        52,
        529,
        480,
        25,
        0,
        small,
    );
    let save = c(
        "BUTTON",
        "更改位置",
        WS_TABSTOP | BS_OWNERDRAW as u32,
        551,
        517,
        153,
        42,
        SAVE,
        regular,
    );
    c(
        "STATIC",
        "保留换行     ·     可编辑公式     ·     本地处理",
        0,
        35,
        595,
        680,
        24,
        FEATURES,
        small,
    );
    let convert = c(
        "BUTTON",
        "生成 Word 文档",
        WS_TABSTOP | BS_OWNERDRAW as u32,
        32,
        628,
        444,
        46,
        CONVERT,
        heading,
    );
    let open = c(
        "BUTTON",
        "打开生成文件",
        WS_TABSTOP | BS_OWNERDRAW as u32,
        492,
        628,
        236,
        46,
        OPEN,
        regular,
    );
    let status_title = c(
        "STATIC",
        "等待输入内容",
        0,
        74,
        700,
        620,
        25,
        STATUS_TITLE,
        heading,
    );
    let status = c(
        "EDIT",
        "选择 Markdown 文件，或切换到“粘贴文字”。",
        ES_READONLY as u32 | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | WS_VSCROLL,
        52,
        735,
        650,
        35,
        0,
        small,
    );
    ShowScrollBar(status, SB_VERT, 0);
    EnableWindow(convert, 0);
    EnableWindow(open, 0);
    for button in [select, save, convert, open, file_tab, text_tab] {
        SetWindowSubclass(button, Some(button_proc), 1, 0);
    }
    App {
        mode: InputMode::File,
        input: None,
        output: None,
        input_label,
        input_name,
        file_hint,
        text_input,
        output_label,
        status_title,
        status,
        select,
        file_tab,
        text_tab,
        save,
        convert,
        open,
        font: regular,
        title_font: title,
        heading_font: heading,
        small_font: small,
        background: CreateSolidBrush(BG),
        surface: CreateSolidBrush(WHITE),
        scale,
        hover: null_mut(),
        status_color: MUTED,
        worker: None,
    }
}

#[cfg(test)]
mod preview {
    use super::*;
    use std::io::Write;

    // Render this application's own controls into a DIB without showing a
    // desktop window. This is an opt-in visual QA aid, not a screen capture.
    #[test]
    #[ignore = "writes native view previews to qa-output"]
    fn render_native_view() {
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
            let name = wide("MoZhuanOffscreenPreview");
            let instance = GetModuleHandleW(null());
            let class = WNDCLASSW {
                lpfnWndProc: Some(super::super::wndproc),
                hInstance: instance,
                lpszClassName: name.as_ptr(),
                ..zeroed()
            };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                0,
                name.as_ptr(),
                wide("Preview").as_ptr(),
                WS_POPUP,
                0,
                0,
                WIDTH,
                HEIGHT,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            assert!(!hwnd.is_null());
            let mut app = Box::new(build(hwnd, 1.0));
            app.set_mode(InputMode::Text);
            SetWindowTextW(
                app.text_input,
                wide("# 数学练习\r\n请计算 $x^2+1$。\r\n\r\n- 第一题\r\n- 第二题").as_ptr(),
            );
            app.busy(false);
            assert!(app.pasted_text().unwrap().contains("请计算 $x^2+1$。"));
            assert_ne!(IsWindowEnabled(app.convert), 0);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
            let mut info: BITMAPINFO = zeroed();
            info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = WIDTH;
            info.bmiHeader.biHeight = -HEIGHT;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;
            let dc = CreateCompatibleDC(null_mut());
            let mut pixels = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
            assert!(!bitmap.is_null());
            let old = SelectObject(dc, bitmap);
            let app = &*(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const App);
            paint(hwnd, app, dc);
            let mut child = GetWindow(hwnd, GW_CHILD);
            while !child.is_null() {
                if GetWindowLongPtrW(child, GWL_STYLE) as u32 & WS_VISIBLE == 0 {
                    child = GetWindow(child, GW_HWNDNEXT);
                    continue;
                }
                let mut bounds: RECT = zeroed();
                GetWindowRect(child, &mut bounds);
                MapWindowPoints(null_mut(), hwnd, (&mut bounds as *mut RECT).cast(), 2);
                let saved = SaveDC(dc);
                SetViewportOrgEx(dc, bounds.left, bounds.top, null_mut());
                IntersectClipRect(
                    dc,
                    0,
                    0,
                    bounds.right - bounds.left,
                    bounds.bottom - bounds.top,
                );
                SendMessageW(
                    child,
                    WM_PRINT,
                    dc as usize,
                    (PRF_CLIENT | PRF_ERASEBKGND | PRF_CHILDREN) as isize,
                );
                RestoreDC(dc, saved);
                child = GetWindow(child, GW_HWNDNEXT);
            }
            GdiFlush();
            let data =
                std::slice::from_raw_parts(pixels as *const u8, (WIDTH * HEIGHT * 4) as usize);
            let mut file = std::fs::File::create("qa-output/interface-v3.bmp").unwrap();
            file.write_all(b"BM").unwrap();
            file.write_all(&(54u32 + data.len() as u32).to_le_bytes())
                .unwrap();
            file.write_all(&0u32.to_le_bytes()).unwrap();
            file.write_all(&54u32.to_le_bytes()).unwrap();
            let header = std::slice::from_raw_parts(
                (&info.bmiHeader as *const BITMAPINFOHEADER).cast::<u8>(),
                40,
            );
            file.write_all(header).unwrap();
            file.write_all(data).unwrap();
            SelectObject(dc, old);
            DeleteObject(bitmap);
            DeleteDC(dc);
            DestroyWindow(hwnd);
        }
    }
}
