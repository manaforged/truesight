use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet};

use public_api::rustdoc_types::Id;

use crate::surface::{Kind, Surface};

pub struct Named<'a> {
    pub id: Id,
    pub kind: Kind,
    pub paths: Vec<&'a str>,
    definition: Option<&'a str>,
}

type Rank<'a> = (bool, bool, usize, Reverse<usize>, Vec<&'a str>);

fn rank<'a>(path: &'a str, definition: Option<&str>, prelude: bool) -> Rank<'a> {
    let segments: Vec<&str> = path.split("::").collect();
    let shared = definition.map_or(0, |definition| {
        definition
            .split("::")
            .zip(segments.iter().copied())
            .take_while(|(left, right)| left == right)
            .count()
    });
    (
        prelude,
        definition != Some(path),
        segments.len(),
        Reverse(shared),
        segments,
    )
}

impl Surface {
    pub fn in_prelude(&self, path: &str) -> bool {
        self.prelude.as_deref().is_some_and(|prelude| {
            path.strip_prefix(prelude)
                .is_some_and(|rest| rest.starts_with("::"))
        })
    }

    pub fn named(&self) -> Vec<Named<'_>> {
        let mut found = self.gather();
        for named in &mut found {
            let definition = named.definition;
            named
                .paths
                .sort_by_cached_key(|&path| rank(path, definition, self.in_prelude(path)));
        }
        found
    }

    pub fn duplicates(&self) -> Vec<Named<'_>> {
        let found: Vec<Named<'_>> = self
            .named()
            .into_iter()
            .filter(|named| named.paths.len() > 1)
            .collect();
        let every: HashSet<&str> = found
            .iter()
            .flat_map(|named| named.paths.iter().copied())
            .collect();
        found
            .into_iter()
            .filter(|named| {
                !named.paths.iter().all(|path| {
                    path.rsplit_once("::")
                        .is_some_and(|(parent, _)| every.contains(parent))
                })
            })
            .collect()
    }

    pub fn aliases(&self) -> Vec<BTreeSet<&str>> {
        let mut groups: Vec<BTreeSet<&str>> = self
            .gather()
            .into_iter()
            .filter(|named| named.paths.len() > 1)
            .map(|named| named.paths.into_iter().collect())
            .collect();
        groups.sort();
        groups
    }

    fn gather(&self) -> Vec<Named<'_>> {
        let mut found: Vec<Named<'_>> = Vec::new();
        let mut slots: HashMap<Id, usize> = HashMap::new();
        for entry in self.entries.iter().filter(|entry| {
            !matches!(entry.kind, Kind::Impl | Kind::Use)
                && !entry.trait_impl
                && self.rooted(&entry.path)
        }) {
            let slot = *slots.entry(entry.id).or_insert_with(|| {
                found.push(Named {
                    id: entry.id,
                    kind: entry.kind,
                    paths: Vec::new(),
                    definition: entry.definition.as_deref(),
                });
                found.len() - 1
            });
            if let Some(named) = found.get_mut(slot)
                && !named.paths.contains(&entry.path.as_str())
            {
                named.paths.push(entry.path.as_str());
            }
        }
        found
    }

    pub fn resolve(&self, path: &str) -> Option<&str> {
        let listed = |candidate: &str| {
            self.entries
                .iter()
                .find(|entry| entry.path == candidate)
                .map(|entry| entry.path.as_str())
        };
        if let Some(found) = listed(path) {
            return Some(found);
        }
        let aliases = self.aliases();
        let mut end = path.len();
        while let Some(split) = path[..end].rfind("::") {
            let (parent, rest) = path.split_at(split);
            for group in aliases.iter().filter(|group| group.contains(parent)) {
                if let Some(found) = group
                    .iter()
                    .find_map(|alias| listed(&format!("{alias}{rest}")))
                {
                    return Some(found);
                }
            }
            end = split;
        }
        None
    }
}
