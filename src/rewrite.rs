use crate::homes::{Doubt, Homes, Resolution};
use crate::path::skip_attributes;
use crate::source::{ident_len, is_ident, visibility};

pub struct Unsure {
    pub line: usize,
    pub path: String,
    pub doubt: Doubt,
}

#[derive(Default)]
pub struct Rewrite {
    pub text: String,
    pub moved: usize,
    pub unsure: Vec<Unsure>,
}

struct Found<'t> {
    start: usize,
    end: usize,
    root: &'t str,
    segments: Vec<&'t str>,
    scope: bool,
}

struct Group<'t> {
    end: usize,
    head: &'t str,
    lead: &'t str,
    root: &'t str,
    base: Vec<&'t str>,
    members: Vec<(Vec<&'t str>, Option<&'t str>)>,
}

pub fn file(text: &str, roots: &[&str], homes: &Homes) -> Rewrite {
    let (grouped, split) = groups(text, roots, homes);
    let mut rewrite = paths(&grouped, roots, homes);
    rewrite.moved += split;
    rewrite
}

fn paths(text: &str, roots: &[&str], homes: &Homes) -> Rewrite {
    let mut rewrite = Rewrite::default();
    let mut copied = 0;
    for found in scan(text, roots) {
        match homes.resolve(&found.segments, found.scope) {
            Resolution::Keep => {}
            Resolution::Move(segments) => {
                let new = format!("{}::{}", found.root, segments.join("::"));
                if text.get(found.start..found.end) != Some(new.as_str()) {
                    rewrite.text.push_str(&text[copied..found.start]);
                    rewrite.text.push_str(&new);
                    copied = found.end;
                    rewrite.moved += 1;
                }
            }
            Resolution::Unsure(doubt) => rewrite.unsure.push(Unsure {
                line: text[..found.start].matches('\n').count() + 1,
                path: text[found.start..found.end].to_owned(),
                doubt,
            }),
        }
    }
    rewrite.text.push_str(&text[copied..]);
    rewrite
}

fn words(text: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let mut start = None;
    for (index, current) in text.char_indices().chain([(text.len(), ' ')]) {
        let inside = current.is_alphanumeric() || current == '_';
        match (start, inside) {
            (None, true) => start = Some(index),
            (Some(from), false) => {
                found.push((from, &text[from..index]));
                start = None;
            }
            _ => {}
        }
    }
    found
}

fn free(text: &str, start: usize) -> bool {
    let before = &text[..start];
    match before.strip_suffix("::") {
        Some(head) => !head.ends_with(|current: char| {
            current.is_alphanumeric() || matches!(current, '_' | '>' | ')' | ']' | ':' | '$')
        }),
        None => !before.ends_with(['$', '\'', '#']),
    }
}

fn scan<'t>(text: &'t str, roots: &[&str]) -> Vec<Found<'t>> {
    let mut found: Vec<Found<'t>> = Vec::new();
    for (start, word) in words(text) {
        if found.last().is_some_and(|last| start < last.end)
            || !roots.contains(&word)
            || !free(text, start)
        {
            continue;
        }
        if let Some(path) = read(text, start, word) {
            found.push(path);
        }
    }
    found
}

fn read<'t>(text: &'t str, start: usize, root: &'t str) -> Option<Found<'t>> {
    let mut at = start + root.len();
    let mut segments = Vec::new();
    let mut scope = false;
    while let Some(rest) = text.get(at..).and_then(|rest| rest.strip_prefix("::")) {
        if rest.starts_with(['{', '*']) {
            scope = true;
            break;
        }
        let length = ident_len(rest);
        if length == 0 {
            break;
        }
        segments.push(&rest[..length]);
        at += 2 + length;
    }
    (!segments.is_empty()).then_some(Found {
        start,
        end: at,
        root,
        segments,
        scope,
    })
}

fn groups(text: &str, roots: &[&str], homes: &Homes) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut moved = 0;
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let here = start;
        start += line.len();
        if here < copied {
            continue;
        }
        let Some(group) = group_at(text, here, roots) else {
            continue;
        };
        if attached(text, here) {
            continue;
        }
        let Some((lines, count)) = split(&group, homes) else {
            continue;
        };
        out.push_str(&text[copied..here]);
        out.push_str(&lines);
        copied = group.end;
        moved += count;
    }
    out.push_str(&text[copied..]);
    (out, moved)
}

fn attached(text: &str, at: usize) -> bool {
    text[..at]
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("//"))
        .is_some_and(|line| line.ends_with(']') && !line.starts_with("#!["))
}

fn head_len(line: &str) -> Option<usize> {
    let rest = visibility(skip_attributes(line.trim_start_matches([' ', '\t'])));
    let after = rest.strip_prefix("use")?;
    after
        .starts_with([' ', '\t'])
        .then_some(line.len() - rest.len())
}

fn group_at<'t>(text: &'t str, at: usize, roots: &[&'t str]) -> Option<Group<'t>> {
    let line = &text[at..];
    let head = head_len(line)?;
    let spaced = line[head + "use".len()..].trim_start_matches([' ', '\t']);
    let (lead, body) = match spaced.strip_prefix("::") {
        Some(body) => ("::", body),
        None => ("", spaced),
    };
    let root = *roots
        .iter()
        .find(|root| body.get(..ident_len(body)) == Some(**root))?;
    let mut rest = body[root.len()..].strip_prefix("::")?;
    let mut base = Vec::new();
    while !rest.starts_with('{') {
        let length = ident_len(rest);
        if length == 0 {
            return None;
        }
        base.push(&rest[..length]);
        rest = rest[length..].strip_prefix("::")?;
    }
    let inner = &rest[1..];
    let close = inner.find('}')?;
    let content = &inner[..close];
    let tail = inner[close + 1..].trim_start().strip_prefix(';')?;
    let members = content
        .split(',')
        .map(str::trim)
        .filter(|member| !member.is_empty())
        .map(member)
        .collect::<Option<Vec<_>>>()?;
    (!content.contains('{')).then_some(Group {
        end: text.len() - tail.len(),
        head: &line[..head],
        lead,
        root,
        base,
        members,
    })
}

fn member(text: &str) -> Option<(Vec<&str>, Option<&str>)> {
    let (path, alias) = match text.split_once(" as ") {
        Some((path, alias)) => (path.trim(), Some(alias.trim())),
        None => (text, None),
    };
    let named = |word: &str| is_ident(word) && !matches!(word, "self" | "super" | "crate");
    let segments: Vec<&str> = path.split("::").map(str::trim).collect();
    let valid = segments.iter().all(|&segment| named(segment))
        && alias.is_none_or(|alias| alias == "_" || named(alias));
    valid.then_some((segments, alias))
}

fn split(group: &Group<'_>, homes: &Homes) -> Option<(String, usize)> {
    let mut moved = 0;
    let lines: Vec<String> = group
        .members
        .iter()
        .map(|(path, alias)| {
            let full: Vec<&str> = group.base.iter().chain(path).copied().collect();
            let target = match homes.resolve(&full, false) {
                Resolution::Move(new) => {
                    moved += 1;
                    new.join("::")
                }
                Resolution::Keep | Resolution::Unsure(_) => full.join("::"),
            };
            let alias = alias
                .map(|alias| format!(" as {alias}"))
                .unwrap_or_default();
            format!(
                "{}use {}{}::{target}{alias};",
                group.head, group.lead, group.root
            )
        })
        .collect();
    (moved > 0).then(|| (lines.join("\n"), moved))
}
