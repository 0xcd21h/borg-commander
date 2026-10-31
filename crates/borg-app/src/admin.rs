//! Commands that run as administrator through the system's own password prompt.

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// The AppleScript that asks for the user's password and runs `words` as one shell command line.
#[cfg(target_os = "macos")]
pub fn apple_script<'a>(words: impl IntoIterator<Item = &'a str>) -> anyhow::Result<String> {
    let line = words.into_iter().map(shell_quote).collect::<Vec<_>>().join(" ");
    Ok(format!("do shell script {} with administrator privileges", serde_json::to_string(&line)?))
}

/// Like [`apple_script`], for several command lines run one after another until one fails.
#[cfg(target_os = "macos")]
pub fn apple_script_all(lines: &[Vec<String>]) -> anyhow::Result<String> {
    let line = lines
        .iter()
        .map(|words| words.iter().map(|word| shell_quote(word)).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(" && ");
    Ok(format!("do shell script {} with administrator privileges", serde_json::to_string(&line)?))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn words_are_quoted_for_the_shell_and_the_line_for_applescript() {
        let script = apple_script(["/usr/bin/tmutil", "it's \"here\""]).unwrap();
        assert_eq!(
            script,
            r#"do shell script "'/usr/bin/tmutil' 'it'\"'\"'s \"here\"'" with administrator privileges"#
        );
        let lines = vec![vec!["a".to_owned()], vec!["b".to_owned(), "c d".to_owned()]];
        assert!(apple_script_all(&lines).unwrap().contains(r"'a' && 'b' 'c d'"));
    }
}
