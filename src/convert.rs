use crate::{math, xml};
use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};
use std::{
    borrow::Cow,
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use zip::{ZipWriter, write::SimpleFileOptions};

pub type Result<T> = std::result::Result<T, String>;
const MAX_MARKDOWN: u64 = 32 * 1024 * 1024;
const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const M_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

#[derive(Debug, Default)]
pub struct Report {
    pub paragraphs: usize,
    pub equations: usize,
    pub tables: usize,
    pub images: usize,
    pub warnings: Vec<String>,
    pub warning_count: usize,
    pub elapsed: Duration,
}
impl Report {
    fn warn(&mut self, message: String) {
        self.warning_count += 1;
        if self.warnings.len() < 20 {
            self.warnings.push(message);
        }
    }
    pub fn summary(&self) -> String {
        let mut s = format!(
            "转换完成 · {} 个段落 · {} 个公式 · {} 个表格 · {} 张图片\n用时 {:.2} 秒",
            self.paragraphs,
            self.equations,
            self.tables,
            self.images,
            self.elapsed.as_secs_f64()
        );
        if self.warning_count > 0 {
            s.push_str(&format!(
                "\n有 {} 处需要检查（原内容已保留）：\n{}",
                self.warning_count,
                self.warnings.join("\n")
            ));
        }
        s
    }
}

/// Write beside the destination and atomically commit only a complete package.
/// Failed conversions never truncate an existing Word document.
pub fn convert(input: &Path, output: &Path, overwrite: bool) -> Result<Report> {
    let began = Instant::now();
    if !input.is_file() {
        return Err("请选择存在的 Markdown 文件".into());
    }
    if output
        .extension()
        .and_then(|s| s.to_str())
        .is_none_or(|s| !s.eq_ignore_ascii_case("docx"))
    {
        return Err("输出文件必须使用 .docx 扩展名".into());
    }
    if input.canonicalize().ok() == output.canonicalize().ok() && output.exists() {
        return Err("输入和输出不能是同一个文件".into());
    }
    if output.exists() && !overwrite {
        return Err("输出文件已存在，请换一个名称，或使用 --force".into());
    }
    let file = File::open(input).map_err(|e| format!("无法读取源文件：{e}"))?;
    let mut source = String::new();
    file.take(MAX_MARKDOWN + 1)
        .read_to_string(&mut source)
        .map_err(|e| format!("请使用 UTF-8 编码的 Markdown：{e}"))?;
    if source.len() as u64 > MAX_MARKDOWN {
        return Err("Markdown 超过 32 MiB，请拆分后转换，以控制内存占用".into());
    }
    let source = source.strip_prefix('\u{feff}').unwrap_or(&source);
    let normalized = normalize_math_delimiters(source);
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("无法在输出目录创建文件：{e}"))?;
    let mut zip = ZipWriter::new(temporary.as_file_mut());
    start(&mut zip, "word/document.xml")?;
    write(
        &mut zip,
        &format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"{W_NS}\" xmlns:m=\"{M_NS}\" xmlns:r=\"{REL_NS}\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><w:body>"
        ),
    )?;
    let mut doc = Document::new(&mut zip, input.parent().unwrap_or(Path::new(".")));
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH;
    let mut events = Parser::new_ext(&normalized, options);
    while let Some(event) = events.next() {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => {
                let mut alt = String::new();
                let mut depth = 1;
                for event in events.by_ref() {
                    match event {
                        Event::Start(Tag::Image { .. }) => depth += 1,
                        Event::End(TagEnd::Image) => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Event::Text(t) | Event::Code(t) | Event::InlineMath(t) => alt.push_str(&t),
                        _ => {}
                    }
                }
                doc.image(&dest_url, &alt)?;
            }
            Event::Start(tag) => doc.begin(tag)?,
            Event::End(tag) => doc.end(tag)?,
            Event::Text(t) => doc.text(&t, false)?,
            Event::Code(t) => doc.text(&t, true)?,
            Event::InlineMath(t) => doc.equation(&t, false)?,
            Event::DisplayMath(t) => doc.equation(&t, true)?,
            // Document conversion follows the author's physical line breaks.
            // CommonMark's default soft break would join adjacent source lines.
            Event::SoftBreak | Event::HardBreak => {
                doc.paragraph()?;
                doc.put("<w:r><w:br/></w:r>")?;
            }
            Event::Rule => {
                doc.close_paragraph()?;
                doc.put("<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:color=\"CBD5E1\"/></w:pBdr></w:pPr></w:p>")?;
            }
            Event::TaskListMarker(done) => doc.text(if done { "☑ " } else { "☐ " }, false)?,
            Event::Html(t) | Event::InlineHtml(t) => {
                if !doc.html_warned {
                    doc.report
                        .warn("HTML 以原始文本保留；建议使用 Markdown 排版".into());
                    doc.html_warned = true;
                }
                doc.text(&t, true)?;
            }
            Event::FootnoteReference(t) => doc.text(&format!("[{t}]"), false)?,
        }
    }
    doc.close_paragraph()?;
    doc.put("<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1134\" w:right=\"1134\" w:bottom=\"1134\" w:left=\"1134\" w:header=\"567\" w:footer=\"567\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>")?;
    let (images, links, numbering, mut report) = (doc.images, doc.links, doc.numbering, doc.report);
    for (index, image) in images.iter().enumerate() {
        start(
            &mut zip,
            &format!("word/media/image{}.{}", index + 1, image.extension),
        )?;
        let mut file =
            File::open(&image.path).map_err(|e| format!("图片在转换过程中无法读取：{e}"))?;
        std::io::copy(&mut file, &mut zip).map_err(|e| e.to_string())?;
    }
    part(&mut zip, "[Content_Types].xml", &content_types())?;
    part(
        &mut zip,
        "_rels/.rels",
        &format!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL_NS}/officeDocument\" Target=\"word/document.xml\"/></Relationships>"
        ),
    )?;
    part(&mut zip, "word/styles.xml", &styles())?;
    part(
        &mut zip,
        "word/settings.xml",
        &format!(
            "<w:settings xmlns:w=\"{W_NS}\" xmlns:m=\"{M_NS}\"><w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat><m:mathPr><m:mathFont m:val=\"Cambria Math\"/></m:mathPr></w:settings>"
        ),
    )?;
    part(&mut zip, "word/numbering.xml", &numbering_xml(&numbering))?;
    let mut rels = format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"styles\" Type=\"{REL_NS}/styles\" Target=\"styles.xml\"/><Relationship Id=\"settings\" Type=\"{REL_NS}/settings\" Target=\"settings.xml\"/><Relationship Id=\"numbering\" Type=\"{REL_NS}/numbering\" Target=\"numbering.xml\"/>"
    );
    for (i, image) in images.iter().enumerate() {
        rels.push_str(&format!(
            "<Relationship Id=\"image{}\" Type=\"{REL_NS}/image\" Target=\"media/image{}.{}\"/>",
            i + 1,
            i + 1,
            image.extension
        ));
    }
    for (i, link) in links.iter().enumerate() {
        rels.push_str(&format!("<Relationship Id=\"link{}\" Type=\"{REL_NS}/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>", i+1, xml(link)));
    }
    rels.push_str("</Relationships>");
    part(&mut zip, "word/_rels/document.xml.rels", &rels)?;
    zip.finish()
        .map_err(|e| format!("无法完成 Word 文件：{e}"))?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    if overwrite {
        temporary.persist(output)
    } else {
        temporary.persist_noclobber(output)
    }
    .map_err(|e| format!("无法保存文件，请检查权限或关闭已打开的 Word 文件：{e}"))?;
    report.elapsed = began.elapsed();
    Ok(report)
}

