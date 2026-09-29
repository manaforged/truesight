use std::collections::HashMap;

use crate::path::{Subject, item_kind};
use crate::surface::{Kind, Surface};

pub enum Resolution {
    Keep,
    Move(Vec<String>),
    Unsure(Doubt),
}

pub enum Doubt {
    Several(Vec<String>),
    Nowhere,
}

enum Home {
    One(String),
    Several(Vec<String>),
    Nowhere,
}

pub struct Homes {
    name: String,
    old: HashMap<String, Vec<Kind>>,
    now: HashMap<String, Vec<Kind>>,
    own: HashMap<String, Vec<(Kind, String)>>,
}

struct Walk<'r> {
    rest: &'r [&'r str],
    done: Vec<String>,
    at: usize,
    moved: bool,
    scope: bool,
}

fn add(kinds: &mut Vec<Kind>, kind: Kind) {
    if !kinds.contains(&kind) {
        kinds.push(kind);
    }
}

fn holds_children(kind: Kind) -> bool {
    kind == Kind::Mod || kind.is_type()
}

fn holds_nothing(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Fn | Kind::Const | Kind::Static | Kind::Macro | Kind::Field | Kind::Variant
    )
}

fn chain(path: &str) -> Vec<&str> {
    let mut segments: Vec<&str> = path.split("::").skip(1).collect();
    segments.pop();
    segments
}

fn within(inner: &[&str], outer: &[&str]) -> bool {
    let mut rest = outer.iter();
    inner
        .iter()
        .all(|segment| rest.any(|other| other == segment))
}

impl Home {
    fn doubt(self) -> Doubt {
        match self {
            Self::Several(paths) => Doubt::Several(paths),
            Self::One(_) | Self::Nowhere => Doubt::Nowhere,
        }
    }

    fn listed(self) -> Doubt {
        match self {
            Self::One(path) => Doubt::Several(vec![path]),
            other => other.doubt(),
        }
    }
}

impl Walk<'_> {
    fn close(&mut self, kinds: &[Kind]) -> Resolution {
        let remaining = self.rest.get(self.at..).unwrap_or_default();
        if !remaining.is_empty() && kinds.iter().all(|kind| holds_nothing(*kind)) {
            return if self.moved {
                Resolution::Unsure(Doubt::Nowhere)
            } else {
                Resolution::Keep
            };
        }
        self.done
            .extend(remaining.iter().map(|segment| (*segment).to_owned()));
        self.at = self.rest.len();
        self.finish()
    }

    fn finish(&mut self) -> Resolution {
        if self.moved {
            Resolution::Move(std::mem::take(&mut self.done))
        } else {
            Resolution::Keep
        }
    }
}

