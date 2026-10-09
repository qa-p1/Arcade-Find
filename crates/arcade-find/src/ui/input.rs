//! A single-line text field: cursor, selection, word movement.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    pub text: String,
    /// Byte offset of the cursor.
    pub cursor: usize,
    /// The other end of the selection, if any.
    pub anchor: Option<usize>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl TextInput {
    pub fn with(text: &str) -> TextInput {
        TextInput { text: text.into(), cursor: text.len(), anchor: None }
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.cursor).then(|| (a.min(self.cursor), a.max(self.cursor)))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|(a, b)| &self.text[a..b])
    }

    pub fn set(&mut self, text: &str) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.anchor = None;
    }

    pub fn clear(&mut self) {
        self.set("");
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    /// Selects `[a, b)` with the cursor at `b`.
    pub fn select_range(&mut self, a: usize, b: usize) {
        self.anchor = Some(a.min(self.text.len()));
        self.cursor = b.min(self.text.len());
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((a, b)) = self.selection() {
            self.text.replace_range(a..b, "");
            self.cursor = a;
            self.anchor = None;
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub fn insert(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        if s.is_empty() {
            return;
        }
        self.delete_selection();
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
    }

    fn prev_boundary(&self, i: usize) -> usize {
        self.text[..i].char_indices().next_back().map_or(0, |(j, _)| j)
    }

    fn next_boundary(&self, i: usize) -> usize {
        self.text[i..].chars().next().map_or(i, |c| i + c.len_utf8())
    }

    fn prev_word(&self, i: usize) -> usize {
        let mut j = i;
        // Skip separators, then the word.
        while j > 0 {
            let p = self.prev_boundary(j);
            if is_word(self.text[p..j].chars().next().unwrap_or(' ')) {
                break;
            }
            j = p;
        }
        while j > 0 {
            let p = self.prev_boundary(j);
            if !is_word(self.text[p..j].chars().next().unwrap_or(' ')) {
                break;
            }
            j = p;
        }
        j
    }

    fn next_word(&self, i: usize) -> usize {
        let mut j = i;
        let n = self.text.len();
        while j < n && !is_word(self.text[j..].chars().next().unwrap_or(' ')) {
            j = self.next_boundary(j);
        }
        while j < n && is_word(self.text[j..].chars().next().unwrap_or(' ')) {
            j = self.next_boundary(j);
        }
        j
    }

    pub fn backspace(&mut self, word: bool) {
        if self.delete_selection() || self.cursor == 0 {
            return;
        }
        let start = if word { self.prev_word(self.cursor) } else { self.prev_boundary(self.cursor) };
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    pub fn delete(&mut self, word: bool) {
        if self.delete_selection() || self.cursor >= self.text.len() {
            return;
        }
        let end = if word { self.next_word(self.cursor) } else { self.next_boundary(self.cursor) };
        self.text.replace_range(self.cursor..end, "");
    }

    fn move_to(&mut self, i: usize, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = i;
    }

    pub fn left(&mut self, word: bool, extend: bool) {
        if !extend {
            if let Some((a, _)) = self.selection() {
                self.move_to(a, false);
                return;
            }
        }
        let i = if word { self.prev_word(self.cursor) } else { self.prev_boundary(self.cursor) };
        self.move_to(i, extend);
    }

    pub fn right(&mut self, word: bool, extend: bool) {
        if !extend {
            if let Some((_, b)) = self.selection() {
                self.move_to(b, false);
                return;
            }
        }
        let i = if word { self.next_word(self.cursor) } else { self.next_boundary(self.cursor) };
        self.move_to(i, extend);
    }

    pub fn home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        let n = self.text.len();
        self.move_to(n, extend);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing() {
        let mut t = TextInput::default();
        t.insert("hello wörld");
        assert_eq!(t.cursor, t.text.len());
        t.backspace(false);
        assert_eq!(t.text, "hello wörl");
        t.backspace(true);
        assert_eq!(t.text, "hello ");
        t.left(true, false);
        assert_eq!(t.cursor, 0);
        t.right(false, true);
        assert_eq!(t.selected_text(), Some("h"));
        t.insert("J");
        assert_eq!(t.text, "Jello ");
        t.select_all();
        t.insert("x\ny");
        assert_eq!(t.text, "xy", "control characters are dropped");
        t.home(false);
        t.delete(false);
        assert_eq!(t.text, "y");
    }

    #[test]
    fn word_moves_over_punctuation() {
        let mut t = TextInput::with("ext:pdf annual-report");
        t.left(true, false);
        assert_eq!(&t.text[t.cursor..], "report");
        t.left(true, false);
        assert_eq!(&t.text[t.cursor..], "annual-report");
        t.right(true, false);
        assert_eq!(&t.text[..t.cursor], "ext:pdf annual");
        t.end(false);
        t.backspace(true);
        assert_eq!(t.text, "ext:pdf annual-");
    }
}
