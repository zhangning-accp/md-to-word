//! AppKit window and controls. The timer exists only while a conversion runs.
#![allow(unsafe_op_in_unsafe_fn)]
use md_to_word::convert;
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSTimer, NSURL, ns_string,
};
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

struct State {
    _window: Retained<NSWindow>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    input_label: Retained<NSTextField>,
    input_name: Retained<NSTextField>,
    output_label: Retained<NSTextField>,
    status: Retained<NSTextField>,
    choose: Retained<NSButton>,
    save: Retained<NSButton>,
    convert: Retained<NSButton>,
    open: Retained<NSButton>,
    worker: Option<Receiver<convert::Result<convert::Report>>>,
    timer: Option<Retained<NSTimer>>,
}
impl State {
    unsafe fn selected(&mut self, path: PathBuf) {
        self.output = Some(path.with_extension("docx"));
        self.input = Some(path);
        self.input_name.setStringValue(&NSString::from_str(
            &self
                .input
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
        ));
        self.input_label.setStringValue(&NSString::from_str(
            &self.input.as_ref().unwrap().display().to_string(),
        ));
        self.output_label.setStringValue(&NSString::from_str(
            &self.output.as_ref().unwrap().display().to_string(),
        ));
        self.status
            .setStringValue(ns_string!("准备就绪\n原文换行和数学公式将保留到 Word 中。"));
        self.convert.setEnabled(true);
        self.open.setEnabled(false);
    }
    unsafe fn busy(&self, busy: bool) {
        self.choose.setEnabled(!busy);
        self.save.setEnabled(!busy);
        self.convert.setEnabled(!busy && self.input.is_some());
        self.convert.setTitle(if busy {
            ns_string!("正在生成…")
        } else {
            ns_string!("生成 Word 文档")
        });
    }
}
#[derive(Default)]
struct Ivars {
    state: RefCell<Option<State>>,
}
define_class!(
    // SAFETY: NSObject has no extra subclass invariants; all UI state is main-thread-only.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Controller;

    unsafe impl NSObjectProtocol for Controller {}
    unsafe impl NSApplicationDelegate for Controller {
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app:&NSApplication)->NSApplicationTerminateReply {
            let state=self.ivars().state.borrow();
            if state.as_ref().is_some_and(|s|s.worker.is_some()) {
                unsafe {state.as_ref().unwrap().status.setStringValue(ns_string!("正在保存文件，请等待转换完成后关闭。"));}
                NSApplicationTerminateReply::TerminateCancel
            } else {NSApplicationTerminateReply::TerminateNow}
        }
    }
    unsafe impl NSWindowDelegate for Controller {
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self,_window:&NSWindow)->bool {
            let state=self.ivars().state.borrow();
            if let Some(s)=state.as_ref().filter(|s|s.worker.is_some()) {
                unsafe {s.status.setStringValue(ns_string!("正在保存文件，请等待转换完成后关闭。"));}
                false
            } else {
                true
            }
        }
        #[unsafe(method(windowWillClose:))]
        fn closed(&self,_notification:&NSNotification) {unsafe {NSApplication::sharedApplication(self.mtm()).terminate(None);}}
    }
    impl Controller {
        #[unsafe(method(choose:))]
        fn choose(&self,_sender:&AnyObject) { unsafe {
            if self.ivars().state.borrow().as_ref().is_some_and(|s|s.worker.is_some()) {return;}
            let panel=NSOpenPanel::openPanel(self.mtm());
            panel.setCanChooseFiles(true);panel.setCanChooseDirectories(false);panel.setAllowsMultipleSelection(false);
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&NSArray::from_slice(&[ns_string!("md"),ns_string!("markdown")])));
            if panel.runModal()==NSModalResponseOK {
                if let Some(path)=panel.URL().and_then(|u|u.path()) {
                    if let Some(s)=self.ivars().state.borrow_mut().as_mut() {s.selected(PathBuf::from(path.to_string()));}
                }
            }
        }}
        #[unsafe(method(save:))]
        fn save(&self,_sender:&AnyObject) {unsafe {
            if self.ivars().state.borrow().as_ref().is_some_and(|s|s.worker.is_some()) {return;}
            let output=self.ivars().state.borrow().as_ref().and_then(|s|s.output.clone());
            let panel=NSSavePanel::savePanel(self.mtm());
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&NSArray::from_slice(&[ns_string!("docx")])));
            panel.setCanCreateDirectories(true);
            if let Some(p)=output {
                panel.setNameFieldStringValue(&NSString::from_str(&p.file_name().unwrap_or_default().to_string_lossy()));
                if let Some(parent)=p.parent() {panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(&parent.display().to_string()))));}
            }
            if panel.runModal()==NSModalResponseOK {
                if let Some(path)=panel.URL().and_then(|u|u.path()) {
                    if let Some(s)=self.ivars().state.borrow_mut().as_mut() {
                        s.output=Some(PathBuf::from(path.to_string()));s.output_label.setStringValue(&path);s.open.setEnabled(false);
                    }
                }
            }
        }}
        #[unsafe(method(convert:))]
        fn convert(&self,_sender:&AnyObject) {unsafe {
            let paths=self.ivars().state.borrow().as_ref().filter(|s|s.worker.is_none()).and_then(|s|Some((s.input.clone()?,s.output.clone()?)));
            let Some((input,output))=paths else{return;};
            if output.exists() {
                let alert=NSAlert::new(self.mtm());alert.setMessageText(ns_string!("替换已有的 Word 文件？"));
                alert.setInformativeText(&NSString::from_str(&output.display().to_string()));
                alert.addButtonWithTitle(ns_string!("取消"));alert.addButtonWithTitle(ns_string!("替换"));
                if alert.runModal()!=NSAlertSecondButtonReturn {return;}
            }
            let (tx,rx)=mpsc::channel();
            if let Some(s)=self.ivars().state.borrow_mut().as_mut() {
                s.worker=Some(rx);s.busy(true);s.open.setEnabled(false);
                s.status.setStringValue(ns_string!("正在转换…\n公式、表格和图片会自动写入 Word。"));
                s.timer=Some(NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(0.15,self,sel!(tick:),None,true));
            }
            std::thread::spawn(move ||{let _=tx.send(convert::convert(&input,&output,true));});
        }}
        #[unsafe(method(tick:))]
        fn tick(&self,_timer:&NSTimer) {unsafe {
            let mut state=self.ivars().state.borrow_mut();let Some(s)=state.as_mut() else{return;};
            let Some(rx)=s.worker.as_ref() else{return;};
            let result=match rx.try_recv() {Ok(r)=>r,Err(TryRecvError::Empty)=>return,Err(_)=>Err("转换任务意外结束，请重试".into())};
            s.worker=None;if let Some(timer)=s.timer.take() {timer.invalidate();}s.busy(false);
            match result {
                Ok(report)=>{let text=report.summary();s.status.setStringValue(&NSString::from_str(&text));s.status.setToolTip(Some(&NSString::from_str(&text)));s.open.setEnabled(true);}
                Err(e)=>s.status.setStringValue(&NSString::from_str(&format!("转换失败：{e}"))),
            }
        }}
        #[unsafe(method(open:))]
        fn open(&self,_sender:&AnyObject) {unsafe {
            if let Some(path)=self.ivars().state.borrow().as_ref().and_then(|s|s.output.as_ref()) {
                NSWorkspace::sharedWorkspace().openURL(&NSURL::fileURLWithPath(&NSString::from_str(&path.display().to_string())));
            }
        }}
    }
);
#[allow(clippy::too_many_arguments)]
unsafe fn label(
    view: &NSView,
    text: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    size: f64,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    label.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)));
    label.setFont(Some(&NSFont::systemFontOfSize(size)));
    label.setSelectable(true);
    view.addSubview(&label);
    label
}
#[allow(clippy::too_many_arguments)]
unsafe fn button(
    view: &NSView,
    title: &str,
    x: f64,
    y: f64,
    w: f64,
    action: objc2::runtime::Sel,
    target: &Controller,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = NSButton::buttonWithTitle_target_action(
        &NSString::from_str(title),
        Some(target),
        Some(action),
        mtm,
    );
    button.setBezelStyle(NSBezelStyle::Push);
    button.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 40.0)));
    view.addSubview(&button);
    button
}
unsafe fn card(view: &NSView, y: f64, height: f64, mtm: MainThreadMarker) {
    let panel = NSBox::initWithFrame(
        NSBox::alloc(mtm),
        NSRect::new(
            NSPoint::new(32.0, 660.0 - y - height),
            NSSize::new(696.0, height),
        ),
    );
    panel.setBoxType(NSBoxType::Custom);
    panel.setTitlePosition(NSTitlePosition::NoTitle);
    panel.setCornerRadius(12.0);
    panel.setBorderWidth(1.0);
    panel.setBorderColor(&NSColor::separatorColor());
    panel.setFillColor(&NSColor::controlBackgroundColor());
    view.addSubview(&panel);
}
#[allow(clippy::too_many_arguments)]
unsafe fn top_label(
    view: &NSView,
    text: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    size: f64,
    bold: bool,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let field = label(view, text, x, 660.0 - y - h, w, h, size, mtm);
    if bold {
        field.setFont(Some(&NSFont::boldSystemFontOfSize(size)));
    } else {
        field.setTextColor(Some(&NSColor::secondaryLabelColor()));
    }
    field
}
pub fn run(initial: Option<PathBuf>) {
    unsafe {
        let mtm = MainThreadMarker::new().expect("AppKit must run on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        let allocated = Controller::alloc(mtm).set_ivars(Ivars::default());
        let controller: Retained<Controller> = msg_send![super(allocated), init];
        let menu = NSMenu::new(mtm);
        let item = NSMenuItem::new(mtm);
        let submenu = NSMenu::new(mtm);
        submenu.addItemWithTitle_action_keyEquivalent(
            ns_string!("退出墨转 Word"),
            Some(sel!(terminate:)),
            ns_string!("q"),
        );
        item.setSubmenu(Some(&submenu));
        menu.addItem(&item);
        app.setMainMenu(Some(&menu));
        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(760.0, 660.0)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable,
            NSBackingStoreType::Buffered,
            false,
        );
        window.setReleasedWhenClosed(false);
        window.setTitle(ns_string!("墨转 Word · 0.2"));
        window.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
        let view = window.contentView().expect("window content view");
        for (y, h) in [(142.0, 150.0), (308.0, 112.0), (534.0, 96.0)] {
            card(&view, y, h, mtm);
        }
        top_label(&view, "墨转 Word", 34.0, 32.0, 600.0, 46.0, 30.0, true, mtm);
        top_label(
            &view,
            "保留原文换行，数学公式可继续编辑",
            35.0,
            87.0,
            650.0,
            25.0,
            14.0,
            false,
            mtm,
        );
        top_label(&view, "0.2", 665.0, 45.0, 60.0, 24.0, 13.0, false, mtm);
        top_label(
            &view,
            "01   Markdown 文件",
            52.0,
            160.0,
            600.0,
            28.0,
            16.0,
            true,
            mtm,
        );
        let input_name = top_label(
            &view,
            "还没有选择文件",
            52.0,
            201.0,
            474.0,
            28.0,
            16.0,
            true,
            mtm,
        );
        let input_label = top_label(
            &view,
            "支持 .md、.markdown 文件",
            52.0,
            241.0,
            474.0,
            28.0,
            13.0,
            false,
            mtm,
        );
        let choose = button(
            &view,
            "选择文件",
            546.0,
            660.0 - 219.0 - 42.0,
            162.0,
            sel!(choose:),
            &controller,
            mtm,
        );
        top_label(
            &view,
            "Word 保存位置",
            52.0,
            328.0,
            500.0,
            28.0,
            16.0,
            true,
            mtm,
        );
        let output_label = top_label(
            &view,
            "默认保存在源文件所在文件夹",
            52.0,
            370.0,
            474.0,
            28.0,
            13.0,
            false,
            mtm,
        );
        let save = button(
            &view,
            "更改位置",
            546.0,
            660.0 - 357.0 - 42.0,
            162.0,
            sel!(save:),
            &controller,
            mtm,
        );
        top_label(
            &view,
            "保留换行     ·     可编辑公式     ·     本地处理",
            35.0,
            437.0,
            690.0,
            25.0,
            13.0,
            false,
            mtm,
        );
        let convert = button(
            &view,
            "生成 Word 文档",
            27.0,
            660.0 - 476.0 - 46.0,
            454.0,
            sel!(convert:),
            &controller,
            mtm,
        );
        convert.setKeyEquivalent(ns_string!("\r"));
        convert.setFont(Some(&NSFont::boldSystemFontOfSize(16.0)));
        let open = button(
            &view,
            "打开生成文件",
            487.0,
            660.0 - 476.0 - 46.0,
            246.0,
            sel!(open:),
            &controller,
            mtm,
        );
        let status = top_label(
            &view,
            "等待选择文件\n选择一份 Markdown，即可开始转换。",
            52.0,
            551.0,
            650.0,
            65.0,
            14.0,
            false,
            mtm,
        );
        convert.setEnabled(false);
        open.setEnabled(false);
        let mut state = State {
            _window: window.clone(),
            input: None,
            output: None,
            input_label,
            input_name,
            output_label,
            status,
            choose,
            save,
            convert,
            open,
            worker: None,
            timer: None,
        };
        if let Some(path) = initial {
            state.selected(path);
        }
        *controller.ivars().state.borrow_mut() = Some(state);
        window.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
        app.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
        window.center();
        window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        app.run();
    }
}
