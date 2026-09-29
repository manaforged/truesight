use std::collections::HashSet;
use std::path::{Component, Path};

#[derive(Clone, Copy)]
pub enum Align {
    Left,
    Right,
}

pub fn table(header: &[(&str, Align)], rows: &[Vec<String>]) -> String {
    let mut out = row(header.iter().map(|(name, _)| (*name).to_owned()));
    out.push_str(&row(header.iter().map(|(_, align)| match align {
        Align::Left => String::from("---"),
        Align::Right => String::from("---:"),
    })));
    for cells in rows {
        out.push_str(&row(cells.iter().map(|cell| cell.replace('|', "\\|"))));
    }
    out
}

fn row(cells: impl Iterator<Item = String>) -> String {
    let cells: Vec<String> = cells.collect();
    format!("| {} |\n", cells.join(" | "))
}

pub fn code(text: &str) -> String {
    if text.contains('`') {
        format!("`` {text} ``")
    } else {
        format!("`{text}`")
    }
}

pub fn short<'a>(path: &'a str, name: &str) -> &'a str {
    path.strip_prefix(name)
        .and_then(|rest| rest.strip_prefix("::"))
        .unwrap_or(path)
}

pub fn short_header(text: &str, name: &str) -> String {
    let root = format!("{name}::");
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for (index, _) in text.match_indices(&root) {
        let inside = text[..index]
            .chars()
            .next_back()
            .is_some_and(|before| before.is_alphanumeric() || before == '_' || before == ':');
        if !inside {
            out.push_str(&text[copied..index]);
            copied = index + root.len();
        }
    }
    out.push_str(&text[copied..]);
    out
}

#[derive(Default)]
pub struct Anchors {
    used: HashSet<String>,
}

impl Anchors {
    pub fn next(&mut self, heading: &str) -> String {
        let id = heading_id(heading);
        if self.used.insert(id.clone()) {
            return id;
        }
        let mut counter = 1u32;
        loop {
            let candidate = format!("{id}-{counter}");
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
            counter += 1;
        }
    }
}

fn heading_id(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|current| {
            if current.is_alphanumeric() || current == '_' || current == '-' {
                Some(current)
            } else if current.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

pub fn link(from_dir: &Path, to: &Path) -> String {
    relative(from_dir, to)
        .chars()
        .map(|current| match current {
            ' ' => String::from("%20"),
            '(' => String::from("%28"),
            ')' => String::from("%29"),
            '<' => String::from("%3C"),
            '>' => String::from("%3E"),
            other => other.to_string(),
        })
        .collect()
}

pub fn relative(from_dir: &Path, to: &Path) -> String {
    let from: Vec<Component<'_>> = from_dir.components().collect();
    let target: Vec<Component<'_>> = to.components().collect();
    let shared = from
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts: Vec<String> = vec![String::from(".."); from.len().saturating_sub(shared)];
    parts.extend(
        target
            .iter()
            .skip(shared)
            .map(|part| part.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}
