//! Only one equation is materialized at a time. Unsupported syntax is returned
//! as an error so the caller preserves the original formula and reports it.
use crate::xml;
use roxmltree::Node;

pub fn to_omml(latex: &str, display: bool) -> Result<String, String> {
    if latex.len() > 16_384 || nesting(latex) > 96 || latex.matches(['\\', '^', '_']).count() > 256
    {
        return Err("公式过长或嵌套过深".into());
    }
    let (latex, text_runs) = extract_text_runs(latex)?;
    let latex = validated_latex(&latex)?;
    let style = if display {
        latex2mathml::DisplayStyle::Block
    } else {
        latex2mathml::DisplayStyle::Inline
    };
    let mathml = latex2mathml::latex_to_mathml(&latex, style)
        .map_err(|e| e.to_string())?
        .replace("&lang;", "⟨")
        .replace("&rang;", "⟩")
        .replace("&#x0338;", "\u{0338}")
        .replace("&#x2061;", "\u{2061}");
    if mathml.contains("[PARSE ERROR:") {
        return Err("公式含有未识别的语法".into());
    }
    let mathml = escape_mathml_leaves(&mathml);
    let tree = roxmltree::Document::parse(&mathml).map_err(|e| e.to_string())?;
    let mut omml = format!("<m:oMath>{}</m:oMath>", render(tree.root_element(), 0)?);
    for (marker, text) in text_runs {
        omml = omml.replace(marker, &xml(&text));
    }
    Ok(omml)
}

/// Keep punctuation, digits, and spaces in \text{} out of the math lexer.
fn extract_text_runs(source: &str) -> Result<(String, Vec<(char, String)>), String> {
    let mut chars = source.chars().peekable();
    let mut processed = String::with_capacity(source.len());
    let mut texts = Vec::new();
    let mut marker_code = 0xf0000;
    while let Some(c) = chars.next() {
        if c != '\\' {
            processed.push(c);
            continue;
        }
        let Some(first) = chars.next() else {
            processed.push(c);
            break;
        };
        let mut command = String::from(first);
        if first.is_ascii_alphabetic() {
            while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                command.push(chars.next().unwrap());
            }
        }
        if !matches!(command.as_str(), "text" | "operatorname") {
            processed.push('\\');
            processed.push_str(&command);
            continue;
        }
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.next() != Some('{') {
            return Err(format!("\\{command} 后需要花括号"));
        }
        let mut depth = 1;
        let mut text = String::new();
        while let Some(c) = chars.next() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                '\\' => {
                    let escaped = chars.next().ok_or("文本命令不完整")?;
                    if !matches!(
                        escaped,
                        '{' | '}' | '&' | '%' | '_' | '#' | '$' | ' ' | '\\'
                    ) {
                        return Err("暂不支持在文本命令中嵌套 LaTeX 命令".into());
                    }
                    text.push(escaped);
                }
                '\n' | '\r' => text.push(' '),
                _ => text.push(c),
            }
        }
        if depth != 0 {
            return Err("文本花括号不匹配".into());
        }
        while source.contains(char::from_u32(marker_code).unwrap()) {
            marker_code += 1;
        }
        let marker = char::from_u32(marker_code).ok_or("文本块过多")?;
        marker_code += 1;
        processed.push(marker);
        texts.push((marker, text));
    }
    Ok((processed, texts))
}

/// The parser's MathML serializer does not escape text in token elements.
/// Escape complete leaf contents before asking an XML parser to read them.
fn escape_mathml_leaves(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut i = 0;
    while i < source.len() {
        let rest = &source[i..];
        let leaf = [
            ("mi", "<mi"),
            ("mo", "<mo"),
            ("mn", "<mn"),
            ("mtext", "<mtext"),
            ("ms", "<ms"),
        ]
        .into_iter()
        .find(|(_, prefix)| {
            rest.strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with(['>', ' ']))
        });
        if let Some((leaf, _)) = leaf {
            if let Some(open_end) = rest.find('>') {
                let closing = format!("</{leaf}>");
                if let Some(end) = rest[open_end + 1..]
                    .find(&closing)
                    .map(|p| p + open_end + 1)
                {
                    result.push_str(&rest[..open_end + 1]);
                    result.push_str(&xml(&rest[open_end + 1..end]));
                    result.push_str(&closing);
                    i += end + closing.len();
                    continue;
                }
            }
        }
        let end = if rest.starts_with('<') {
            rest.find('>').map(|p| p + 1).unwrap_or(rest.len())
        } else {
            rest.find('<').unwrap_or(rest.len())
        };
        result.push_str(&rest[..end]);
        i += end;
    }
    result
}

