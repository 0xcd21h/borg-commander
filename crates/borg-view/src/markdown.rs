use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::paint::{Paint, RichText, highlight};

const ACCENT: Paint = Paint {
    dark: Some([136, 192, 208]),
    light: Some([0, 92, 128]),
    bold: false,
    italic: false,
    underline: false,
    dim: false,
};

pub(crate) fn render(source: &str) -> RichText {
    let mut out = RichText::default();
    let mut paint = Paint::default();
    let mut stack = Vec::new();
    let mut lists = Vec::<Option<u64>>::new();
    let mut links = Vec::new();
    let mut code = None::<(String, String)>;
    let mut cell = 0;
    for event in
        Parser::new_ext(source, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS)
    {
        match event {
            Event::Start(tag) => {
                stack.push(paint);
                match tag {
                    Tag::Heading { .. } => {
                        newline(&mut out);
                        paint = Paint { bold: true, ..ACCENT };
                    }
                    Tag::Emphasis => paint.italic = true,
                    Tag::Strong => paint.bold = true,
                    Tag::Strikethrough => paint.dim = true,
                    Tag::BlockQuote(_) => {
                        newline(&mut out);
                        out.push("│ ", ACCENT);
                    }
                    Tag::List(start) => {
                        newline(&mut out);
                        lists.push(start);
                    }
                    Tag::Item => {
                        newline(&mut out);
                        out.push(&"  ".repeat(lists.len().saturating_sub(1)), paint);
                        let marker = match lists.last_mut() {
                            Some(Some(number)) => {
                                let s = format!("{number}. ");
                                *number += 1;
                                s
                            }
                            _ => "• ".into(),
                        };
                        out.push(&marker, ACCENT);
                    }
                    Tag::CodeBlock(kind) => {
                        newline(&mut out);
                        if !out.text.is_empty() && !out.text.ends_with("\n\n") {
                            out.push("\n", paint);
                        }
                        let language = match kind {
                            CodeBlockKind::Fenced(name) => name.split_whitespace().next().unwrap_or("").to_owned(),
                            CodeBlockKind::Indented => String::new(),
                        };
                        code = Some((language, String::new()));
                    }
                    Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                        links.push(dest_url.into_string());
                        paint = Paint { underline: true, ..ACCENT };
                    }
                    Tag::TableHead | Tag::TableRow => {
                        newline(&mut out);
                        cell = 0;
                    }
                    Tag::TableCell => {
                        if cell > 0 {
                            out.push(" │ ", ACCENT);
                        }
                        cell += 1;
                    }
                    _ => {}
                }
            }
            Event::End(tag) => {
                paint = stack.pop().unwrap_or_default();
                match tag {
                    TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::BlockQuote(_) | TagEnd::Table => {
                        newline(&mut out);
                        out.push("\n", paint);
                    }
                    TagEnd::Item | TagEnd::TableHead | TagEnd::TableRow => newline(&mut out),
                    TagEnd::List(_) => {
                        lists.pop();
                        newline(&mut out);
                    }
                    TagEnd::CodeBlock => {
                        if let Some((language, text)) = code.take() {
                            let rich = highlight(&text, &language, 0);
                            for (range, style) in rich.regions {
                                out.push(&rich.text[range.start as usize..range.end as usize], style);
                            }
                        }
                        newline(&mut out);
                        out.push("\n", paint);
                    }
                    TagEnd::Link | TagEnd::Image => {
                        if let Some(url) = links.pop() {
                            out.push(&format!(" ({url})"), Paint { dim: true, ..paint });
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(text) => match code.as_mut() {
                Some((_, body)) => body.push_str(&text),
                None => out.push(&text, paint),
            },
            Event::Code(text) => out.push(&text, Paint { bold: true, ..ACCENT }),
            Event::SoftBreak => out.push(" ", paint),
            Event::HardBreak => out.push("\n", paint),
            Event::Rule => {
                newline(&mut out);
                out.push("────────────────────────\n\n", ACCENT);
            }
            Event::TaskListMarker(done) => out.push(if done { "[x] " } else { "[ ] " }, ACCENT),
            Event::Html(text) | Event::InlineHtml(text) => out.push(&text, Paint { dim: true, ..paint }),
            Event::FootnoteReference(text) => out.push(&format!("[{text}]"), ACCENT),
            _ => {}
        }
    }
    out
}

fn newline(out: &mut RichText) {
    if !out.text.is_empty() && !out.text.ends_with('\n') {
        out.push("\n", Paint::default());
    }
}
