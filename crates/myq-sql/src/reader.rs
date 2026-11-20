//! Statement-by-statement reading of a SQL script of any size.
//!
//! `split_statements` needs the whole script in memory. A database dump can be gigabytes, so this
//! reader pulls one line at a time and yields each statement as soon as its delimiter is seen. It
//! follows the same lexical rules (strings, backticks, comments, `DELIMITER`) and keeps the lexer
//! state between lines, so a string or comment may span many lines.

use crate::split::is_comment_only;
use std::collections::VecDeque;
use std::io::{self, BufRead};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptStatement {
    pub text: String,
    /// 1-based line where the statement starts.
    pub line: usize,
    /// Bytes of the script consumed once this statement ended (for progress).
    pub bytes_read: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Normal,
    Single,
    Double,
    Backtick,
    Block,
}

pub struct ScriptReader<R: BufRead> {
    reader: R,
    delim: String,
    buf: String,
    state: State,
    /// A backslash was the last character inside a quoted string: the next character is literal.
    escape: bool,
    line_no: usize,
    start_line: usize,
    bytes: u64,
    ready: VecDeque<ScriptStatement>,
    done: bool,
}

impl<R: BufRead> ScriptReader<R> {
    pub fn new(reader: R) -> Self {
        ScriptReader { reader, delim: ";".into(), buf: String::new(), state: State::Normal, escape: false, line_no: 0, start_line: 1, bytes: 0, ready: VecDeque::new(), done: false }
    }

    fn emit(&mut self) {
        let text = self.buf.trim().to_string();
        if !text.is_empty() && !is_comment_only(&text) {
            self.ready.push_back(ScriptStatement { text, line: self.start_line, bytes_read: self.bytes });
        }
        self.buf.clear();
    }

    fn push(&mut self, c: char) {
        if self.buf.trim().is_empty() && !c.is_whitespace() {
            self.start_line = self.line_no;
        }
        self.buf.push(c);
    }

    fn scan_line(&mut self, line: &str) {
        // `DELIMITER x` is a client command, valid only between statements.
        if self.state == State::Normal && self.buf.trim().is_empty() {
            let t = line.trim_start();
            if t.len() > 9 && t[..9].eq_ignore_ascii_case("delimiter") && t.as_bytes()[9].is_ascii_whitespace() {
                let new = t[9..].trim();
                if !new.is_empty() {
                    self.delim = new.to_string();
                }
                self.buf.clear();
                return;
            }
        }
        let chars: Vec<char> = line.chars().collect();
        let delim: Vec<char> = self.delim.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            match self.state {
                State::Normal => {
                    if chars[i..].starts_with(&delim) {
                        self.emit();
                        i += delim.len();
                        continue;
                    }
                    let dash_comment = c == '-' && chars.get(i + 1) == Some(&'-') && chars.get(i + 2).map_or(true, |n| n.is_whitespace());
                    if c == '#' || dash_comment {
                        // the rest of the line is a comment (kept in the text; it is harmless to the server)
                        for &ch in &chars[i..] {
                            self.push(ch);
                        }
                        return;
                    }
                    match c {
                        '\'' => self.state = State::Single,
                        '"' => self.state = State::Double,
                        '`' => self.state = State::Backtick,
                        '/' if chars.get(i + 1) == Some(&'*') => {
                            self.push('/');
                            self.push('*');
                            self.state = State::Block;
                            i += 2;
                            continue;
                        }
                        _ => {}
                    }
                    self.push(c);
                }
                State::Single | State::Double => {
                    let quote = if self.state == State::Single { '\'' } else { '"' };
                    self.push(c);
                    if self.escape {
                        self.escape = false;
                    } else if c == '\\' {
                        self.escape = true;
                    } else if c == quote {
                        if chars.get(i + 1) == Some(&quote) {
                            self.push(quote); // doubled quote stays inside the string
                            i += 1;
                        } else {
                            self.state = State::Normal;
                        }
                    }
                }
                State::Backtick => {
                    self.push(c);
                    if c == '`' {
                        if chars.get(i + 1) == Some(&'`') {
                            self.push('`');
                            i += 1;
                        } else {
                            self.state = State::Normal;
                        }
                    }
                }
                State::Block => {
                    self.push(c);
                    if c == '*' && chars.get(i + 1) == Some(&'/') {
                        self.push('/');
                        self.state = State::Normal;
                        i += 1;
                    }
                }
            }
            i += 1;
        }
    }
}

