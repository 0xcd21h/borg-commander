use super::{BEL, ESC, clipboard_text};

const CSI_OPENER: u8 = b'[';
const OSC_OPENER: u8 = b']';
const DCS_OPENER: u8 = b'P';
const APC_OPENER: u8 = b'_';
const STRING_OPENERS: &[u8] = b"]P_^X";
const STRING_END: u8 = b'\\';
const CSI_PRIVATE: u8 = b'?';
const CSI_FINALS: &[u8] = b"@ABCDEFGHIJKLMPSTXZ`abdefghlmrsu";
const CSI_PRIVATE_FINALS: &[u8] = b"hlJK";
const CURSOR_STYLE_INTERMEDIATES: &[u8] = b" ";
const CURSOR_STYLE_FINAL: u8 = b'q';
const ESCAPE_FINALS: &[u8] = b"78=>DEMH";
const CHARSET_SLOTS: &[u8] = b"()*+";
const TEXT_CONTROLS: &[char] = &['\u{7}', '\u{8}', '\t', '\n', '\r'];
const CLIPBOARD_COPY: &[u8] = b"52;c;";
const INLINE_IMAGE: &[u8] = b"1337;File=";
const INLINE_IMAGE_SHOWN: &str = "inline=1";
const INLINE_IMAGE_KEYS: &[&str] = &["size", "width", "height", "preserveAspectRatio", "doNotMoveCursor"];
const SIXEL_START: u8 = b'q';
const KITTY_GRAPHICS: u8 = b'G';
const KITTY_ACTION: &str = "a";
const KITTY_QUERY: &str = "q";
const KITTY_MEDIUM: &str = "t";
const KITTY_DIRECT: &str = "d";