fn start<W: Write + std::io::Seek>(zip: &mut ZipWriter<W>, path: &str) -> Result<()> {
    zip.start_file(
        path,
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
    )
    .map_err(|e| e.to_string())
}
fn write<W: Write>(w: &mut W, text: &str) -> Result<()> {
    w.write_all(text.as_bytes()).map_err(|e| e.to_string())
}
fn part<W: Write + std::io::Seek>(zip: &mut ZipWriter<W>, path: &str, text: &str) -> Result<()> {
    start(zip, path)?;
    write(zip, text)
}

struct Image {
    path: PathBuf,
    extension: String,
    width: u64,
    height: u64,
}
struct List {
    id: usize,
    level: usize,
    pending: bool,
}
struct Document<'a, W: Write> {
    writer: &'a mut W,
    base: &'a Path,
    report: Report,
    p: bool,
    style: String,
    strong: usize,
    emphasis: usize,
    strike: usize,
    quote: usize,
    code: bool,
    lists: Vec<List>,
    numbering: Vec<(bool, u64, usize)>,
    table_align: Vec<Alignment>,
    column: usize,
    header: bool,
    cell: bool,
    links: Vec<String>,
    in_link: bool,
    images: Vec<Image>,
    image_paths: HashMap<PathBuf, usize>,
    html_warned: bool,
}
impl<'a, W: Write> Document<'a, W> {
    fn new(writer: &'a mut W, base: &'a Path) -> Self {
        Self {
            writer,
            base,
            report: Report::default(),
            p: false,
            style: String::new(),
            strong: 0,
            emphasis: 0,
            strike: 0,
            quote: 0,
            code: false,
            lists: vec![],
            numbering: vec![],
            table_align: vec![],
            column: 0,
            header: false,
            cell: false,
            links: vec![],
            in_link: false,
            images: vec![],
            image_paths: HashMap::new(),
            html_warned: false,
        }
    }
    fn put(&mut self, s: &str) -> Result<()> {
        write(self.writer, s)
    }
    fn paragraph(&mut self) -> Result<()> {
        if self.p {
            return Ok(());
        }
        self.p = true;
        self.report.paragraphs += 1;
        let mut p = String::from("<w:p><w:pPr>");
        let mut indent = None;
        if !self.style.is_empty() {
            p.push_str(&format!("<w:pStyle w:val=\"{}\"/>", self.style));
        }
        if let Some(list) = self.lists.last_mut() {
            if list.pending {
                p.push_str(&format!(
                    "<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{}\"/></w:numPr>",
                    list.level, list.id
                ));
                list.pending = false;
            } else {
                indent = Some((list.level + 1) * 420);
            }
        }
        if self.quote > 0 {
            p.push_str("<w:pBdr><w:left w:val=\"single\" w:sz=\"16\" w:space=\"10\" w:color=\"CBD5E1\"/></w:pBdr>");
            indent = Some(indent.unwrap_or(0) + self.quote * 420);
        }
        if self.cell {
            p.push_str("<w:spacing w:after=\"80\"/>");
        }
        if let Some(left) = indent {
            p.push_str(&format!("<w:ind w:left=\"{left}\"/>"));
        }
        if self.cell {
            let align = match self.table_align.get(self.column) {
                Some(Alignment::Center) => "center",
                Some(Alignment::Right) => "right",
                _ => "left",
            };
            p.push_str(&format!("<w:jc w:val=\"{align}\"/>"));
        }
        p.push_str("</w:pPr>");
        self.put(&p)
    }
    fn close_paragraph(&mut self) -> Result<()> {
        if self.p {
            self.put("</w:p>")?;
            self.p = false;
        }
        Ok(())
    }
    fn text(&mut self, t: &str, inline_code: bool) -> Result<()> {
        self.paragraph()?;
        let mut s = String::from("<w:r><w:rPr>");
        if self.strong > 0 || self.header {
            s.push_str("<w:b/>");
        }
        if self.emphasis > 0 {
            s.push_str("<w:i/>");
        }
        if self.strike > 0 {
            s.push_str("<w:strike/>");
        }
        if inline_code || self.code {
            s.push_str("<w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\"/><w:sz w:val=\"20\"/><w:shd w:val=\"clear\" w:fill=\"F1F5F9\"/>");
        }
        if self.in_link {
            s.push_str("<w:color w:val=\"2563EB\"/><w:u w:val=\"single\"/>");
        }
        s.push_str("</w:rPr>");
        for (i, line) in t.split('\n').enumerate() {
            if i > 0 {
                s.push_str("<w:br/>");
            }
            for (j, chunk) in line.split('\t').enumerate() {
                if j > 0 {
                    s.push_str("<w:tab/>");
                }
                s.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", xml(chunk)));
            }
        }
        s.push_str("</w:r>");
        self.put(&s)
    }
    fn equation(&mut self, source: &str, display: bool) -> Result<()> {
        match math::to_omml(source, display) {
            Ok(omml) => {
                if display {
                    self.close_paragraph()?;
                    self.paragraph()?;
                    self.put(
                        "<m:oMathPara><m:oMathParaPr><m:jc m:val=\"center\"/></m:oMathParaPr>",
                    )?;
                    self.put(&omml)?;
                    self.put("</m:oMathPara>")?;
                    self.close_paragraph()?;
                } else {
                    self.paragraph()?;
                    self.put(&omml)?;
                }
                self.report.equations += 1;
            }
            Err(error) => {
                let excerpt: String = source.chars().take(60).collect();
                self.report
                    .warn(format!("公式保留为 LaTeX：{excerpt}（{error}）"));
                if display {
                    self.close_paragraph()?;
                }
                self.text(
                    &format!(
                        "{}{}{}",
                        if display { "$$" } else { "$" },
                        source,
                        if display { "$$" } else { "$" }
                    ),
                    true,
                )?;
                if display {
                    self.close_paragraph()?;
                }
            }
        }
        Ok(())
    }
    fn begin(&mut self, tag: Tag<'_>) -> Result<()> {
        match tag {
            Tag::Paragraph => {
                self.close_paragraph()?;
            }
            Tag::Heading { level, .. } => {
                self.close_paragraph()?;
                self.style = format!("Heading{}", level as usize);
            }
            Tag::BlockQuote(_) => {
                self.close_paragraph()?;
                self.quote += 1;
            }
            Tag::CodeBlock(_) => {
                self.close_paragraph()?;
                self.code = true;
                self.style = "Code".into();
            }
            Tag::List(start) => {
                self.close_paragraph()?;
                let level = self.lists.len().min(8);
                self.numbering
                    .push((start.is_some(), start.unwrap_or(1), level));
                self.lists.push(List {
                    id: self.numbering.len(),
                    level,
                    pending: false,
                });
            }
            Tag::Item => {
                self.close_paragraph()?;
                if let Some(l) = self.lists.last_mut() {
                    l.pending = true;
                }
            }
            Tag::Strong => self.strong += 1,
            Tag::Emphasis => self.emphasis += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => {
                self.paragraph()?;
                self.links.push(dest_url.to_string());
                self.put(&format!("<w:hyperlink r:id=\"link{}\">", self.links.len()))?;
                self.in_link = true;
            }
            Tag::Table(align) => {
                self.close_paragraph()?;
                self.table_align = align;
                self.report.tables += 1;
                let width = 9638 / self.table_align.len().max(1);
                self.put("<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"5000\" w:type=\"pct\"/><w:tblLayout w:type=\"fixed\"/></w:tblPr><w:tblGrid>")?;
                for _ in 0..self.table_align.len() {
                    self.put(&format!("<w:gridCol w:w=\"{width}\"/>"))?;
                }
                self.put("</w:tblGrid>")?;
            }
            Tag::TableHead | Tag::TableRow => {
                self.column = 0;
                self.header = matches!(tag, Tag::TableHead);
                self.put(if self.header {
                    "<w:tr><w:trPr><w:tblHeader/></w:trPr>"
                } else {
                    "<w:tr>"
                })?;
            }
            Tag::TableCell => {
                self.cell = true;
                let width = 9638 / self.table_align.len().max(1);
                self.put(&format!(
                    "<w:tc><w:tcPr><w:tcW w:w=\"{width}\" w:type=\"dxa\"/>{}</w:tcPr>",
                    if self.header {
                        "<w:shd w:val=\"clear\" w:fill=\"EFF6FF\"/>"
                    } else {
                        ""
                    }
                ))?;
            }
            _ => {}
        }
        Ok(())
    }
    fn end(&mut self, tag: TagEnd) -> Result<()> {
        match tag {
            TagEnd::Paragraph => {
                self.close_paragraph()?;
            }
            TagEnd::Item => {
                if self.lists.last().is_some_and(|l| l.pending) {
                    self.paragraph()?;
                }
                self.close_paragraph()?;
            }
            TagEnd::Heading(_) => {
                self.paragraph()?;
                self.close_paragraph()?;
                self.style.clear();
            }
            TagEnd::BlockQuote(_) => {
                self.close_paragraph()?;
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                self.close_paragraph()?;
                self.code = false;
                self.style.clear();
            }
            TagEnd::List(_) => {
                self.close_paragraph()?;
                self.lists.pop();
            }
            TagEnd::Strong => self.strong = self.strong.saturating_sub(1),
            TagEnd::Emphasis => self.emphasis = self.emphasis.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                self.put("</w:hyperlink>")?;
                self.in_link = false;
            }
            TagEnd::TableCell => {
                self.paragraph()?;
                self.close_paragraph()?;
                self.put("</w:tc>")?;
                self.column += 1;
                self.cell = false;
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                self.put("</w:tr>")?;
                self.header = false;
            }
            TagEnd::Table => {
                self.put("</w:tbl><w:p/>")?;
                self.table_align.clear();
            }
            _ => {}
        }
        Ok(())
    }
    fn image(&mut self, destination: &str, alt: &str) -> Result<()> {
        let image = self.register_image(destination);
        let id = match image {
            Ok(id) => id,
            Err(error) => {
                self.report
                    .warn(format!("图片保留为引用：{destination}（{error}）"));
                return self.text(&format!("![{alt}]({destination})"), false);
            }
        };
        self.paragraph()?;
        let image = &self.images[id];
        let max_w = if self.cell {
            (6_100_000 / self.table_align.len().max(1) as u64).saturating_sub(140_000)
        } else {
            6_100_000
        };
        let scale = (max_w as f64 / image.width as f64)
            .min(8_000_000f64 / image.height as f64)
            .min(9525.0);
        let (cx, cy) = (
            (image.width as f64 * scale) as u64,
            (image.height as f64 * scale) as u64,
        );
        let n = id + 1;
        // Each occurrence needs its own document-wide unique drawing identifier.
        self.report.images += 1;
        let drawing = self.report.images;
        self.put(&format!("<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{drawing}\" name=\"图片 {drawing}\" descr=\"{}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"image{n}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"image{n}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>", xml(alt)))
    }
    fn register_image(&mut self, destination: &str) -> Result<usize> {
        if destination.contains("://") || destination.starts_with("data:") {
            return Err("请先把图片下载到本地，再使用相对路径".into());
        }
        let decoded = percent_encoding::percent_decode_str(destination)
            .decode_utf8()
            .map_err(|e| e.to_string())?;
        let path = self
            .base
            .join(decoded.as_ref())
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if let Some(id) = self.image_paths.get(&path) {
            return Ok(*id);
        }
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg") {
            return Err("目前支持 PNG、JPG 图片".into());
        }
        let size = imagesize::size(&path).map_err(|e| e.to_string())?;
        if size.width == 0 || size.height == 0 {
            return Err("图片尺寸无效".into());
        }
        let id = self.images.len();
        self.image_paths.insert(path.clone(), id);
        self.images.push(Image {
            path,
            extension,
            width: size.width as u64,
            height: size.height as u64,
        });
        Ok(id)
    }
}

