use md_to_word::convert::convert;
use std::{fs, io::Read};

fn part(path: &std::path::Path, name: &str) -> String {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}
fn fixture(
    source: &str,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    md_to_word::convert::Report,
) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("数学 练习.md");
    let output = dir.path().join("数学 练习.docx");
    fs::write(&input, source).unwrap();
    let report = convert(&input, &output, false).unwrap();
    (dir, output, report)
}

#[test]
fn complete_package_is_well_formed_and_keeps_editable_structures() {
    let (_dir, path, report) = fixture(include_str!("../examples/数学示例.md"));
    assert_eq!(report.warning_count, 0, "{:?}", report.warnings);
    assert!(report.equations >= 12);
    assert_eq!(report.tables, 1);
    let mut zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let name = file.name().to_owned();
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let mut s = String::new();
            file.read_to_string(&mut s).unwrap();
            roxmltree::Document::parse(&s).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
    let document = part(&path, "word/document.xml");
    let tree = roxmltree::Document::parse(&document).unwrap();
    for name in ["oMath", "f", "rad", "sSup", "sSub", "m", "tbl", "hyperlink"] {
        assert!(
            tree.descendants()
                .any(|n| n.is_element() && n.tag_name().name() == name),
            "missing {name}"
        );
    }
    let rels = part(&path, "word/_rels/document.xml.rels");
    let tree = roxmltree::Document::parse(&rels).unwrap();
    for rel in tree.root_element().children().filter(|n| n.is_element()) {
        if rel.attribute("TargetMode") != Some("External") {
            assert!(
                zip.by_name(&format!("word/{}", rel.attribute("Target").unwrap()))
                    .is_ok()
            );
        }
    }
}

#[test]
fn unsupported_math_and_missing_images_preserve_original_content() {
    let (_dir, path, report) = fixture(r"保留 $\unsupported{x}$ 和 ![原图](missing.png)");
    assert_eq!(report.warning_count, 2);
    let document = part(&path, "word/document.xml");
    assert!(document.contains(r"$\unsupported{x}$"));
    assert!(document.contains("![原图](missing.png)"));
}
#[test]
fn original_output_survives_read_failure_and_overwrite_is_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.md");
    let output = dir.path().join("out.docx");
    fs::write(&input, "new content").unwrap();
    fs::write(&output, b"existing file").unwrap();
    assert!(convert(&input, &output, false).is_err());
    assert_eq!(fs::read(&output).unwrap(), b"existing file");
    fs::write(&input, [0xff, 0xfe]).unwrap();
    assert!(convert(&input, &output, true).is_err());
    assert_eq!(fs::read(&output).unwrap(), b"existing file");
    fs::write(&input, "new content").unwrap();
    convert(&input, &output, true).unwrap();
    assert!(part(&output, "word/document.xml").contains("new content"));
}
#[test]
fn loose_lists_do_not_grow_empty_paragraphs_and_starts_are_kept() {
    let (_dir, path, report) = fixture("3. 第一项\n\n4. 第二项\n\n5. 第三项\n");
    assert_eq!(report.paragraphs, 3);
    let numbers = part(&path, "word/numbering.xml");
    assert!(numbers.contains("<w:startOverride w:val=\"3\"/>"));
    let doc = part(&path, "word/document.xml");
    assert_eq!(doc.matches("<w:numPr>").count(), 3);
}
#[test]
fn latex_delimiters_are_converted_except_inside_code() {
    let (_dir, path, report) = fixture(
        "内联 \\(x^2\\)\n\n\\[\n\\frac{1}{2}\n\\]\n\n`\\(code\\)`\n\n```text\n\\[not math\\]\n```\n",
    );
    assert_eq!(report.equations, 2);
    let document = part(&path, "word/document.xml");
    assert!(document.contains(r"\(code\)"));
    assert!(document.contains(r"\[not math\]"));
}
#[test]
fn xml_special_characters_and_empty_table_cells_are_valid() {
    let (_dir, path, _report) =
        fixture("# &amp; <标题>\n\n| A | B |\n|---|---|\n| | $a < b$ |\n\n`<x & y>`\n");
    let document = part(&path, "word/document.xml");
    roxmltree::Document::parse(&document).unwrap();
    assert!(document.contains("&lt;x &amp; y&gt;"));
}
#[test]
fn relative_png_is_embedded_once_even_when_reused() {
    let dir = tempfile::tempdir().unwrap();
    // 1x1 transparent PNG, read without image decoding.
    let png: [u8; 67] = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 10, 73, 68, 65, 84, 120, 156, 99, 0, 1, 0, 0, 5, 0, 1,
        13, 10, 45, 180, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    fs::write(dir.path().join("图 片.png"), png).unwrap();
    let input = dir.path().join("in.md");
    let output = dir.path().join("out.docx");
    fs::write(&input, "![图](图%20片.png)\n\n![重复](图%20片.png)").unwrap();
    let report = convert(&input, &output, false).unwrap();
    assert_eq!(report.images, 2);
    assert_eq!(report.warning_count, 0);
    let mut archive = zip::ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
    assert_eq!(archive.by_name("word/media/image1.png").unwrap().size(), 67);
    assert!(archive.by_name("word/media/image2.png").is_err());
    let document = part(&output, "word/document.xml");
    assert!(document.contains("<wp:docPr id=\"1\""));
    assert!(document.contains("<wp:docPr id=\"2\""));
}
#[test]
fn malformed_formulas_do_not_disappear() {
    for formula in [
        r"\frac{1}",
        r"x^{",
        r"\sqrt",
        r"a_",
        r"\begin{matrix}1 & 2",
        r"\text{a & b}",
    ] {
        let source = format!("before ${formula}$ after");
        let (_dir, path, report) = fixture(&source);
        let document = part(&path, "word/document.xml");
        roxmltree::Document::parse(&document).unwrap();
        assert!(document.contains("before"));
        assert!(document.contains("after"));
        if report.equations == 0 && report.warning_count == 0 {
            // CommonMark treats unbalanced math braces as ordinary text.
            let tree = roxmltree::Document::parse(&document).unwrap();
            let text: String = tree
                .descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect();
            assert!(
                text.contains(&format!("${formula}$")),
                "original formula lost: {formula}"
            );
        }
    }
}

