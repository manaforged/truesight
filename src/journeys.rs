use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::config::{Project, read_optional};
use crate::error::Error;
use crate::package::Crate;
use crate::source::{Source, ident_len};

const IGNORED: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "bool", "char", "str", "u8", "u16", "u32", "u64", "u128",
    "usize", "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "String", "Vec", "Option",
    "Some", "None", "Ok", "Err", "Result", "Box", "std", "core", "alloc", "format", "println",
    "vec", "main",
];

const DEFINERS: &[&str] = &[
    "fn", "struct", "enum", "mod", "trait", "type", "const", "static", "union",
];

pub struct JourneyConfig {
    pub prefix: Option<String>,
    pub budgets: BTreeMap<String, usize>,
    pub ignore: Vec<String>,
}

pub struct Journey {
    pub name: String,
    pub names: Vec<String>,
    pub budget: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Token<'t> {
    Ident(&'t str),
    Punct(&'t str),
}

pub fn measure(project: &Project, krate: &Crate, spine: &str) -> Result<Vec<Journey>, Error> {
    let Some(prefix) = &krate.journeys.prefix else {
        return Ok(Vec::new());
    };
    let own = names(spine, &krate.name);
    let mut journeys = Vec::new();
    for example in krate
        .examples
        .iter()
        .filter(|example| example.name.starts_with(prefix.as_str()))
    {
        let text = read_optional(&example.source)?.unwrap_or_default();
        let code = Source::new(&text).code();
        let tokens = without_attributes(&tokenize(&code));
        let mut known = own.clone();
        for (library, path) in project.spines.iter().filter(|(library, _)| {
            *library != krate.name && tokens.contains(&Token::Ident(library.as_str()))
        }) {
            known.extend(names(&read_optional(path)?.unwrap_or_default(), library));
        }
        journeys.push(Journey {
            name: example.name.clone(),
            names: used(&tokens, &known, &krate.journeys.ignore),
            budget: krate.journeys.budgets.get(&example.name).copied(),
        });
    }
    Ok(journeys)
}

pub fn problems(krate: &Crate, journeys: &[Journey]) -> Vec<String> {
    let mut found = Vec::new();
    for journey in journeys {
        let count = journey.names.len();
        match journey.budget {
            None => found.push(format!(
                "journey `{}` uses {count} names and has no budget",
                journey.name
            )),
            Some(budget) if count > budget => found.push(format!(
                "journey `{}` uses {count} names, over its budget of {budget}: {}",
                journey.name,
                journey.names.join(" ")
            )),
            Some(budget) if count < budget => found.push(format!(
                "journey `{}` uses {count} names; lower its budget from {budget} to {count}",
                journey.name
            )),
            Some(_) => {}
        }
    }
    for name in krate.journeys.budgets.keys() {
        if !journeys.iter().any(|journey| &journey.name == name) {
            found.push(format!("budget `{name}` names no journey example"));
        }
    }
    found
}

pub fn names(spine: &str, library: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::from([library.to_owned()]);
    for path in paths(spine, library) {
        found.extend(path.split("::").skip(1).map(str::to_owned));
    }
    found
}

pub fn paths(spine: &str, library: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let tokens = tokenize(spine);
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        index += 1;
        if *token != Token::Ident(library) {
            continue;
        }
        let mut path = library.to_owned();
        while let (Some(Token::Punct("::")), Some(Token::Ident(segment))) =
            (tokens.get(index), tokens.get(index + 1))
        {
            path.push_str("::");
            path.push_str(segment);
            found.insert(path.clone());
            index += 2;
        }
    }
    found
}

fn used(tokens: &[Token<'_>], known: &BTreeSet<String>, extra: &[String]) -> Vec<String> {
    let own = declared(tokens);
    let skip: HashSet<&str> = IGNORED
        .iter()
        .copied()
        .chain(extra.iter().map(String::as_str))
        .collect();
    let mut used = BTreeSet::new();
    for token in tokens {
        if let Token::Ident(word) = token
            && known.contains(*word)
            && !skip.contains(word)
            && !own.contains(word)
        {
            used.insert((*word).to_owned());
        }
    }
    used.into_iter().collect()
}

fn tokenize(code: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut at = 0;
    while let Some(rest) = code.get(at..) {
        let Some(first) = rest.chars().next() else {
            break;
        };
        let ident = ident_len(rest);
        let length = if first.is_whitespace() {
            first.len_utf8()
        } else if ident > 0 {
            tokens.push(Token::Ident(rest.get(..ident).unwrap_or_default()));
            ident
        } else if first.is_ascii_digit() {
            rest.find(|next: char| !(next.is_alphanumeric() || next == '_' || next == '.'))
                .unwrap_or(rest.len())
        } else {
            let width = ["::", "=>", "||", "->"]
                .iter()
                .find(|pair| rest.starts_with(**pair))
                .map_or(first.len_utf8(), |pair| pair.len());
            tokens.push(Token::Punct(rest.get(..width).unwrap_or_default()));
            width
        };
        at += length;
    }
    tokens
}

fn without_attributes<'t>(tokens: &[Token<'t>]) -> Vec<Token<'t>> {
    let mut kept = Vec::with_capacity(tokens.len());
    let mut index = 0;
    while let Some(&token) = tokens.get(index) {
        let open = match (tokens.get(index + 1), tokens.get(index + 2)) {
            (Some(Token::Punct("[")), _) => Some(index + 1),
            (Some(Token::Punct("!")), Some(Token::Punct("["))) => Some(index + 2),
            _ => None,
        };
        match open.filter(|_| token == Token::Punct("#")) {
            Some(open) => index = closing(tokens, open, "[", "]") + 1,
            None => {
                kept.push(token);
                index += 1;
            }
        }
    }
    kept
}

