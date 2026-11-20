//! beatoraja reads skins with libGDX's lenient JSON reader: what json5
//! needs added to read the same files.

/// Arrays and objects in one another, at most (serde_json's own limit;
/// skins stay far below it).
const MAX_DEPTH: usize = 128;

/// JSON5 text as a value. json5 calls itself once per nested array or
/// object and has no limit of its own, so text nested deeper than
/// [`MAX_DEPTH`] is refused before it is given to it (any `.json` below
/// the skins folder is read to list the skins). Strings and comments,
/// where a bracket opens nothing, are told as json5 tells them.
pub(super) fn parse(text: &str) -> Result<serde_json::Value, String> {
    let (mut depth, mut quote, mut chars) = (0usize, None, text.chars().peekable());
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') => {
                chars.next();
            }
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '/') if chars.next_if_eq(&'/').is_some() => {
                chars.find(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'));
            }
            (None, '/') if chars.next_if_eq(&'*').is_some() => {
                let mut star = false;
                chars.find(|c| std::mem::replace(&mut star, *c == '*') && *c == '/');
            }
            (None, '[' | '{') => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(format!("nested deeper than {MAX_DEPTH}"));
                }
            }
            (None, ']' | '}') => depth = depth.saturating_sub(1),
            (None, _) => {}
        }
    }
    json5::from_str(text).map_err(|e| e.to_string())
}

/// beatoraja's own play5.json has `"value": {{...}},`, one brace too many
/// that libGDX's lenient reader gets past: read `{{` as `{`.
pub(super) fn merge_double_braces(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut quote = None;
    while i < b.len() {
        let c = b[i];
        out.push(c);
        i += 1;
        match quote {
            Some(_) if c == '\\' => {
                if let Some(n) = b.get(i) {
                    out.push(*n);
                    i += 1;
                }
            }
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '{' => {
                let mut j = i;
                while j < b.len() && b[j].is_whitespace() {
                    j += 1;
                }
                if b.get(j) == Some(&'{') {
                    i = j + 1;
                }
            }
            None => {}
        }
    }
    out
}

/// beatoraja reads skins with libGDX's lenient JSON reader, which accepts
/// values separated only by a line break and numbers with leading zeros;
/// insert the missing commas and drop the zeros so json5 accepts them too
/// (strings and comments are left alone).
pub(super) fn add_missing_commas(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 64);
    // last significant char outside strings / comments
    let mut last: Option<char> = None;
    let mut i = 0;
    let ends_value = |c: char| matches!(c, '}' | ']' | '"' | '\'') || c.is_alphanumeric();
    let starts_value =
        |c: char| matches!(c, '{' | '[' | '"' | '\'' | '-' | '+' | '.') || c.is_alphanumeric();
    let mut newline_since_last = false;
    while i < b.len() {
        let c = b[i];
        if c == '"' || c == '\'' {
            if newline_since_last && last.map(ends_value).unwrap_or(false) {
                out.push(',');
            }
            newline_since_last = false;
            out.push(c);
            i += 1;
            while i < b.len() && b[i] != c {
                if b[i] == '\\' && i + 1 < b.len() {
                    out.push(b[i]);
                    i += 1;
                }
                out.push(b[i]);
                i += 1;
            }
            if i < b.len() {
                out.push(b[i]);
                i += 1;
            }
            last = Some(c);
            continue;
        }
        if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                out.push(b[i]);
                i += 1;
            }
            continue;
        }
        if c == '/' && b.get(i + 1) == Some(&'*') {
            while i < b.len() && !(b[i] == '*' && b.get(i + 1) == Some(&'/')) {
                out.push(b[i]);
                i += 1;
            }
            out.push_str("*/");
            i += 2;
            continue;
        }
        // numbers with leading zeros (`"y":00`)
        if c == '0'
            && b.get(i + 1).is_some_and(char::is_ascii_digit)
            && !out
                .chars()
                .next_back()
                .is_some_and(|p| p.is_alphanumeric() || p == '.' || p == '_')
        {
            i += 1;
            continue;
        }
        if c == '\n' {
            newline_since_last = true;
        } else if !c.is_whitespace() {
            if newline_since_last && starts_value(c) && last.map(ends_value).unwrap_or(false) {
                out.push(',');
            }
            newline_since_last = false;
            last = Some(c);
        }
        out.push(c);
        i += 1;
    }
    out
}
