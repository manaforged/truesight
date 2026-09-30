use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cargo_metadata::{Metadata, Package, TargetKind};
use serde::Deserialize;

use crate::config::{VERSION_SLOT, normal};
use crate::error::Error;
use crate::intent;
use crate::journeys::JourneyConfig;
use crate::levels::{LintLevels, LintOverrides};

const DEFAULT_TAG: &str = "v{version}";

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CrateEntry {
    pub package: String,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default = "default_features")]
    default_features: bool,
    #[serde(default)]
    gates: Vec<String>,
    spine: Option<PathBuf>,
    intent: Option<PathBuf>,
    tag: Option<String>,
    budget: Option<usize>,
    prelude: Option<String>,
    prelude_budget: Option<usize>,
    #[serde(default)]
    docs: Vec<PathBuf>,
    #[serde(default)]
    doc_sections: BTreeMap<String, String>,
    journey_prefix: Option<String>,
    #[serde(default)]
    journeys: BTreeMap<String, usize>,
    #[serde(default)]
    journey_ignore: Vec<String>,
    #[serde(default)]
    lint: LintOverrides,
}

fn default_features() -> bool {
    true
}

impl CrateEntry {
    pub fn named(package: String) -> Self {
        Self {
            package,
            features: Vec::new(),
            default_features: true,
            gates: Vec::new(),
            spine: None,
            intent: None,
            tag: None,
            budget: None,
            prelude: None,
            prelude_budget: None,
            docs: Vec::new(),
            doc_sections: BTreeMap::new(),
            journey_prefix: None,
            journeys: BTreeMap::new(),
            journey_ignore: Vec::new(),
            lint: LintOverrides::default(),
        }
    }
}

pub struct Example {
    pub name: String,
    pub source: PathBuf,
    pub features: Vec<String>,
}

pub struct Crate {
    pub package: String,
    pub name: String,
    pub version: String,
    pub manifest: PathBuf,
    pub feature_table: BTreeMap<String, Vec<String>>,
    pub features: Vec<String>,
    pub default_features: bool,
    pub gates: Vec<String>,
    pub spine: PathBuf,
    pub intent: PathBuf,
    pub tag: String,
    pub readme: Option<PathBuf>,
    pub examples: Vec<Example>,
    pub budget: Option<usize>,
    pub prelude: Option<String>,
    pub prelude_budget: Option<usize>,
    pub docs: Vec<PathBuf>,
    pub doc_sections: BTreeMap<String, String>,
    pub journeys: JourneyConfig,
    pub lint: LintLevels,
    pub library: PathBuf,
}

pub fn resolve(
    metadata: &Metadata,
    root: &Path,
    entry: CrateEntry,
    lint: LintLevels,
) -> Result<Crate, Error> {
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == entry.package)
        .ok_or_else(|| Error::NotInWorkspace(entry.package.clone()))?;
    let library = package
        .targets
        .iter()
        .find(|target| target.is_kind(TargetKind::Lib) || target.is_kind(TargetKind::RLib))
        .ok_or_else(|| Error::NoLibrary(entry.package.clone()))?;
    let tag = entry.tag.unwrap_or_else(|| DEFAULT_TAG.to_owned());
    if !tag.contains(VERSION_SLOT) {
        return Err(Error::TagPattern {
            package: entry.package,
            pattern: tag,
        });
    }
    let spine = located(root, entry.spine, &entry.package, "txt");
    let intent = located(root, entry.intent, &entry.package, "toml");
    let name = library.name.replace('-', "_");
    Ok(Crate {
        prelude: entry.prelude.map(|path| intent::qualify(&name, path)),
        library: library.src_path.clone().into_std_path_buf(),
        name,
        version: package.version.to_string(),
        manifest: package.manifest_path.clone().into_std_path_buf(),
        feature_table: package.features.clone(),
        readme: readme_of(package),
        examples: examples_of(package),
        features: entry.features,
        default_features: entry.default_features,
        gates: entry.gates,
        spine,
        intent,
        tag,
        budget: entry.budget,
        prelude_budget: entry.prelude_budget,
        docs: entry
            .docs
            .into_iter()
            .map(|path| normal(&root.join(path)))
            .collect(),
        doc_sections: entry.doc_sections,
        lint: lint.with(entry.lint),
        journeys: JourneyConfig {
            prefix: entry.journey_prefix,
            budgets: entry.journeys,
            ignore: entry.journey_ignore,
        },
        package: entry.package,
    })
}

pub fn spine_of(metadata: &Metadata, root: &Path, entry: &CrateEntry) -> Option<(String, PathBuf)> {
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == entry.package)?;
    let library = package
        .targets
        .iter()
        .find(|target| target.is_kind(TargetKind::Lib) || target.is_kind(TargetKind::RLib))?;
    Some((
        library.name.replace('-', "_"),
        located(root, entry.spine.clone(), &entry.package, "txt"),
    ))
}

fn located(root: &Path, configured: Option<PathBuf>, package: &str, extension: &str) -> PathBuf {
    let path =
        configured.unwrap_or_else(|| PathBuf::from("api").join(format!("{package}.{extension}")));
    normal(&root.join(path))
}

pub fn readme_of(package: &Package) -> Option<PathBuf> {
    let dir = package.manifest_path.parent()?.as_std_path();
    let name = package.readme.as_ref().map_or_else(
        || PathBuf::from("README.md"),
        |path| path.clone().into_std_path_buf(),
    );
    let path = dir.join(name);
    path.is_file().then_some(path)
}

fn examples_of(package: &Package) -> Vec<Example> {
    let mut examples: Vec<Example> = package
        .targets
        .iter()
        .filter(|target| target.is_kind(TargetKind::Example))
        .map(|target| Example {
            name: target.name.clone(),
            source: target.src_path.clone().into_std_path_buf(),
            features: target.required_features.clone(),
        })
        .collect();
    examples.sort_by(|left, right| left.name.cmp(&right.name));
    examples
}