fn validated_latex(source: &str) -> Result<String, String> {
    let mut chars = source.chars().peekable();
    let mut result = String::with_capacity(source.len());
    let mut braces = 0usize;
    while let Some(c) = chars.next() {
        if c == '\\' {
            let first = chars.next().ok_or("公式末尾有未完成的命令")?;
            let mut command = String::from(first);
            if first.is_ascii_alphabetic() {
                while chars.peek().is_some_and(char::is_ascii_alphabetic) {
                    command.push(chars.next().unwrap());
                }
            }
            let normalized = match command.as_str() {
                "dfrac" | "tfrac" => "frac",
                "mathcal" => "mathscr",
                "dots" => "ldots",
                other => other,
            };
            if matches!(
                latex2mathml::token::Token::from_command(normalized),
                latex2mathml::token::Token::Command(_)
            ) {
                return Err(format!("暂不支持命令 \\{command}"));
            }
            result.push('\\');
            result.push_str(normalized);
        } else {
            if c == '{' {
                braces += 1;
            }
            if c == '}' {
                braces = braces.checked_sub(1).ok_or("公式花括号不匹配")?;
            }
            result.push(c);
        }
    }
    if braces != 0 {
        return Err("公式花括号不匹配".into());
    }
    Ok(result)
}

fn nesting(s: &str) -> usize {
    let (mut depth, mut max) = (0usize, 0usize);
    for c in s.chars() {
        if c == '{' {
            depth += 1;
            max = max.max(depth);
        }
        if c == '}' {
            depth = depth.saturating_sub(1);
        }
    }
    max
}

fn run(s: &str, variant: &str) -> String {
    let (script, style) = match variant {
        "bold" => ("roman", "b"),
        "bold-italic" => ("roman", "bi"),
        "double-struck" => ("double-struck", "p"),
        "fraktur" => ("fraktur", "p"),
        "bold-fraktur" => ("fraktur", "b"),
        "script" => ("script", "p"),
        "bold-script" => ("script", "b"),
        "sans-serif" => ("sans-serif", "p"),
        "monospace" => ("monospace", "p"),
        "normal" => ("roman", "p"),
        _ => ("roman", "i"),
    };
    let prop = format!("<m:rPr><m:scr m:val=\"{script}\"/><m:sty m:val=\"{style}\"/></m:rPr>");
    format!(
        "<m:r>{prop}<m:t xml:space=\"preserve\">{}</m:t></m:r>",
        xml(s)
    )
}

