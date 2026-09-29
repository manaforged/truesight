use crate::surface::Kind;

const QUALIFIERS: [&str; 19] = [
    "pub", "const", "async", "unsafe", "safe", "extern", "crate", "fn", "struct", "enum", "union",
    "trait", "auto", "type", "mod", "use", "static", "mut", "macro",
];

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Subject {
    Item(String),
    Impl(String),
}

impl Subject {
    pub fn of(line: &str) -> Self {
        match head(line) {
            Head::Impl { header, .. } => Self::Impl(header.to_owned()),
            Head::Item(rest) => Self::Item(read_path(strip_reference(rest))),
        }
    }

    pub fn item(&self) -> Option<&str> {
        match self {
            Self::Item(path) => Some(path.as_str()),
            Self::Impl(_) => None,
        }
    }
}

enum Head<'a> {
    Item(&'a str),
    Impl { header: &'a str, body: &'a str },
}

fn head(line: &str) -> Head<'_> {
    let header = skip_attributes(line.trim_start());
    let mut rest = header;
    loop {
        let (word, after) = rest.split_once(' ').unwrap_or((rest, ""));
        if word == "impl" || word.starts_with("impl<") {
            return Head::Impl {
                header,
                body: &rest["impl".len()..],
            };
        }
        if is_qualifier(word) {
            rest = after.trim_start();
        } else {
            return Head::Item(rest);
        }
    }
}

fn is_qualifier(word: &str) -> bool {
    QUALIFIERS.contains(&word) || word.starts_with('"')
}

pub fn item_path(line: &str) -> String {
    match head(line) {
        Head::Impl { body, .. } => impl_subject(body),
        Head::Item(rest) => read_path(strip_reference(rest)),
    }
}

pub fn item_kind(line: &str) -> Option<Kind> {
    let Head::Item(rest) = head(line) else {
        return None;
    };
    let header = skip_attributes(line.trim_start());
    let words = header.get(..header.len() - rest.len()).unwrap_or_default();
    let named = words.split_whitespace().rev().find_map(keyword);
    Some(named.unwrap_or_else(|| {
        let (_, after) = path_and_rest(strip_reference(rest));
        if after.starts_with(':') {
            Kind::Field
        } else {
            Kind::Variant
        }
    }))
}

fn keyword(word: &str) -> Option<Kind> {
    let kind = match word {
        "mod" => Kind::Mod,
        "use" => Kind::Use,
        "struct" => Kind::Struct,
        "union" => Kind::Union,
        "enum" => Kind::Enum,
        "fn" => Kind::Fn,
        "trait" => Kind::Trait,
        "type" => Kind::Type,
        "const" => Kind::Const,
        "static" => Kind::Static,
        "macro" => Kind::Macro,
        "crate" => Kind::ExternCrate,
        _ => return None,
    };
    Some(kind)
}

pub fn is_mod(line: &str) -> bool {
    skip_attributes(line.trim_start())
        .split(' ')
        .take_while(|word| is_qualifier(word))
        .any(|word| word == "mod")
}

pub fn rehome(line: &str, from: &str, to: &str) -> Option<String> {
    let Head::Item(rest) = head(line) else {
        return None;
    };
    let text = strip_reference(rest);
    let below = text.strip_prefix(from)?.strip_prefix("::")?;
    Some(format!("{}{to}::{below}", &line[..line.len() - text.len()]))
}

pub fn skip_attributes(mut text: &str) -> &str {
    while let Some(attribute) = text.strip_prefix('#').filter(|rest| rest.starts_with('[')) {
        text = after_group(attribute, '[', ']').trim_start();
    }
    text
}

fn impl_subject(text: &str) -> String {
    let text = if text.starts_with('<') {
        after_group(text, '<', '>')
    } else {
        text
    };
    let text = text.trim_start().trim_start_matches('!');
    let Some(index) = top_level(text, " for ") else {
        return read_path(strip_reference(text));
    };
    let subject = read_path(strip_reference(&text[index + " for ".len()..]));
    if subject.is_empty() {
        read_path(strip_reference(&text[..index]))
    } else {
        subject
    }
}

fn strip_reference(mut text: &str) -> &str {
    loop {
        text = text.trim_start().trim_start_matches('&');
        if text.starts_with('\'') {
            text = text.split_once(' ').map_or("", |(_, rest)| rest);
        } else if let Some(rest) = text
            .strip_prefix("mut ")
            .or_else(|| text.strip_prefix("dyn "))
        {
            text = rest;
        } else {
            return text;
        }
    }
}

fn read_path(text: &str) -> String {
    path_and_rest(text).0
}

fn path_and_rest(mut text: &str) -> (String, &str) {
    let mut segments = Vec::new();
    if text.starts_with('(') {
        let rest = after_group(text, '(', ')');
        segments.push(&text[..text.len() - rest.len()]);
        match rest.strip_prefix("::") {
            Some(next) => text = next,
            None => return (segments.concat(), rest),
        }
    }
    loop {
        let end = text
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '#'))
            .unwrap_or(text.len());
        if end == 0 {
            break;
        }
        let (segment, rest) = text.split_at(end);
        segments.push(segment);
        text = if rest.starts_with('<') {
            after_group(rest, '<', '>')
        } else {
            rest
        };
        match text.strip_prefix("::") {
            Some(next) => text = next,
            None => break,
        }
    }
    (segments.join("::"), text)
}

fn after_group(text: &str, open: char, close: char) -> &str {
    let mut depth = 0usize;
    let mut previous = ' ';
    for (index, current) in text.char_indices() {
        if current == open {
            depth += 1;
        } else if current == close && !(close == '>' && previous == '-') {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return &text[index + current.len_utf8()..];
            }
        }
        previous = current;
    }
    ""
}

fn top_level(text: &str, needle: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut previous = ' ';
    for (index, current) in text.char_indices() {
        match current {
            '<' | '(' | '[' => depth += 1,
            '>' if previous != '-' => depth -= 1,
            ')' | ']' => depth -= 1,
            _ => {}
        }
        if depth == 0 && text[index..].starts_with(needle) {
            return Some(index);
        }
        previous = current;
    }
    None
}