/// Recognize ChatGPT/LaTeX delimiters without changing code spans or code blocks.
fn normalize_math_delimiters(source: &str) -> Cow<'_, str> {
    if !source.contains(r"\(") && !source.contains(r"\[") {
        return Cow::Borrowed(source);
    }
    let protected: Vec<_> = Parser::new(source)
        .into_offset_iter()
        .filter_map(|(e, r)| {
            if matches!(e, Event::Code(_) | Event::Start(Tag::CodeBlock(_))) {
                Some(r)
            } else {
                None
            }
        })
        .collect();
    let (mut out, mut i, mut region) = (String::with_capacity(source.len()), 0, 0);
    while i < source.len() {
        while region < protected.len() && protected[region].end <= i {
            region += 1;
        }
        if let Some(r) = protected.get(region).filter(|r| r.start <= i && i < r.end) {
            out.push_str(&source[i..r.end]);
            i = r.end;
            continue;
        }
        let rest = &source[i..];
        let closing = if rest.starts_with(r"\(") {
            Some((r"\)", "$"))
        } else if rest.starts_with(r"\[") {
            Some((r"\]", "$$"))
        } else {
            None
        };
        if let Some((closing, delimiter)) = closing {
            if let Some(end) = source[i + 2..].find(closing).map(|j| i + 2 + j) {
                if protected.get(region).is_none_or(|r| r.start >= end + 2) {
                    out.push_str(delimiter);
                    out.push_str(source[i + 2..end].trim());
                    out.push_str(delimiter);
                    i = end + 2;
                    continue;
                }
            }
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
        // An escaped backslash must not start a math delimiter.
        if c == '\\' && source[i..].starts_with('\\') {
            out.push('\\');
            i += 1;
        }
    }
    Cow::Owned(out)
}