#[test]
fn source_line_breaks_keep_exam_metadata_on_separate_lines() {
    let (_dir, path, report) = fixture(
        "**教材版本**：北师大版　**考查范围**：九年级上册\n**满分**：150 分　**考试时间**：120 分钟\n**适用对象**：某同学（现有水平约 130 分，目标 145 分）\n",
    );
    assert_eq!(report.paragraphs, 1);
    let document = part(&path, "word/document.xml");
    let tree = roxmltree::Document::parse(&document).unwrap();
    assert_eq!(
        tree.descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "br")
            .count(),
        2
    );
    let text: String = tree
        .descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect();
    assert!(
        !text.contains("**"),
        "bold markers must become Word formatting"
    );
    assert!(document.contains("<w:b/>"));
    let mut lines = vec![String::new()];
    for node in tree.descendants().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "t" => lines
                .last_mut()
                .unwrap()
                .push_str(node.text().unwrap_or("")),
            "br" => lines.push(String::new()),
            _ => {}
        }
    }
    assert!(lines[0].starts_with("教材版本："));
    assert!(lines[1].starts_with("满分："));
    assert!(lines[2].starts_with("适用对象："));
}

#[test]
fn line_breaks_do_not_change_paragraph_lists_or_code_structure() {
    let (_dir, path, report) = fixture(
        "第一行\n第二行\n\n新段落\n\n- 列表第一行\n  列表第二行\n\n```text\n代码甲\n代码乙\n```\n",
    );
    assert_eq!(report.paragraphs, 4);
    let document = part(&path, "word/document.xml");
    assert_eq!(document.matches("<w:numPr>").count(), 1);
    assert_eq!(document.matches("<w:br/>").count(), 4);
}
