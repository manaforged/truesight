use crate::path::skip_attributes;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scope {
    pub start: usize,
    pub end: usize,
}

pub struct Source<'t> {
    text: &'t str,
    quiet: Vec<(usize, usize)>,
    braces: Vec<(usize, u8)>,
}

pub fn ident_len(text: &str) -> usize {
    let (prefix, body) = match text.strip_prefix("r#") {
        Some(body) => (2, body),
        None => (0, text),
    };
    if body.starts_with(|current: char| current.is_ascii_digit()) {
        return 0;
    }
    let length = body
        .find(|current: char| !(current.is_alphanumeric() || current == '_'))
        .unwrap_or(body.len());
    if length == 0 { 0 } else { prefix + length }
}

pub fn is_ident(text: &str) -> bool {
    !text.is_empty() && ident_len(text) == text.len()
}

pub fn visibility(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("pub") else {
        return text;
    };
    if let Some(inner) = rest.strip_prefix('(') {
        return inner
            .find(')')
            .and_then(|close| inner.get(close + 1..))
            .map_or(text, str::trim_start);
    }
    if rest.starts_with([' ', '\t']) {
        rest.trim_start()
    } else {
        text
    }
}

pub fn starts_use(text: &str) -> bool {
    visibility(skip_attributes(text))
        .strip_prefix("use")
        .is_some_and(|rest| rest.starts_with([' ', '\t']))
}

fn ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn line_end(bytes: &[u8], at: usize) -> usize {
    bytes
        .iter()
        .skip(at)
        .position(|&byte| byte == b'\n')
        .map_or(bytes.len(), |offset| at + offset)
}

fn comment_end(bytes: &[u8], at: usize) -> usize {
    let mut depth = 0usize;
    let mut index = at;
    while let Some(pair) = bytes.get(index..index + 2) {
        match pair {
            b"/*" => {
                depth += 1;
                index += 2;
            }
            b"*/" => {
                depth = depth.saturating_sub(1);
                index += 2;
                if depth == 0 {
                    return index;
                }
            }
            _ => index += 1,
        }
    }
    bytes.len()
}