fn render(n: Node<'_, '_>, depth: usize) -> Result<String, String> {
    if depth > 128 {
        return Err("公式结构过深".into());
    }
    let children: Vec<_> = n.children().filter(Node::is_element).collect();
    let all =
        || -> Result<String, String> { children.iter().map(|&c| render(c, depth + 1)).collect() };
    let arg = |i: usize| -> Result<String, String> {
        render(*children.get(i).ok_or("公式参数不完整")?, depth + 1)
    };
    Ok(match n.tag_name().name() {
        "math" | "mstyle" | "mpadded" => all()?,
        "mrow" => {
            let first = children.first().and_then(Node::text).unwrap_or("");
            let last = children.last().and_then(Node::text).unwrap_or("");
            if children.len() >= 3
                && matches!(first, "(" | "[" | "{" | "|" | "‖" | "⟨")
                && matches!(last, ")" | "]" | "}" | "|" | "‖" | "⟩")
            {
                let inner: String = children[1..children.len() - 1]
                    .iter()
                    .map(|&c| render(c, depth + 1))
                    .collect::<Result<_, _>>()?;
                format!(
                    "<m:d><m:dPr><m:begChr m:val=\"{}\"/><m:endChr m:val=\"{}\"/></m:dPr><m:e>{inner}</m:e></m:d>",
                    xml(first),
                    xml(last)
                )
            } else {
                all()?
            }
        }
        "mi" | "mn" | "mo" | "mtext" | "ms" => {
            let text: String = n
                .children()
                .filter(Node::is_text)
                .filter_map(|c| c.text())
                .collect();
            run(
                &text,
                n.attribute("mathvariant")
                    .unwrap_or(if n.tag_name().name() == "mi" {
                        "italic"
                    } else {
                        "normal"
                    }),
            )
        }
        "mspace" => run(" ", "normal"),
        "mfrac" => {
            let prop = if n.attribute("linethickness") == Some("0") {
                "<m:fPr><m:type m:val=\"noBar\"/></m:fPr>"
            } else {
                ""
            };
            format!(
                "<m:f>{prop}<m:num>{}</m:num><m:den>{}</m:den></m:f>",
                arg(0)?,
                arg(1)?
            )
        }
        "msqrt" => format!(
            "<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/><m:e>{}</m:e></m:rad>",
            all()?
        ),
        "mroot" => format!(
            "<m:rad><m:deg>{}</m:deg><m:e>{}</m:e></m:rad>",
            arg(1)?,
            arg(0)?
        ),
        "msub" => format!(
            "<m:sSub><m:e>{}</m:e><m:sub>{}</m:sub></m:sSub>",
            arg(0)?,
            arg(1)?
        ),
        "msup" => format!(
            "<m:sSup><m:e>{}</m:e><m:sup>{}</m:sup></m:sSup>",
            arg(0)?,
            arg(1)?
        ),
        "msubsup" => format!(
            "<m:sSubSup><m:e>{}</m:e><m:sub>{}</m:sub><m:sup>{}</m:sup></m:sSubSup>",
            arg(0)?,
            arg(1)?,
            arg(2)?
        ),
        "munder" => format!(
            "<m:limLow><m:e>{}</m:e><m:lim>{}</m:lim></m:limLow>",
            arg(0)?,
            arg(1)?
        ),
        "mover" => {
            let accent = children.get(1).and_then(Node::text).unwrap_or("");
            if children.get(1).and_then(|n| n.attribute("accent")) == Some("true") {
                format!(
                    "<m:acc><m:accPr><m:chr m:val=\"{}\"/></m:accPr><m:e>{}</m:e></m:acc>",
                    xml(accent),
                    arg(0)?
                )
            } else {
                format!(
                    "<m:limUpp><m:e>{}</m:e><m:lim>{}</m:lim></m:limUpp>",
                    arg(0)?,
                    arg(1)?
                )
            }
        }
        "munderover" => format!(
            "<m:limUpp><m:e><m:limLow><m:e>{}</m:e><m:lim>{}</m:lim></m:limLow></m:e><m:lim>{}</m:lim></m:limUpp>",
            arg(0)?,
            arg(1)?,
            arg(2)?
        ),
        "mtable" => format!("<m:m>{}</m:m>", all()?),
        "mtr" => format!("<m:mr>{}</m:mr>", all()?),
        "mtd" => format!("<m:e>{}</m:e>", all()?),
        other => return Err(format!("暂不支持的公式结构：{other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editable_equation_structures() {
        for (source, element) in [
            (r"\frac{a+b}{2}", "m:f"),
            (r"\sqrt{x}", "m:rad"),
            (r"\sqrt[3]{x}", "m:rad"),
            ("x^2", "m:sSup"),
            ("a_n", "m:sSub"),
            (r"\begin{pmatrix}1 & 2 \\ 3 & 4\end{pmatrix}", "m:m"),
        ] {
            let out = to_omml(source, true).unwrap();
            assert!(out.contains(&format!("<{element}>")), "{source}: {out}");
            let wrapper = format!(
                "<root xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">{out}</root>"
            );
            roxmltree::Document::parse(&wrapper).unwrap();
        }
    }
    #[test]
    fn unknown_command_is_not_silently_lost() {
        assert!(to_omml(r"\madeupcommand{x}", false).is_err());
        assert!(to_omml(&"{".repeat(1000), false).is_err());
    }

    #[test]
    fn comparisons_dots_and_units_keep_their_content() {
        for source in [
            r"1\ \text{cm/s}",
            r"\Delta=36-48=-12<0",
            r"0<t<6",
            r"\frac12\times4\sqrt5\times EF=\frac12\times5\times(\text{边})\dots",
            r"\text{速度 1 & 2 < 3}",
        ] {
            let out = to_omml(source, false).unwrap_or_else(|e| panic!("{source}: {e}"));
            let wrapper = format!(
                "<root xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">{out}</root>"
            );
            roxmltree::Document::parse(&wrapper).unwrap();
            if source.contains("cm/s") {
                assert!(out.contains("cm/s"));
            }
            if source.contains("速度") {
                assert!(out.contains("速度 1 &amp; 2 &lt; 3"));
            }
        }
    }
}