enum Sequence<'a> {
    Escape { intermediates: &'a [u8], last: u8 },
    Csi { parameters: &'a [u8], intermediates: &'a [u8], last: u8 },
    Text { opener: u8, body: &'a [u8], bell: bool },
}

fn parameter(byte: u8) -> bool {
    (0x30..=0x3f).contains(&byte)
}

fn intermediate(byte: u8) -> bool {
    (0x20..=0x2f).contains(&byte)
}

fn printable(byte: u8) -> bool {
    (0x20..=0x7e).contains(&byte)
}

fn run(bytes: &[u8], from: usize, belongs: fn(u8) -> bool) -> usize {
    from + bytes[from..].iter().take_while(|byte| belongs(**byte)).count()
}

fn csi(bytes: &[u8]) -> (Option<Sequence<'_>>, usize) {
    let parameters_end = run(bytes, 2, parameter);
    let intermediates_end = run(bytes, parameters_end, intermediate);
    match bytes.get(intermediates_end) {
        Some(&last) if (0x40..=0x7e).contains(&last) => (
            Some(Sequence::Csi {
                parameters: &bytes[2..parameters_end],
                intermediates: &bytes[parameters_end..intermediates_end],
                last,
            }),
            intermediates_end + 1,
        ),
        _ => (None, intermediates_end),
    }
}

fn text(bytes: &[u8], opener: u8) -> (Option<Sequence<'_>>, usize) {
    let Some(end) = bytes[2..].iter().position(|byte| matches!(*byte, BEL | ESC)).map(|offset| offset + 2) else {
        return (None, bytes.len());
    };
    let body = &bytes[2..end];
    match (bytes[end], bytes.get(end + 1)) {
        (BEL, _) => (Some(Sequence::Text { opener, body, bell: true }), end + 1),
        (_, Some(&STRING_END)) => (Some(Sequence::Text { opener, body, bell: false }), end + 2),
        _ => (None, end),
    }
}

fn escape(bytes: &[u8]) -> (Option<Sequence<'_>>, usize) {
    let intermediates_end = run(bytes, 1, intermediate);
    match bytes.get(intermediates_end) {
        Some(&last) if (0x30..=0x7e).contains(&last) => {
            (Some(Sequence::Escape { intermediates: &bytes[1..intermediates_end], last }), intermediates_end + 1)
        }
        _ => (None, intermediates_end),
    }
}

fn sequence(bytes: &[u8]) -> (Option<Sequence<'_>>, usize) {
    match bytes.get(1) {
        Some(&CSI_OPENER) => csi(bytes),
        Some(&opener) if STRING_OPENERS.contains(&opener) => text(bytes, opener),
        _ => escape(bytes),
    }
}

fn base64(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

fn csi_allowed(parameters: &[u8], intermediates: &[u8], last: u8) -> bool {
    let (private, values) = match parameters.split_first() {
        Some((&CSI_PRIVATE, values)) => (true, values),
        _ => (false, parameters),
    };
    let plain_values = values.iter().all(|byte| byte.is_ascii_digit() || matches!(byte, b';' | b':'));
    plain_values
        && match (private, intermediates) {
            (false, []) => CSI_FINALS.contains(&last),
            (false, CURSOR_STYLE_INTERMEDIATES) => last == CURSOR_STYLE_FINAL,
            (true, []) => CSI_PRIVATE_FINALS.contains(&last),
            _ => false,
        }
}

fn clipboard_copy(body: &[u8]) -> bool {
    body.strip_prefix(CLIPBOARD_COPY).and_then(|copy| std::str::from_utf8(copy).ok()).is_some_and(clipboard_text)
}

fn inline_image(body: &[u8]) -> bool {
    let Some((options, data)) = body
        .strip_prefix(INLINE_IMAGE)
        .and_then(|image| std::str::from_utf8(image).ok())
        .and_then(|image| image.split_once(':'))
    else {
        return false;
    };
    let option_allowed = |option: &str| {
        option == INLINE_IMAGE_SHOWN
            || option.split_once('=').is_some_and(|(key, value)| {
                INLINE_IMAGE_KEYS.contains(&key) && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
    };
    let options: Vec<&str> = options.split(';').collect();
    options.contains(&INLINE_IMAGE_SHOWN) && options.into_iter().all(option_allowed) && base64(data.as_bytes())
}

fn sixel(body: &[u8]) -> bool {
    let header_end = body.iter().take_while(|byte| byte.is_ascii_digit() || **byte == b';').count();
    body.get(header_end) == Some(&SIXEL_START) && body[header_end..].iter().copied().all(printable)
}

fn kitty_graphics(body: &[u8]) -> bool {
    let Some(command) = body.strip_prefix(&[KITTY_GRAPHICS]).and_then(|command| std::str::from_utf8(command).ok())
    else {
        return false;
    };
    let (control, data) = command.split_once(';').unwrap_or((command, ""));
    let key_allowed = |pair: &str| {
        pair.split_once('=').is_some_and(|(key, value)| {
            key.bytes().all(|byte| byte.is_ascii_alphabetic())
                && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && (key, value) != (KITTY_ACTION, KITTY_QUERY)
                && (key != KITTY_MEDIUM || value == KITTY_DIRECT)
        })
    };
    control.split(',').all(key_allowed) && base64(data.as_bytes())
}

fn allowed(sequence: &Sequence<'_>) -> bool {
    match *sequence {
        Sequence::Escape { intermediates: [], last } => ESCAPE_FINALS.contains(&last),
        Sequence::Escape { intermediates: [slot], .. } => CHARSET_SLOTS.contains(slot),
        Sequence::Escape { .. } => false,
        Sequence::Csi { parameters, intermediates, last } => csi_allowed(parameters, intermediates, last),
        Sequence::Text { opener: OSC_OPENER, body, .. } => clipboard_copy(body) || inline_image(body),
        Sequence::Text { opener: DCS_OPENER, body, bell: false } => sixel(body),
        Sequence::Text { opener: APC_OPENER, body, bell: false } => kitty_graphics(body),
        Sequence::Text { .. } => false,
    }
}

fn push_text(output: &mut Vec<u8>, bytes: &[u8]) {
    let mut encoded = [0; 4];
    for chunk in bytes.utf8_chunks() {
        for character in chunk.valid().chars().filter(|c| !c.is_control() || TEXT_CONTROLS.contains(c)) {
            output.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
        }
    }
}

pub(super) fn displayable(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while !rest.is_empty() {
        let text_end = rest.iter().position(|byte| *byte == ESC).unwrap_or(rest.len());
        push_text(&mut output, &rest[..text_end]);
        rest = &rest[text_end..];
        if rest.is_empty() {
            break;
        }
        let (parsed, length) = sequence(rest);
        if parsed.as_ref().is_some_and(allowed) {
            output.extend_from_slice(&rest[..length]);
        }
        rest = &rest[length..];
    }
    output
}

#[cfg(test)]
mod tests {
    use super::displayable;

    fn kept(bytes: &[u8]) -> bool {
        displayable(bytes) == bytes
    }

    fn shown(bytes: &[u8]) -> String {
        String::from_utf8(displayable(bytes)).expect("displayable output is UTF-8")
    }

    #[test]
    fn what_borg_draws_with_reaches_the_terminal_unchanged() {
        for drawn in [
            "\x1b[?2026h\x1b[?25l\x1b[0m\x1b[2J\x1b[H\x1b[?1049h",
            "\x1b[12;40H\x1b[38;2;30;30;30m\x1b[48;5;17m\x1b[1;4:3mBorg ┌─┐ 文件 🖖\x1b[0m\x1b[K",
            "\x1b7\x1b[3;5H\x1b8\x1b(B\x1b[2 q\x1b[?1000h\x1b[?1006h\x1b[?2004h",
            "line one\r\n\tline two\x08",
            "\x1b]52;c;bm90ZXMudHh0\x07",
            "\x1b_Ga=t,f=100,t=d,i=7,q=2,m=1;iVBORw0KGgo=\x1b\\\x1b_Gm=0;AAAA\x1b\\",
            "\x1b_Ga=p,i=7,p=3,c=3,r=1,X=1,Y=2,C=1,z=-1,q=2\x1b\\\x1b_Ga=d,d=A,q=2\x1b\\",
            "\x1b]1337;File=inline=1;size=68;width=8px;height=16px;preserveAspectRatio=1;doNotMoveCursor=1:iVBORw0KGgo=\x07",
            "\x1bP0;1;0q\"1;1;8;16#0;2;0;0;0#0~~@@vv@@~~$-\x1b\\",
        ] {
            assert!(kept(drawn.as_bytes()), "{drawn:?}");
        }
    }

    #[test]
    fn requests_the_terminal_would_answer_are_dropped() {
        for request in [
            "\x1b[6n",
            "\x1b[?6n",
            "\x1b[c",
            "\x1b[>c",
            "\x1b[=c",
            "\x1b[>q",
            "\x1b[?u",
            "\x1b[21t",
            "\x1b[14t",
            "\x1b[?2026$p",
            "\x1b[x",
            "\x1bZ",
            "\x05",
            "\x1b]52;c;?\x07",
            "\x1b]10;?\x07",
            "\x1b]11;?\x1b\\",
            "\x1b]4;4;?\x07",
            "\x1bP$qm\x1b\\",
            "\x1bP+q544e\x1b\\",
            "\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\",
        ] {
            assert_eq!(shown(format!("before{request}after").as_bytes()), "beforeafter", "{request:?}");
        }
    }

    #[test]
    fn sequences_that_change_the_terminal_or_read_its_files_are_dropped() {
        for change in [
            "\x1b]0;title\x07",
            "\x1b]2;title\x1b\\",
            "\x1b]7;file://host/tmp\x07",
            "\x1b]8;;https://example.com\x07",
            "\x1b]1337;File=inline=0;size=4:AAAA\x07",
            "\x1b]1337;File=name=bm90ZXM=;inline=1:AAAA\x07",
            "\x1b]1337;File=size=4:AAAA\x07",
            "\x1b]1337;SetProfile=other\x07",
            "\x1b_Ga=T,f=100,t=f;L2V0Yy9wYXNzd2Q=\x1b\\",
            "\x1b_Ga=T,f=100,t=t;L3RtcC9maWxl\x1b\\",
            "\x1bPtmux;\x1b\\",
            "\x1b^private\x1b\\",
            "\x1bXstring\x1b\\",
            "\x1b[5i",
            "\x1b[>1u",
            "\x1b[<u",
            "\x1b[8;40;120t",
            "\x1b[!p",
            "\x1bc",
        ] {
            assert_eq!(shown(format!("before{change}after").as_bytes()), "beforeafter", "{change:?}");
        }
    }

    #[test]
    fn control_characters_outside_plain_layout_are_dropped() {
        assert_eq!(shown("a\u{9b}6n\u{9d}52;c;?\u{9c}\u{90}b\x0e\x7f".as_bytes()), "a6n52;c;?b");
        assert_eq!(shown(b"a\x9b6n\xc2"), "a6n");
        assert_eq!(shown(b"ring\x07 tab\t"), "ring\x07 tab\t");
    }

    #[test]
    fn a_sequence_cut_short_or_broken_is_dropped_and_what_follows_is_read_again() {
        assert_eq!(shown(b"text\x1b]52;c;?"), "text");
        assert_eq!(shown(b"text\x1b[38;2"), "text");
        assert_eq!(shown(b"text\x1b"), "text");
        assert_eq!(shown(b"\x1b]0;title\x1b[1mbold"), "\x1b[1mbold");
        assert_eq!(shown(b"\x1b[3\x1b[1mbold"), "\x1b[1mbold");
        assert_eq!(shown(b"\x1b[3\x07m"), "\x07m");
        assert_eq!(shown(b"\x1bPq#0~~\x07\x1b[6n"), "");
        assert_eq!(shown(b"\x1b]52;c;bm90\x1b]52;c;?\x07"), "");
    }
}
