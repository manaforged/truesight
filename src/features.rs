use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::thread::{self, ScopedJoinHandle};

use crate::config::Project;
use crate::error::Error;
use crate::package::Crate;
use crate::surface::{Entry, Kind, Listing};

const DEPRECATED: &str = "#[deprecated] ";

pub struct Plan {
    pub base: Vec<String>,
    pub gates: Vec<(String, Vec<String>)>,
    pub none: Option<Vec<String>>,
    pub only: Vec<(String, Vec<String>)>,
    pub implies: HashMap<String, Vec<String>>,
}

pub fn plan(krate: &Crate) -> Result<Plan, Error> {
    let table = &krate.feature_table;
    if let Some(missing) = krate
        .features
        .iter()
        .chain(&krate.gates)
        .find(|name| !table.contains_key(name.as_str()))
    {
        return Err(Error::UnknownFeature {
            package: krate.package.clone(),
            feature: missing.clone(),
        });
    }
    let mut roots: Vec<&str> = krate.features.iter().map(String::as_str).collect();
    if krate.default_features {
        roots.push("default");
    }
    let enabled = closure(table, roots);
    let mut gates = Vec::new();
    for gate in &krate.gates {
        if !enabled.contains(gate.as_str()) {
            return Err(Error::GateNotEnabled {
                package: krate.package.clone(),
                gate: gate.clone(),
            });
        }
        gates.push((gate.clone(), owned(&without(table, &enabled, &[gate]))));
    }
    let every: Vec<&String> = krate.gates.iter().collect();
    let none = (every.len() > 1).then(|| without(table, &enabled, &every));
    let only = none.as_ref().map_or_else(Vec::new, |none| {
        krate
            .gates
            .iter()
            .map(|gate| {
                let mut on = none.clone();
                on.extend(closure(table, [gate.as_str()]));
                (gate.clone(), owned(&on))
            })
            .collect()
    });
    let implies = krate
        .gates
        .iter()
        .map(|gate| {
            let reached = closure(table, [gate.as_str()]);
            let others = krate
                .gates
                .iter()
                .filter(|other| *other != gate && reached.contains(other.as_str()))
                .cloned()
                .collect();
            (gate.clone(), others)
        })
        .collect();
    Ok(Plan {
        base: owned(&enabled),
        gates,
        none: none.as_ref().map(owned),
        only,
        implies,
    })
}

fn owned(names: &BTreeSet<&str>) -> Vec<String> {
    names.iter().copied().map(str::to_owned).collect()
}

fn without<'a>(
    table: &'a BTreeMap<String, Vec<String>>,
    enabled: &BTreeSet<&'a str>,
    gates: &[&String],
) -> BTreeSet<&'a str> {
    enabled
        .iter()
        .copied()
        .filter(|name| {
            let reached = closure(table, [*name]);
            !gates.iter().any(|gate| reached.contains(gate.as_str()))
        })
        .collect()
}

fn closure<'a>(
    table: &'a BTreeMap<String, Vec<String>>,
    roots: impl IntoIterator<Item = &'a str>,
) -> BTreeSet<&'a str> {
    let mut enabled = BTreeSet::new();
    let mut pending: Vec<&'a str> = roots.into_iter().collect();
    while let Some(name) = pending.pop() {
        let Some((key, implied)) = table.get_key_value(name) else {
            continue;
        };
        if enabled.insert(key.as_str()) {
            pending.extend(implied.iter().filter_map(|entry| own_feature(table, entry)));
        }
    }
    enabled
}

fn own_feature<'a>(table: &BTreeMap<String, Vec<String>>, entry: &'a str) -> Option<&'a str> {
    if entry.starts_with("dep:") || entry.contains("?/") {
        return None;
    }
    let name = entry.split_once('/').map_or(entry, |(head, _)| head);
    table.contains_key(name).then_some(name)
}

enum Condition {
    All(Vec<String>),
    Any(Vec<String>),
    Not(String),
}

impl Condition {
    fn any(gates: Vec<String>) -> Self {
        if gates.len() == 1 {
            Self::All(gates)
        } else {
            Self::Any(gates)
        }
    }

    fn prefix(&self) -> String {
        match self {
            Self::All(gates) => match gates.as_slice() {
                [] => String::new(),
                [gate] => format!("#[cfg(feature = \"{gate}\")] "),
                many => format!("#[cfg(all({}))] ", each(many)),
            },
            Self::Any(gates) => format!("#[cfg(any({}))] ", each(gates)),
            Self::Not(gate) => format!("#[cfg(not(feature = \"{gate}\"))] "),
        }
    }

    fn line(&self, entry: &Entry) -> String {
        let deprecated = if entry.deprecation.is_some() {
            DEPRECATED
        } else {
            ""
        };
        format!("{}{deprecated}{}", self.prefix(), entry.line)
    }

    fn into_gates(self) -> Vec<String> {
        match self {
            Self::All(gates) | Self::Any(gates) => gates,
            Self::Not(_) => Vec::new(),
        }
    }
}

