use crate::markdown::{Align, table};
use crate::modules::Group;
use crate::path::skip_attributes;
use crate::surface::{Entry, Kind};

const DECLARED: [Kind; 6] = [
    Kind::Fn,
    Kind::Type,
    Kind::Const,
    Kind::Static,
    Kind::Macro,
    Kind::TraitAlias,
];

pub struct Context<'a> {
    pub krate: &'a str,
    pub full: &'a str,
    pub tasks: &'a dyn Fn(&str) -> Vec<String>,
    pub link: &'a dyn Fn(&str) -> Option<String>,
}

#[derive(Default)]
struct Parts {
    methods: Vec<Vec<String>>,
    fields: Vec<Vec<String>>,
    variants: Vec<Vec<String>>,
    traits: Vec<String>,
}

pub fn render(group: &Group<'_>, context: &Context<'_>) -> String {
    let mut out = String::new();
    if let Some(head) = group.head {
        if DECLARED.contains(&head.kind) {
            let body = skip_attributes(head.line.trim_start());
            out.push_str(&format!("<pre>{}</pre>\n\n", shorten(body, context)));
        }
        let notes = describe(head, &(context.tasks)(&head.path));
        if !notes.is_empty() && head.summary.is_none() {
            out.push_str(&format!("{notes}\n\n"));
        }
    }
    let parts = collect(group, context);
    out.push_str(&section("Methods", "Method", &parts.methods));
    out.push_str(&section("Fields", "Field", &parts.fields));
    out.push_str(&section("Variants", "Variant", &parts.variants));
    if !parts.traits.is_empty() {
        out.push_str(&format!(
            "**Trait implementations:** {}\n\n",
            parts.traits.join(", ")
        ));
    }
    out
}

pub fn description(entry: &Entry, tasks: &[String]) -> String {
    describe(entry, tasks)
}

fn collect(group: &Group<'_>, context: &Context<'_>) -> Parts {
    let mut parts = Parts::default();
    let head = group.head.map(|head| head.path.as_str());
    for entry in &group.entries {
        if entry.kind == Kind::Impl {
            if let Some(name) = entry.trait_impl.then(|| implemented(&entry.line)).flatten() {
                let name = format!("<code>{}</code>", shorten(&name, context));
                if !parts.traits.contains(&name) {
                    parts.traits.push(name);
                }
            }
            continue;
        }
        if entry.trait_impl || Some(entry.path.as_str()) == head {
            continue;
        }
        let row = vec![
            format!("<code>{}</code>", member(entry, context)),
            describe(entry, &(context.tasks)(&entry.path)),
        ];
        match entry.kind {
            Kind::Field => parts.fields.push(row),
            Kind::Variant => parts.variants.push(row),
            _ => parts.methods.push(row),
        }
    }
    parts
}

fn section(title: &str, column: &str, rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    format!(
        "**{title}**\n\n{}\n",
        table(&[(column, Align::Left), ("Description", Align::Left)], rows)
    )
}

fn describe(entry: &Entry, tasks: &[String]) -> String {
    let mut notes: Vec<String> = Vec::new();
    match &entry.summary {
        Some(summary) => notes.push(summary.clone()),
        None if !tasks.is_empty() => notes.push(format!("{}.", tasks.join("; "))),
        None => {}
    }
    let features = features(&entry.line);
    if !features.is_empty() {
        let names: Vec<String> = features.iter().map(|name| format!("`{name}`")).collect();
        notes.push(format!("Requires feature {}.", names.join(" or ")));
    }
    if let Some(note) = entry.note() {
        notes.push(format!("Deprecated: {note}"));
    } else if entry.deprecation.is_some() {
        notes.push(String::from("Deprecated."));
    }
    notes.join(" ")
}

fn member(entry: &Entry, context: &Context<'_>) -> String {
    let body = skip_attributes(entry.line.trim_start());
    let owner = format!("{}::", context.full);
    let shown = match body.find(&owner) {
        Some(at) => {
            let qualifiers: Vec<&str> = body[..at]
                .split_whitespace()
                .filter(|word| !matches!(*word, "pub" | "fn"))
                .collect();
            let rest = &body[at + owner.len()..];
            if qualifiers.is_empty() {
                rest.to_owned()
            } else {
                format!("{} {rest}", qualifiers.join(" "))
            }
        }
        None => body.to_owned(),
    };
    shorten(&shown, context)
}

fn features(line: &str) -> Vec<&str> {
    let attributes = &line[..line.len() - skip_attributes(line.trim_start()).len()];
    attributes.split('"').skip(1).step_by(2).collect()
}

fn implemented(line: &str) -> Option<String> {
    let text = skip_attributes(line.trim_start());
    let text = text.strip_prefix("unsafe ").unwrap_or(text);
    let text = text.strip_prefix("impl")?;
    let text = match text.strip_prefix('<') {
        Some(generics) => after_close(generics)?,
        None => text,
    };
    let (name, _) = text.trim_start().split_once(" for ")?;
    Some(name.trim().to_owned())
}

fn after_close(text: &str) -> Option<&str> {
    let mut depth = 1usize;
    for (index, current) in text.char_indices() {
        match current {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return text.get(index + 1..);
                }
            }
            _ => {}
        }
    }
    None
}

fn shorten(text: &str, context: &Context<'_>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while let Some(&current) = chars.get(index) {
        let starts = is_start(current)
            && index
                .checked_sub(1)
                .and_then(|before| chars.get(before))
                .is_none_or(|before| !is_ident(*before) && *before != ':' && *before != '\'');
        if !starts {
            out.push_str(&escape(current));
            index += 1;
            continue;
        }
        let (segments, end) = read_path(&chars, index);
        out.push_str(&shown_path(&segments, context));
        index = end;
    }
    out
}

fn read_path(chars: &[char], start: usize) -> (Vec<String>, usize) {
    let mut segments = Vec::new();
    let mut index = start;
    loop {
        let begin = index;
        while chars.get(index).is_some_and(|current| is_ident(*current)) {
            index += 1;
        }
        segments.push(chars[begin..index].iter().collect());
        let joined = chars.get(index) == Some(&':')
            && chars.get(index + 1) == Some(&':')
            && chars.get(index + 2).is_some_and(|next| is_start(*next));
        if !joined {
            return (segments, index);
        }
        index += 2;
    }
}

fn shown_path(segments: &[String], context: &Context<'_>) -> String {
    match segments {
        [] => String::new(),
        [only] => only.clone(),
        [first, .., last] if first == context.krate => match (context.link)(&segments.join("::")) {
            Some(href) => format!("<a href=\"{href}\">{last}</a>"),
            None => segments[1..].join("::"),
        },
        [first, .., last] if first.starts_with(|c: char| c.is_ascii_lowercase()) => last.clone(),
        all => all.join("::"),
    }
}

fn escape(current: char) -> String {
    match current {
        '&' => String::from("&amp;"),
        '<' => String::from("&lt;"),
        '>' => String::from("&gt;"),
        '|' => String::from("&#124;"),
        other => other.to_string(),
    }
}

fn is_start(current: char) -> bool {
    current.is_alphabetic() || current == '_'
}

fn is_ident(current: char) -> bool {
    current.is_alphanumeric() || current == '_'
}
