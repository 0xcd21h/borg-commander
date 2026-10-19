use std::{path::Path, sync::Arc};

use borg_view::Pager;

fn document(path: &str, content: &str, width: usize, height: usize) -> Pager {
    Pager::for_path(Arc::new(content.as_bytes().to_vec()), Path::new(path), width, height).unwrap()
}

#[test]
fn recognises_common_languages_and_special_filenames() {
    for (path, content) in [
        ("main.rs", "fn main() { println!(\"hello\"); }"),
        ("main.py", "def greet():\n    return 'hello'"),
        ("app.ts", "const name: string = 'hello';"),
        ("app.tsx", "export const View = () => <div>Hello</div>;"),
        ("app.js", "const greeting = 'hello';"),
        ("main.go", "package main\nfunc main() {}"),
        ("main.c", "#include <stdio.h>\nint main() { return 0; }"),
        ("main.cpp", "#include <iostream>\nint main() { return 0; }"),
        ("Main.java", "class Main { public static void main(String[] args) {} }"),
        ("Main.cs", "class Main { static void Main() {} }"),
        ("index.php", "<?php echo 'hello';"),
        ("main.rb", "def greet\n  puts 'hello'\nend"),
        ("init.lua", "local name = 'hello'"),
        ("query.sql", "SELECT name FROM users WHERE id = 1;"),
        ("config.yaml", "name: hello\nenabled: true"),
        ("config.json", "{\"name\": \"hello\"}"),
        ("Cargo.toml", "[package]\nname = \"hello\""),
        ("index.html", "<html><body>Hello</body></html>"),
        ("style.css", "body { color: red; }"),
        ("Dockerfile", "FROM alpine:latest\nRUN echo hello"),
        ("script", "#!/bin/bash\necho hello"),
    ] {
        let pager = document(path, content, 80, 10);
        assert!(pager.language().is_some(), "missing grammar for {path}");
        assert!(
            pager.rich_rows().iter().flatten().any(|run| run.paint.dark.is_some() && run.paint.light.is_some()),
            "no highlights for {path}"
        );
    }
}

#[test]
fn markdown_opens_rendered_and_source_toggle_restores_each_position() {
    let source =
        "# Heading\n\nA **bold** word and [link](https://example.com).\n\n- [x] done\n\n```rust\nfn main() {}\n```\n";
    let mut pager = document("README.md", source, 60, 4);
    assert!(pager.formatted());
    assert_eq!(pager.rows()[0], "Heading");
    assert!(pager.rich_rows()[0][0].paint.bold);
    pager.end().unwrap();
    let end = pager.offset();
    assert!(pager.rows().iter().any(|row| row.contains("fn main()")));
    pager.toggle_source().unwrap();
    assert!(!pager.formatted());
    assert_eq!(pager.rows()[0], "# Heading");
    pager.scroll_down(1).unwrap();
    let raw = pager.offset();
    pager.toggle_source().unwrap();
    assert_eq!(pager.offset(), end);
    pager.toggle_source().unwrap();
    assert_eq!(pager.offset(), raw);
}

#[test]
fn syntax_survives_wrapping_horizontal_scroll_and_multiline_comments() {
    let mut pager = document("main.rs", "/* comment\nstill a comment\n*/\nfn main() { let x = \"text\"; }\n", 10, 2);
    let first = pager.rich_rows()[0][0].paint;
    pager.scroll_down(2).unwrap();
    assert_eq!(pager.rich_rows()[0][0].paint.dark, first.dark);
    pager.end().unwrap();
    pager.toggle_wrap().unwrap();
    pager.end().unwrap();
    pager.scroll_right().unwrap();
    assert_eq!(pager.rows().last().unwrap(), ") { let x ");
    assert!(pager.rich_rows().iter().flatten().any(|run| run.paint.dark.is_some()));
}

#[test]
fn plain_text_preserves_tabs_unicode_and_masks_terminal_control_sequences() {
    let pager = document("notes.txt", "a\t日本\x1b[31m\n", 40, 3);
    assert_eq!(pager.rows()[0], "a       日本·[31m");
    assert_eq!(pager.rich_rows()[0].iter().map(|run| run.text.as_str()).collect::<String>(), pager.rows()[0]);
}

#[test]
fn giant_lines_and_binary_content_use_plain_text_without_expensive_parsing() {
    let minified = "x".repeat(64 * 1024);
    let pager = document("generated.js", &minified, 60, 5);
    assert_eq!(pager.rows().len(), 5);
    assert!(pager.rich_rows().iter().flatten().all(|run| run.paint.dark.is_none()));
    let binary = document("dump.md", "first line\n\0binary", 60, 5);
    assert!(!binary.formatted());
    assert!(binary.language().is_none());
}

#[test]
fn giant_line_before_a_syntax_window_stays_unstyled_when_jumping_to_the_end() {
    let source = format!("{}\nlet answer = 42;\n", "x".repeat(400_000));
    let mut pager = document("generated.js", &source, 60, 3);
    pager.end().unwrap();
    assert!(pager.rows()[0].starts_with('x'));
    assert!(pager.rich_rows()[0].iter().all(|run| run.paint.dark.is_none()));
    assert!(pager.rich_rows().last().unwrap().iter().any(|run| run.paint.dark.is_some()));
}