fn closing(tokens: &[Token<'_>], open: usize, left: &str, right: &str) -> usize {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if *token == Token::Punct(left) {
            depth += 1;
        } else if *token == Token::Punct(right) {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return index;
            }
        }
    }
    tokens.len()
}

fn declared<'t>(tokens: &[Token<'t>]) -> HashSet<&'t str> {
    let mut found = HashSet::new();
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::Ident("let") => bindings(tokens, index + 1, &["=", ";"], &mut found),
            Token::Ident("for") => bindings(tokens, index + 1, &["in"], &mut found),
            Token::Ident(word) if DEFINERS.contains(word) => definition(tokens, index, &mut found),
            Token::Punct("|") => closure(tokens, index, &mut found),
            Token::Punct("=>") => arm(tokens, index, &mut found),
            _ => {}
        }
    }
    found
}

fn binding(word: &str) -> bool {
    word.starts_with(|first: char| first.is_lowercase() || first == '_')
        && !matches!(word, "mut" | "ref")
}

fn bindings<'t>(tokens: &[Token<'t>], from: usize, ends: &[&str], found: &mut HashSet<&'t str>) {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(from) {
        match token {
            Token::Punct("(" | "[" | "{") => depth += 1,
            Token::Punct(")" | "]" | "}") => depth = depth.saturating_sub(1),
            Token::Punct(":") if depth == 0 => return,
            Token::Punct(end) | Token::Ident(end) if depth == 0 && ends.contains(end) => return,
            Token::Ident(word) if binding(word) && !follows_path(tokens, index) => {
                found.insert(word);
            }
            _ => {}
        }
    }
}

fn follows_path(tokens: &[Token<'_>], index: usize) -> bool {
    matches!(
        tokens.get(index + 1),
        Some(Token::Punct("::" | "(" | "{" | "!"))
    )
}

fn definition<'t>(tokens: &[Token<'t>], index: usize, found: &mut HashSet<&'t str>) {
    let Some(Token::Ident(name)) = tokens.get(index + 1) else {
        return;
    };
    found.insert(name);
    let mut open = index + 2;
    if tokens.get(open) == Some(&Token::Punct("<")) {
        open = closing(tokens, open, "<", ">") + 1;
    }
    let (left, right) = match tokens.get(open) {
        Some(Token::Punct("(")) if tokens.get(index) == Some(&Token::Ident("fn")) => ("(", ")"),
        Some(Token::Punct("{")) if tokens.get(index) == Some(&Token::Ident("struct")) => ("{", "}"),
        _ => return,
    };
    let end = closing(tokens, open, left, right);
    labelled(tokens, open + 1, end, found);
}

fn labelled<'t>(tokens: &[Token<'t>], from: usize, to: usize, found: &mut HashSet<&'t str>) {
    for index in from..to {
        if let (Some(Token::Ident(word)), Some(Token::Punct(":"))) =
            (tokens.get(index), tokens.get(index + 1))
        {
            found.insert(word);
        }
    }
}

fn closure<'t>(tokens: &[Token<'t>], index: usize, found: &mut HashSet<&'t str>) {
    let opens = index == 0
        || matches!(
            tokens.get(index - 1),
            Some(Token::Punct("(" | "," | "=" | "{") | Token::Ident("move" | "return"))
        );
    if !opens {
        return;
    }
    let Some(end) = tokens
        .iter()
        .skip(index + 1)
        .position(|token| *token == Token::Punct("|"))
        .map(|offset| index + 1 + offset)
    else {
        return;
    };
    for token in tokens.get(index + 1..end).unwrap_or_default() {
        if let Token::Ident(word) = token
            && binding(word)
        {
            found.insert(word);
        }
    }
}

fn arm<'t>(tokens: &[Token<'t>], index: usize, found: &mut HashSet<&'t str>) {
    let mut depth = 0usize;
    let mut candidates = Vec::new();
    for back in (0..index).rev() {
        match tokens.get(back) {
            Some(Token::Punct(")" | "]" | "}")) => depth += 1,
            Some(Token::Punct("(" | "[")) => depth = depth.saturating_sub(1),
            Some(Token::Punct("{" | ",")) if depth == 0 => break,
            Some(Token::Punct("{")) => depth -= 1,
            Some(Token::Ident("if")) if depth == 0 => candidates.clear(),
            Some(Token::Ident(word))
                if depth > 0 && binding(word) && !follows_path(tokens, back) =>
            {
                candidates.push(*word);
            }
            _ => {}
        }
    }
    found.extend(candidates);
}
