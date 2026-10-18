use unicode_width::UnicodeWidthChar;

const TAB_WIDTH: usize = 8;
const CONTROL_GLYPH: char = '·';
const CARRIAGE_RETURN: char = '\r';

pub fn display(bytes: &[u8]) -> Vec<(char, usize)> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_suffix(CARRIAGE_RETURN).unwrap_or(&text);
    let mut column = 0;
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        if c == '\t' {
            let spaces = TAB_WIDTH - column % TAB_WIDTH;
            out.extend(std::iter::repeat_n((' ', 1), spaces));
            column += spaces;
        } else if c.is_control() {
            out.push((CONTROL_GLYPH, 1));
            column += 1;
        } else {
            let width = c.width().unwrap_or(0);
            out.push((c, width));
            column += width;
        }
    }
    out
}

pub fn wrap(cells: &[(char, usize)], width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = vec![String::new()];
    let mut used = 0;
    for &(c, w) in cells {
        if used + w > width && used > 0 {
            rows.push(String::new());
            used = 0;
        }
        rows.last_mut().into_iter().for_each(|row| row.push(c));
        used += w;
    }
    rows
}

pub fn slice(cells: &[(char, usize)], skip: usize, width: usize) -> String {
    let mut column = 0;
    cells
        .iter()
        .filter_map(|&(c, w)| {
            let start = column;
            column += w;
            (start >= skip && column <= skip + width).then_some(c)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(cells: &[(char, usize)]) -> String {
        cells.iter().map(|(c, _)| c).collect()
    }

    #[test]
    fn expands_tabs_and_masks_control_bytes() {
        assert_eq!(text(&display(b"a\tb\x01\r")), format!("a{}b·", " ".repeat(7)));
    }

    #[test]
    fn wraps_by_display_width() {
        let cells = display("日本語abc".as_bytes());
        assert_eq!(wrap(&cells, 4), ["日本", "語ab", "c"]);
        assert_eq!(wrap(&[], 4), [""]);
    }

    #[test]
    fn slices_horizontally() {
        let cells = display(b"0123456789");
        assert_eq!(slice(&cells, 3, 4), "3456");
        assert_eq!(slice(&cells, 20, 4), "");
    }
}