impl<R: BufRead> Iterator for ScriptReader<R> {
    type Item = io::Result<ScriptStatement>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(s) = self.ready.pop_front() {
                return Some(Ok(s));
            }
            if self.done {
                return None;
            }
            let mut raw = Vec::new();
            match self.reader.read_until(b'\n', &mut raw) {
                Err(e) => {
                    self.done = true;
                    return Some(Err(e));
                }
                Ok(0) => {
                    self.done = true;
                    self.emit(); // a final statement without a delimiter
                }
                Ok(n) => {
                    self.bytes += n as u64;
                    self.line_no += 1;
                    // Dumps are UTF-8; a stray invalid byte becomes U+FFFD instead of aborting a long restore.
                    let line = String::from_utf8_lossy(&raw);
                    self.scan_line(&line);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::split_statements;
    use std::io::Cursor;

    fn read(s: &str) -> Vec<ScriptStatement> {
        ScriptReader::new(Cursor::new(s.as_bytes())).map(|r| r.unwrap()).collect()
    }
    fn texts(s: &str) -> Vec<String> {
        read(s).into_iter().map(|x| x.text).collect()
    }

    #[test]
    fn agrees_with_the_in_memory_splitter_on_awkward_scripts() {
        let corpus = [
            "SELECT 1;\nSELECT 2;",
            "SELECT 'a;b'; -- c;d\nSELECT `x;y`; /* e;f */ SELECT \"q\\\";\"; SELECT 1 # z;\n",
            "SELECT 'multi\nline;\nstring'; SELECT 2",
            "INSERT INTO t VALUES ('it''s;', 'a\\'b;c');\nSELECT 3;",
            "/*!40101 SET NAMES utf8mb4 */;\n/* note */\nSELECT 1;\n-- only a comment\n",
            "SELECT /* inline; comment */ 1;\r\nSELECT 2;\r\n",
            "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END$$\nDELIMITER ;\nSELECT 3;",
            ";;  SELECT 1;\n\n;SELECT 2",
        ];
        for sql in corpus {
            let streamed: Vec<String> = texts(sql);
            let in_memory: Vec<String> = split_statements(sql).into_iter().map(|s| s.text).collect();
            assert_eq!(streamed, in_memory, "script: {sql:?}");
        }
    }

    #[test]
    fn reports_start_lines_and_progress() {
        let s = read("SELECT 1;\n\n  SELECT\n  2;\nSELECT 3");
        assert_eq!(s.iter().map(|x| x.line).collect::<Vec<_>>(), vec![1, 3, 5]);
        assert!(s[0].bytes_read < s[1].bytes_read && s[1].bytes_read < s[2].bytes_read);
        assert_eq!(s[2].bytes_read, "SELECT 1;\n\n  SELECT\n  2;\nSELECT 3".len() as u64);
    }

    #[test]
    fn a_string_or_comment_may_span_many_lines() {
        assert_eq!(texts("SELECT 'a\n;\nb'; SELECT 1"), vec!["SELECT 'a\n;\nb'", "SELECT 1"]);
        assert_eq!(texts("SELECT 1 /* a\n;\n b */;SELECT 2"), vec!["SELECT 1 /* a\n;\n b */", "SELECT 2"]);
        // a backslash before the line break escapes the break, not the quote after it
        assert_eq!(texts("SELECT 'x\\\n';SELECT 2"), vec!["SELECT 'x\\\n'", "SELECT 2"]);
    }

    #[test]
    fn dump_style_routine_blocks_and_executable_comments() {
        let dump = "/*!40101 SET @OLD=@@X */;\nDROP TABLE IF EXISTS `t`;\nDELIMITER ;;\n/*!50003 CREATE TRIGGER `tr` BEFORE INSERT ON `t` FOR EACH ROW BEGIN SET NEW.a = 1; END */;;\nDELIMITER ;\nSELECT 1;\n";
        let t = texts(dump);
        assert_eq!(t.len(), 4, "{t:?}");
        assert!(t[0].starts_with("/*!40101"));
        assert!(t[2].contains("SET NEW.a = 1; END"), "the trigger body stays in one statement: {}", t[2]);
    }

    #[test]
    fn invalid_utf8_does_not_abort_and_empty_input_yields_nothing() {
        let bytes = b"SELECT 'caf\xe9';SELECT 2";
        let t: Vec<String> = ScriptReader::new(Cursor::new(&bytes[..])).map(|r| r.unwrap().text).collect();
        assert_eq!(t.len(), 2);
        assert!(t[0].contains('\u{FFFD}'));
        assert!(read("").is_empty() && read("  \n-- x\n").is_empty());
    }
}