fn each(gates: &[String]) -> String {
    gates
        .iter()
        .map(|gate| format!("feature = \"{gate}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

struct Wave {
    base: Listing,
    offs: Vec<Listing>,
    none: Option<Listing>,
}

pub fn gated(project: &Project, krate: &Crate, plan: &Plan) -> Result<Listing, Error> {
    let wave = first_wave(project, krate, plan)?;
    let required = required_gates(plan, &wave);
    let either = any_gates(project, krate, plan, &wave, &required)?;
    let base_lines: HashSet<String> = wave
        .base
        .entries
        .iter()
        .map(|entry| entry.line.clone())
        .collect();
    let Wave { base, offs, .. } = wave;
    let off_lines: Vec<HashSet<&str>> = offs.iter().map(lines).collect();
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for mut entry in base.entries {
        let condition = match (required.get(&entry.line), either.get(&entry.line)) {
            (Some(gates), _) => Condition::All(gates.clone()),
            (None, Some(gates)) => Condition::any(gates.clone()),
            (None, None) => Condition::All(Vec::new()),
        };
        let variants = variants_of(&entry, &base_lines, plan, &offs, &off_lines);
        entry.line = condition.line(&entry);
        entry.gates = condition.into_gates();
        entries.push(entry);
        entries.extend(
            variants
                .into_iter()
                .filter(|variant| seen.insert(variant.line.clone())),
        );
    }
    Ok(Listing {
        entries,
        format: base.format,
        globs: base.globs,
    })
}

fn first_wave(project: &Project, krate: &Crate, plan: &Plan) -> Result<Wave, Error> {
    thread::scope(|scope| {
        let base = scope.spawn(|| Listing::build(project, krate, &plan.base));
        let offs: Vec<_> = plan
            .gates
            .iter()
            .map(|(_, features)| scope.spawn(move || Listing::build(project, krate, features)))
            .collect();
        let none = plan
            .none
            .as_ref()
            .map(|features| scope.spawn(move || Listing::build(project, krate, features)));
        Ok(Wave {
            base: joined(base)?,
            offs: offs
                .into_iter()
                .map(joined)
                .collect::<Result<Vec<_>, _>>()?,
            none: none.map(joined).transpose()?,
        })
    })
}

fn second_wave(project: &Project, krate: &Crate, plan: &Plan) -> Result<Vec<Listing>, Error> {
    thread::scope(|scope| {
        let builds: Vec<_> = plan
            .only
            .iter()
            .map(|(_, features)| scope.spawn(move || Listing::build(project, krate, features)))
            .collect();
        builds.into_iter().map(joined).collect()
    })
}

fn joined<T>(handle: ScopedJoinHandle<'_, T>) -> T {
    match handle.join() {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn lines(listing: &Listing) -> HashSet<&str> {
    listing
        .entries
        .iter()
        .map(|entry| entry.line.as_str())
        .collect()
}

fn required_gates(plan: &Plan, wave: &Wave) -> HashMap<String, Vec<String>> {
    let mut found: HashMap<String, Vec<String>> = HashMap::new();
    for ((gate, _), off) in plan.gates.iter().zip(&wave.offs) {
        let present = lines(off);
        for entry in wave
            .base
            .entries
            .iter()
            .filter(|entry| !present.contains(entry.line.as_str()))
        {
            found
                .entry(entry.line.clone())
                .or_default()
                .push(gate.clone());
        }
    }
    for gates in found.values_mut() {
        let implied: HashSet<String> = gates
            .iter()
            .filter_map(|gate| plan.implies.get(gate))
            .flatten()
            .cloned()
            .collect();
        if gates.iter().any(|gate| !implied.contains(gate)) {
            gates.retain(|gate| !implied.contains(gate));
        }
    }
    found
}

fn any_gates(
    project: &Project,
    krate: &Crate,
    plan: &Plan,
    wave: &Wave,
    required: &HashMap<String, Vec<String>>,
) -> Result<HashMap<String, Vec<String>>, Error> {
    let Some(none) = &wave.none else {
        return Ok(HashMap::new());
    };
    let without = lines(none);
    let candidates: Vec<&str> = wave
        .base
        .entries
        .iter()
        .map(|entry| entry.line.as_str())
        .filter(|line| !without.contains(*line) && !required.contains_key(*line))
        .collect();
    if candidates.is_empty() {
        return Ok(HashMap::new());
    }
    let only = second_wave(project, krate, plan)?;
    let sets: Vec<(&String, HashSet<&str>)> = plan
        .only
        .iter()
        .zip(&only)
        .map(|((gate, _), listing)| (gate, lines(listing)))
        .collect();
    let every: Vec<String> = plan.gates.iter().map(|(gate, _)| gate.clone()).collect();
    Ok(candidates
        .into_iter()
        .map(|line| {
            let on: Vec<String> = sets
                .iter()
                .filter(|(_, present)| present.contains(line))
                .map(|(gate, _)| (*gate).clone())
                .collect();
            (
                line.to_owned(),
                if on.is_empty() { every.clone() } else { on },
            )
        })
        .collect())
}

fn variants_of(
    entry: &Entry,
    base_lines: &HashSet<String>,
    plan: &Plan,
    offs: &[Listing],
    off_lines: &[HashSet<&str>],
) -> Vec<Entry> {
    if entry.kind == Kind::Impl {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (((gate, _), off), present) in plan.gates.iter().zip(offs).zip(off_lines) {
        if present.contains(entry.line.as_str()) {
            continue;
        }
        found.extend(
            off.entries
                .iter()
                .filter(|other| {
                    other.kind == entry.kind
                        && other.path == entry.path
                        && !base_lines.contains(&other.line)
                })
                .map(|other| Entry {
                    line: Condition::Not(gate.clone()).line(other),
                    id: entry.id,
                    ..other.clone()
                }),
        );
    }
    found
}
