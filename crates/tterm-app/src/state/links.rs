//! Links in the terminal's text: the URL under a cell, for Ctrl+click (Cmd+click on macOS) and
//! the Open link item of the menu. URLs are found in the text as kitty and Ghostty find plain
//! ones: a scheme (`https://`, `http://`, `file://`, `ftp://`) and every character up to a space
//! or a quote, without trailing punctuation. A URL that reaches the last column goes on in the
//! next row, as a long URL the terminal wrapped.

/// A link on the screen: its URL and the cells it covers, both ends included, as
/// `(column, row)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub url: String,
    pub start: (u16, usize),
    pub end: (u16, usize),
}

impl Link {
    /// The columns the link covers on `row`, both included, if it reaches that row.
    pub fn cols_on(&self, row: usize, cols: u16) -> Option<(u16, u16)> {
        if row < self.start.1 || row > self.end.1 {
            return None;
        }
        let from = if row == self.start.1 { self.start.0 } else { 0 };
        let to = if row == self.end.1 {
            self.end.0
        } else {
            cols.saturating_sub(1)
        };
        Some((from, to))
    }
}

const SCHEMES: [&str; 4] = ["https://", "http://", "file://", "ftp://"];

/// The link under the cell `(col, row)` of a screen whose rows hold one character per cell
/// (`'\0'` for the second cell of a wide character), each `cols` wide.
pub fn link_at(rows: &[Vec<char>], cols: u16, col: u16, row: usize) -> Option<Link> {
    let width = usize::from(cols);
    if width == 0 || row >= rows.len() || usize::from(col) >= width {
        return None;
    }
    let full = |r: usize| rows[r].len() >= width && !rows[r][width - 1].is_whitespace();
    // The logical line around `row`: rows that reach the last column go on in the next one.
    let mut first = row;
    while first > 0 && full(first - 1) {
        first -= 1;
    }
    let mut last = row;
    while last + 1 < rows.len() && full(last) {
        last += 1;
    }
    let mut text: Vec<char> = Vec::with_capacity((last - first + 1) * width);
    for cells in &rows[first..=last] {
        let mut line: Vec<char> = cells.iter().copied().take(width).collect();
        line.resize(width, ' ');
        text.extend(line);
    }
    let at = (row - first) * width + usize::from(col);
    let (start, end) = url_spans(&text)
        .into_iter()
        .find(|&(s, e)| s <= at && at <= e)?;
    let url: String = text[start..=end].iter().filter(|c| **c != '\0').collect();
    let cell = |index: usize| ((index % width) as u16, first + index / width);
    Some(Link {
        url,
        start: cell(start),
        end: cell(end),
    })
}

/// The URLs in `text`, as index ranges, both ends included.
fn url_spans(text: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut index = 0;
    while index < text.len() {
        let Some(scheme) = SCHEMES
            .iter()
            .find(|scheme| starts_with(text, index, scheme))
        else {
            index += 1;
            continue;
        };
        // The scheme must start a word: `xhttps://` is not a link.
        if index > 0 && (text[index - 1].is_alphanumeric()) {
            index += 1;
            continue;
        }
        let mut end = index + scheme.len();
        while end < text.len() && is_url_char(text[end]) {
            end += 1;
        }
        let end = trim_trailing(text, index + scheme.len(), end);
        if end > index + scheme.len() {
            spans.push((index, end - 1));
        }
        index = end.max(index + 1);
    }
    spans
}

fn starts_with(text: &[char], at: usize, prefix: &str) -> bool {
    let mut chars = prefix.chars();
    let mut index = at;
    loop {
        match chars.next() {
            None => return true,
            Some(c) => {
                if text.get(index) != Some(&c) {
                    return false;
                }
                index += 1;
            }
        }
    }
}

fn is_url_char(c: char) -> bool {
    !(c.is_whitespace() || c.is_control() && c != '\0' || matches!(c, '"' | '\'' | '<' | '>' | '`'))
}

/// Drops the punctuation that ends a sentence rather than the URL, and closing brackets
/// without their opening one inside the URL (`(see https://a.b/c)`).
fn trim_trailing(text: &[char], body: usize, mut end: usize) -> usize {
    while end > body {
        let last = text[end - 1];
        let unbalanced = |open: char, close: char| {
            let inside = &text[body..end];
            inside.iter().filter(|c| **c == close).count()
                > inside.iter().filter(|c| **c == open).count()
        };
        let drop = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\0' => true,
            ')' => unbalanced('(', ')'),
            ']' => unbalanced('[', ']'),
            '}' => unbalanced('{', '}'),
            _ => false,
        };
        if !drop {
            break;
        }
        end -= 1;
    }
    end
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn screen(lines: &[&str], cols: u16) -> Vec<Vec<char>> {
        lines
            .iter()
            .map(|line| {
                let mut cells: Vec<char> = line.chars().collect();
                cells.resize(usize::from(cols), ' ');
                cells
            })
            .collect()
    }

    #[test]
    fn a_url_is_found_under_any_of_its_cells() {
        let rows = screen(&["see https://tanis.codes/docs now"], 40);
        let link = link_at(&rows, 40, 10, 0).unwrap();
        assert_eq!(link.url, "https://tanis.codes/docs");
        assert_eq!(link.start, (4, 0));
        assert_eq!(link.end, (27, 0));
        assert!(link_at(&rows, 40, 2, 0).is_none());
        assert!(link_at(&rows, 40, 29, 0).is_none());
    }

    #[test]
    fn trailing_punctuation_and_unbalanced_brackets_are_left_out() {
        let rows = screen(&["(at https://a.io/x)."], 30);
        assert_eq!(link_at(&rows, 30, 6, 0).unwrap().url, "https://a.io/x");
        let rows = screen(&["https://en.wikipedia.org/wiki/Rust_(language), ok"], 60);
        assert_eq!(
            link_at(&rows, 60, 3, 0).unwrap().url,
            "https://en.wikipedia.org/wiki/Rust_(language)"
        );
        let rows = screen(&["\"http://x.y/z\""], 20);
        assert_eq!(link_at(&rows, 20, 3, 0).unwrap().url, "http://x.y/z");
    }

    #[test]
    fn a_url_that_reaches_the_last_column_goes_on_in_the_next_row() {
        // 10 columns: the URL fills the first row and ends on the second.
        let rows = screen(&["https://ab", "c.io/d end"], 10);
        let link = link_at(&rows, 10, 2, 1).unwrap();
        assert_eq!(link.url, "https://abc.io/d");
        assert_eq!(link.start, (0, 0));
        assert_eq!(link.end, (5, 1));
        assert_eq!(link.cols_on(0, 10), Some((0, 9)));
        assert_eq!(link.cols_on(1, 10), Some((0, 5)));
        assert_eq!(link.cols_on(2, 10), None);
        assert_eq!(link_at(&rows, 10, 0, 0).unwrap().url, "https://abc.io/d");
    }

    #[test]
    fn other_schemes_and_words_that_only_contain_one() {
        let rows = screen(&["file:///home/a.txt ftp://h/f xhttps://no"], 50);
        assert_eq!(link_at(&rows, 50, 3, 0).unwrap().url, "file:///home/a.txt");
        assert_eq!(link_at(&rows, 50, 22, 0).unwrap().url, "ftp://h/f");
        assert!(link_at(&rows, 50, 35, 0).is_none());
        let rows = screen(&["https:// alone"], 20);
        assert!(link_at(&rows, 20, 2, 0).is_none());
    }
}