fn string_end(bytes: &[u8], from: usize) -> usize {
    let mut index = from;
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn raw_hashes(bytes: &[u8], at: usize) -> Option<usize> {
    let before = |back: usize| {
        at.checked_sub(back)
            .and_then(|index| bytes.get(index))
            .copied()
    };
    let free = match before(1) {
        None => true,
        Some(b'b') => before(2).is_none_or(|byte| !ident_byte(byte)),
        Some(byte) => !ident_byte(byte),
    };
    if !free {
        return None;
    }
    let hashes = bytes
        .iter()
        .skip(at + 1)
        .take_while(|&&byte| byte == b'#')
        .count();
    (bytes.get(at + 1 + hashes) == Some(&b'"')).then_some(hashes)
}

fn raw_end(bytes: &[u8], at: usize, hashes: usize) -> usize {
    let mut index = at + hashes + 2;
    while let Some(&byte) = bytes.get(index) {
        let closed = bytes
            .iter()
            .skip(index + 1)
            .take(hashes)
            .filter(|&&next| next == b'#')
            .count()
            == hashes;
        if byte == b'"' && closed {
            return index + 1 + hashes;
        }
        index += 1;
    }
    bytes.len()
}

fn char_end(bytes: &[u8], at: usize) -> Option<usize> {
    let next = *bytes.get(at + 1)?;
    if next == b'\\' {
        let offset = bytes.iter().skip(at + 3).position(|&byte| byte == b'\'')?;
        return Some(at + 4 + offset);
    }
    let width = match next {
        0..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    };
    (bytes.get(at + 1 + width) == Some(&b'\'')).then_some(at + 2 + width)
}

fn header_comment(text: &str) -> bool {
    let outer_line = text.starts_with("///") && !text.starts_with("////");
    let outer_block =
        text.starts_with("/**") && !text.starts_with("/***") && !text.starts_with("/**/");
    (text.starts_with("//") && !outer_line) || (text.starts_with("/*") && !outer_block)
}

impl<'t> Source<'t> {
    pub fn new(text: &'t str) -> Self {
        let bytes = text.as_bytes();
        let mut quiet = Vec::new();
        let mut braces = Vec::new();
        let mut at = 0;
        while let Some(&byte) = bytes.get(at) {
            let end = match byte {
                b'/' if bytes.get(at + 1) == Some(&b'/') => Some(line_end(bytes, at)),
                b'/' if bytes.get(at + 1) == Some(&b'*') => Some(comment_end(bytes, at)),
                b'"' => Some(string_end(bytes, at + 1)),
                b'r' => raw_hashes(bytes, at).map(|hashes| raw_end(bytes, at, hashes)),
                b'\'' => char_end(bytes, at),
                b'{' | b'}' => {
                    braces.push((at, byte));
                    None
                }
                _ => None,
            };
            match end {
                Some(end) => {
                    quiet.push((at, end));
                    at = end.max(at + 1);
                }
                None => at += 1,
            }
        }
        Self {
            text,
            quiet,
            braces,
        }
    }

    fn quiet_at(&self, offset: usize) -> Option<(usize, usize)> {
        let index = self.quiet.partition_point(|(start, _)| *start <= offset);
        index
            .checked_sub(1)
            .and_then(|index| self.quiet.get(index))
            .copied()
            .filter(|(_, end)| offset < *end)
    }

    fn line_start(&self, offset: usize) -> usize {
        self.text
            .get(..offset)
            .and_then(|head| head.rfind('\n'))
            .map_or(0, |index| index + 1)
    }

    fn line_offset(&self, line: usize) -> usize {
        if line <= 1 {
            return 0;
        }
        self.text
            .match_indices('\n')
            .nth(line - 2)
            .map_or(self.text.len(), |(index, _)| index + 1)
    }

    fn blocks(&self) -> Vec<(usize, usize)> {
        let mut open = Vec::new();
        let mut found = Vec::new();
        for &(at, brace) in &self.braces {
            if brace == b'{' {
                open.push(at);
            } else if let Some(start) = open.pop()
                && self.opens_mod(start)
            {
                found.push((start, at));
            }
        }
        found
    }

    fn opens_mod(&self, at: usize) -> bool {
        let head = self
            .text
            .get(self.line_start(at)..at)
            .unwrap_or_default()
            .trim();
        visibility(skip_attributes(head))
            .strip_prefix("mod ")
            .is_some_and(|name| is_ident(name.trim()))
    }

    pub fn scope(&self, line: usize) -> Scope {
        let offset = self.line_offset(line);
        self.blocks()
            .into_iter()
            .filter(|&(open, close)| open < offset && offset <= close)
            .max_by_key(|&(open, _)| open)
            .map_or(
                Scope {
                    start: 0,
                    end: self.text.len(),
                },
                |(open, close)| Scope {
                    start: open + 1,
                    end: close,
                },
            )
    }

    pub fn insertion(&self, scope: Scope) -> (usize, String) {
        let body = self.header_end(scope);
        let at = self.first_use(scope, body).unwrap_or(body);
        (at, self.indent(scope, at))
    }

    fn header_end(&self, scope: Scope) -> usize {
        let mut at = scope.start;
        loop {
            let rest = self.text.get(at..scope.end).unwrap_or_default();
            let next = rest.trim_start();
            let here = at + rest.len() - next.len();
            if next.starts_with("#![") {
                at = self.bracket_end(here + 2).min(scope.end).max(here + 1);
            } else if header_comment(next)
                && let Some((_, end)) = self.quiet_at(here)
            {
                at = end;
            } else {
                return self.line_start(here).max(scope.start);
            }
        }
    }

    fn bracket_end(&self, open: usize) -> usize {
        let bytes = self.text.as_bytes();
        let mut depth = 0usize;
        let mut index = open;
        while let Some(&byte) = bytes.get(index) {
            if let Some((_, end)) = self.quiet_at(index) {
                index = end;
                continue;
            }
            if byte == b'[' {
                depth += 1;
            } else if byte == b']' {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index + 1;
                }
            }
            index += 1;
        }
        bytes.len()
    }

    fn first_use(&self, scope: Scope, from: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut braces = self
            .braces
            .iter()
            .filter(|(at, _)| (scope.start..scope.end).contains(at))
            .peekable();
        let mut line = from;
        while line < scope.end {
            while let Some(&&(at, brace)) = braces.peek()
                && at < line
            {
                depth += if brace == b'{' { 1 } else { -1 };
                braces.next();
            }
            let rest = self.text.get(line..scope.end).unwrap_or_default();
            let first = rest.split('\n').next().unwrap_or_default();
            if depth == 0 && self.quiet_at(line).is_none() && starts_use(first.trim_start()) {
                return Some(self.attributes_above(line, from));
            }
            line += first.len() + 1;
        }
        None
    }

    fn attributes_above(&self, mut line: usize, floor: usize) -> usize {
        while line > floor {
            let previous = self.line_start(line - 1);
            let text = self.text.get(previous..line).unwrap_or_default().trim();
            if previous < floor || !text.starts_with("#[") {
                break;
            }
            line = previous;
        }
        line
    }

    fn indent(&self, scope: Scope, at: usize) -> String {
        if scope.start == 0 {
            return String::new();
        }
        let first = self
            .text
            .get(at..scope.end)
            .unwrap_or_default()
            .split('\n')
            .next()
            .unwrap_or_default();
        let content = first.trim_start();
        if !content.is_empty() && !content.starts_with('}') {
            return first[..first.len() - content.len()].to_owned();
        }
        let line = self
            .text
            .get(self.line_start(scope.start - 1)..)
            .unwrap_or_default();
        let width = line.len() - line.trim_start_matches([' ', '\t']).len();
        format!("{}    ", &line[..width])
    }
}
