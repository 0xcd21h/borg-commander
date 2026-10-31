//! JSON laid out one value to a line, as files written on a single line read best: keys keep
//! their order, and numbers and strings their text, so nothing but the spacing changes.

use serde::de::IgnoredAny;

/// How much is laid out between asking whether to go on.
const CHECK_EVERY: usize = 1024 * 1024;

/// `text` laid out with `indent` per level, or why it is not JSON. `go_on` stops it by
/// answering false, which gives `None`.
pub fn json(text: &str, indent: &str, go_on: &dyn Fn() -> bool) -> Option<Result<String, String>> {
    if let Err(error) = serde_json::from_str::<IgnoredAny>(text) {
        return Some(Err(error.to_string()));
    }
    let mut out = String::with_capacity(text.len() * 2);
    lay_out(text, indent, go_on, &mut out)?;
    Some(Ok(out))
}

/// JSON Lines laid out a record at a time, leaving lines that are not JSON as they are; an
/// error when none is.
pub fn json_lines(text: &str, indent: &str, go_on: &dyn Fn() -> bool) -> Option<Result<String, String>> {
    let mut out = String::with_capacity(text.len() * 2);
    let mut first_error = None;
    let mut any = false;
    for line in text.lines() {
        match serde_json::from_str::<IgnoredAny>(line) {
            Ok(_) => {
                lay_out(line, indent, go_on, &mut out)?;
                any = true;
            }
            Err(error) => {
                first_error.get_or_insert_with(|| error.to_string());
                out.push_str(line);
            }
        }
        out.push('\n');
    }
    Some(if any { Ok(out) } else { Err(first_error.unwrap_or_default()) })
}

/// Lays out `text`, known to be JSON, onto `out`.
fn lay_out(text: &str, indent: &str, go_on: &dyn Fn() -> bool, out: &mut String) -> Option<()> {
    let bytes = text.as_bytes();
    let mut depth = 0;
    let mut at = 0;
    let mut check = CHECK_EVERY;
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        (0..depth).for_each(|_| out.push_str(indent));
    };
    while at < bytes.len() {
        if at >= check {
            if !go_on() {
                return None;
            }
            check += CHECK_EVERY;
        }
        match bytes[at] {
            b' ' | b'\t' | b'\n' | b'\r' => at += 1,
            b'"' => {
                let mut end = at + 1;
                while bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                out.push_str(&text[at..=end]);
                at = end + 1;
            }
            open @ (b'{' | b'[') => {
                let close = if open == b'{' { b'}' } else { b']' };
                let next = at + 1 + bytes[at + 1..].iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(0);
                if bytes.get(next) == Some(&close) {
                    // Empty, it stays on the line: `{}` or `[]`.
                    out.push(char::from(open));
                    out.push(char::from(close));
                    at = next + 1;
                } else {
                    out.push(char::from(open));
                    depth += 1;
                    newline(out, depth);
                    at += 1;
                }
            }
            close @ (b'}' | b']') => {
                depth -= 1;
                newline(out, depth);
                out.push(char::from(close));
                at += 1;
            }
            b',' => {
                out.push(',');
                newline(out, depth);
                at += 1;
            }
            b':' => {
                out.push_str(": ");
                at += 1;
            }
            _ => {
                // A number, `true`, `false` or `null`, kept as written.
                let end = bytes[at..]
                    .iter()
                    .position(|b| matches!(b, b',' | b']' | b'}' | b':') || b.is_ascii_whitespace())
                    .map_or(bytes.len(), |n| at + n);
                out.push_str(&text[at..end]);
                at = end;
            }
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pretty(text: &str) -> Result<String, String> {
        json(text, "  ", &|| true).unwrap()
    }

    #[test]
    fn json_is_laid_out_keeping_keys_numbers_and_strings_as_written() {
        let text = r#"{"b":1.50,"a":[true,null,{}, [ ]],"s":"x, \"y\": [z]","n":-2e10}"#;
        assert_eq!(
            pretty(text).unwrap(),
            "{\n  \"b\": 1.50,\n  \"a\": [\n    true,\n    null,\n    {},\n    []\n  ],\n  \"s\": \"x, \\\"y\\\": [z]\",\n  \"n\": -2e10\n}"
        );
        assert_eq!(pretty(" 42 ").unwrap(), "42");
        let again = pretty(&pretty(text).unwrap()).unwrap();
        assert_eq!(again, pretty(text).unwrap(), "laying out twice changes nothing");
    }

    #[test]
    fn what_is_not_json_says_where_and_json_lines_keep_lines_that_are_not() {
        assert_eq!(pretty("[1,]"), Err("expected value at line 1 column 4".into()));
        let lines = json_lines("{\"a\":1}\nnot json\n[2]\n", "  ", &|| true).unwrap().unwrap();
        assert_eq!(lines, "{\n  \"a\": 1\n}\nnot json\n[\n  2\n]\n");
        assert!(json_lines("one\ntwo\n", "  ", &|| true).unwrap().is_err());
        assert_eq!(json(&format!("[{}1]", "1,".repeat(CHECK_EVERY)), "  ", &|| false), None, "stopped");
    }
}