impl Homes {
    pub fn new(surface: &Surface, old: &str) -> Self {
        let mut before: HashMap<String, Vec<Kind>> = HashMap::new();
        for line in old.lines() {
            if let (Subject::Item(path), Some(kind)) = (Subject::of(line), item_kind(line)) {
                add(before.entry(path).or_default(), kind);
            }
        }
        let mut now: HashMap<String, Vec<Kind>> = HashMap::new();
        for entry in surface
            .entries
            .iter()
            .filter(|entry| entry.kind != Kind::Impl && surface.rooted(&entry.path))
        {
            add(now.entry(entry.path.clone()).or_default(), entry.kind);
        }
        let mut own: HashMap<String, Vec<(Kind, String)>> = HashMap::new();
        for named in surface.named() {
            if let Some(&path) = named.paths.first() {
                let leaf = path.rsplit("::").next().unwrap_or(path);
                own.entry(leaf.to_owned())
                    .or_default()
                    .push((named.kind, path.to_owned()));
            }
        }
        Self {
            name: surface.name.clone(),
            old: before,
            now,
            own,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn resolve(&self, rest: &[&str], scope: bool) -> Resolution {
        let mut walk = Walk {
            rest,
            done: Vec::new(),
            at: 0,
            moved: false,
            scope,
        };
        while let Some(segment) = rest.get(walk.at) {
            if let Some(kinds) = self.now.get(&self.path(&walk.done, segment)) {
                walk.done.push((*segment).to_owned());
                walk.at += 1;
                if kinds.contains(&Kind::Mod) {
                    continue;
                }
                return walk.close(kinds);
            }
            if let Some(done) = self.repair(&mut walk) {
                return done;
            }
        }
        walk.finish()
    }

    fn repair(&self, walk: &mut Walk<'_>) -> Option<Resolution> {
        let old = self.original(walk.rest, walk.at);
        let Some(kinds) = self.old.get(&old) else {
            return Some(Resolution::Keep);
        };
        let scope = walk.at + 1 < walk.rest.len() || walk.scope;
        match self.home(&old, kinds, scope) {
            Home::One(new) => {
                walk.done = new.split("::").skip(1).map(str::to_owned).collect();
                walk.at += 1;
                walk.moved = true;
                match self.now.get(&new) {
                    Some(kinds) if kinds.contains(&Kind::Mod) => None,
                    Some(kinds) => Some(walk.close(kinds)),
                    None => Some(Resolution::Unsure(Doubt::Nowhere)),
                }
            }
            home => self.skip_module(walk, kinds, home),
        }
    }

    fn skip_module(&self, walk: &mut Walk<'_>, kinds: &[Kind], home: Home) -> Option<Resolution> {
        if kinds.contains(&Kind::Mod)
            && let Some(next) = walk.rest.get(walk.at + 1)
        {
            if self.now.contains_key(&self.path(&walk.done, next)) {
                walk.at += 1;
                walk.moved = true;
                return None;
            }
            let item = self.original(walk.rest, walk.at + 1);
            let Some(item_kinds) = self.old.get(&item) else {
                return Some(Resolution::Keep);
            };
            let scope = walk.at + 2 < walk.rest.len() || walk.scope;
            return Some(Resolution::Unsure(
                self.home(&item, item_kinds, scope).listed(),
            ));
        }
        Some(Resolution::Unsure(home.doubt()))
    }

    fn home(&self, old: &str, kinds: &[Kind], scope: bool) -> Home {
        let leaf = old.rsplit("::").next().unwrap_or(old);
        let mut found: Vec<&str> = self
            .own
            .get(leaf)
            .into_iter()
            .flatten()
            .filter(|(kind, _)| kinds.contains(kind) && (!scope || holds_children(*kind)))
            .map(|(_, path)| path.as_str())
            .collect();
        found.sort_unstable();
        found.dedup();
        let near = chain(old);
        let close: Vec<&str> = found
            .iter()
            .copied()
            .filter(|path| found.len() == 1 || within(&chain(path), &near))
            .collect();
        match (close.as_slice(), found.is_empty()) {
            ([one], _) => Home::One((*one).to_owned()),
            (_, true) => Home::Nowhere,
            _ => Home::Several(found.into_iter().map(str::to_owned).collect()),
        }
    }

    pub fn import(&self, name: &str, fits: impl Fn(Kind) -> bool) -> Vec<String> {
        self.own
            .get(name)
            .into_iter()
            .flatten()
            .filter(|(kind, path)| fits(*kind) && self.in_module(path))
            .map(|(_, path)| path.clone())
            .collect()
    }

    fn in_module(&self, path: &str) -> bool {
        path.rsplit_once("::").is_some_and(|(parent, _)| {
            self.now
                .get(parent)
                .is_some_and(|kinds| kinds.contains(&Kind::Mod))
        })
    }

    fn path(&self, done: &[String], segment: &str) -> String {
        let mut path = self.name.clone();
        for part in done.iter().map(String::as_str).chain([segment]) {
            path.push_str("::");
            path.push_str(part);
        }
        path
    }

    fn original(&self, rest: &[&str], at: usize) -> String {
        let mut path = self.name.clone();
        for part in rest.iter().take(at + 1) {
            path.push_str("::");
            path.push_str(part);
        }
        path
    }
}
