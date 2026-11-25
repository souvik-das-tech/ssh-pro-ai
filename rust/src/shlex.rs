//! POSIX shell word splitting and quoting with the exact behaviour of Python's `shlex.split`,
//! `shlex.quote` and `shlex.join`, which the safety rules were written against.

#[derive(Debug, PartialEq, Eq)]
pub struct SplitError(pub &'static str);

impl std::fmt::Display for SplitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

const WHITESPACE: &[char] = &[' ', '\t', '\r', '\n'];

/// `shlex.split(text, comments=comments)` (posix mode, whitespace_split).
pub fn split(text: &str, comments: bool) -> Result<Vec<String>, SplitError> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Space,
        Word,
        Quote(char),
        Escape(char), // char = the state to return to: 'a' (word) or the quote
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut state = State::Space;
    let mut i = 0;
    while i <= chars.len() {
        let next = chars.get(i).copied();
        i += 1;
        match state {
            State::Space | State::Word => {
                let Some(ch) = next else {
                    break;
                };
                if WHITESPACE.contains(&ch) {
                    if state == State::Word || !token.is_empty() || quoted {
                        if !token.is_empty() || quoted {
                            out.push(std::mem::take(&mut token));
                            quoted = false;
                        }
                    }
                    state = State::Space;
                } else if comments && ch == '#' {
                    // the rest of the line is a comment
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    if i < chars.len() {
                        i += 1;
                    }
                    if !token.is_empty() || quoted {
                        out.push(std::mem::take(&mut token));
                        quoted = false;
                    }
                    state = State::Space;
                } else if ch == '\\' {
                    state = State::Escape('a');
                } else if ch == '\'' || ch == '"' {
                    quoted = true;
                    state = State::Quote(ch);
                } else {
                    token.push(ch);
                    state = State::Word;
                }
            }
            State::Quote(q) => {
                let Some(ch) = next else {
                    return Err(SplitError("No closing quotation"));
                };
                if ch == q {
                    state = State::Word;
                } else if ch == '\\' && q == '"' {
                    state = State::Escape(q);
                } else {
                    token.push(ch);
                }
            }
            State::Escape(back) => {
                let Some(ch) = next else {
                    return Err(SplitError("No escaped character"));
                };
                // inside double quotes only \" and \\ are escapes; others keep the backslash
                if back == '"' && ch != '\\' && ch != '"' {
                    token.push('\\');
                }
                token.push(ch);
                state = if back == 'a' { State::Word } else { State::Quote(back) };
            }
        }
    }
    if !token.is_empty() || quoted {
        out.push(token);
    }
    Ok(out)
}

/// `shlex.quote`: safe words stay as they are, anything else goes in single quotes.
pub fn quote(word: &str) -> String {
    if word.is_empty() {
        return "''".to_string();
    }
    let safe = word
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if safe {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', "'\"'\"'"))
}

pub fn join<S: AsRef<str>>(words: &[S]) -> String {
    words.iter().map(|w| quote(w.as_ref())).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Vec<String> {
        split(text, false).unwrap()
    }

    #[test]
    fn splits_like_python() {
        assert_eq!(s("a b  c"), ["a", "b", "c"]);
        assert_eq!(s("a 'b c' \"d e\""), ["a", "b c", "d e"]);
        assert_eq!(s("''"), [""]);
        assert_eq!(s(r#"a\ b"#), ["a b"]);
        assert_eq!(s(r#""a\"b" "c\d""#), ["a\"b", "c\\d"]);
        assert_eq!(s("x'y'z"), ["xyz"]);
        assert_eq!(split("a #c\nb", true).unwrap(), ["a", "b"]);
        assert_eq!(split("a#b", true).unwrap(), ["a"]);
        assert!(split("'a", false).is_err());
        assert!(split("a\\", false).is_err());
    }

    #[test]
    fn quotes_like_python() {
        assert_eq!(quote("abc/def-1.2"), "abc/def-1.2");
        assert_eq!(quote(""), "''");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\"'\"'s'");
    }
}