fn content_types() -> String {
    let mut s = String::from(
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>",
    );
    for (path, kind) in [
        ("document", "document.main"),
        ("styles", "styles"),
        ("numbering", "numbering"),
        ("settings", "settings"),
    ] {
        s.push_str(&format!("<Override PartName=\"/word/{path}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml\"/>"));
    }
    s.push_str("</Types>");
    s
}
fn styles() -> String {
    let mut s = format!(
        "<w:styles xmlns:w=\"{W_NS}\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:eastAsia=\"宋体\"/><w:sz w:val=\"22\"/><w:lang w:val=\"en-US\" w:eastAsia=\"zh-CN\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"140\" w:line=\"300\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>"
    );
    for (level, size) in [36, 30, 26, 24, 22, 22].iter().enumerate() {
        let n = level + 1;
        s.push_str(&format!("<w:style w:type=\"paragraph\" w:styleId=\"Heading{n}\"><w:name w:val=\"heading {n}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"240\" w:after=\"120\"/><w:outlineLvl w:val=\"{level}\"/></w:pPr><w:rPr><w:rFonts w:eastAsia=\"黑体\"/><w:b/><w:color w:val=\"183153\"/><w:sz w:val=\"{size}\"/></w:rPr></w:style>"));
    }
    s.push_str("<w:style w:type=\"paragraph\" w:styleId=\"Code\"><w:name w:val=\"Code\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:shd w:val=\"clear\" w:fill=\"F1F5F9\"/><w:spacing w:before=\"120\" w:after=\"120\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:style><w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:tblPr><w:tblBorders>");
    for edge in ["top", "left", "bottom", "right", "insideH", "insideV"] {
        s.push_str(&format!(
            "<w:{edge} w:val=\"single\" w:sz=\"4\" w:color=\"CBD5E1\"/>"
        ));
    }
    s.push_str("</w:tblBorders><w:tblCellMar><w:top w:w=\"100\" w:type=\"dxa\"/><w:left w:w=\"100\" w:type=\"dxa\"/><w:bottom w:w=\"100\" w:type=\"dxa\"/><w:right w:w=\"100\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style></w:styles>");
    s
}
fn numbering_xml(numbers: &[(bool, u64, usize)]) -> String {
    let mut s = format!("<w:numbering xmlns:w=\"{W_NS}\">");
    for ordered in [false, true] {
        let id = usize::from(ordered);
        s.push_str(&format!(
            "<w:abstractNum w:abstractNumId=\"{id}\"><w:multiLevelType w:val=\"multilevel\"/>"
        ));
        for level in 0..9 {
            let fmt = if ordered { "decimal" } else { "bullet" };
            let text = if ordered {
                format!("%{}.", level + 1)
            } else {
                "•".into()
            };
            s.push_str(&format!("<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:tabs><w:tab w:val=\"num\" w:pos=\"{}\"/></w:tabs><w:ind w:left=\"{}\" w:hanging=\"280\"/></w:pPr></w:lvl>", (level+1)*420,(level+1)*420));
        }
        s.push_str("</w:abstractNum>");
    }
    for (i, (ordered, start, level)) in numbers.iter().enumerate() {
        s.push_str(&format!("<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"{}\"/><w:lvlOverride w:ilvl=\"{level}\"><w:startOverride w:val=\"{start}\"/></w:lvlOverride></w:num>",i+1,usize::from(*ordered)));
    }
    s.push_str("</w:numbering>");
    s
}
