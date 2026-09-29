use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::process::Command;

use public_api::PublicApi;
use public_api::rustdoc_types::Crate as Rustdoc;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::config::{Crate, DEFAULT_TOOLCHAIN, Project};
use crate::error::Error;

const FORMATS: RangeInclusive<u32> = 59..=61;
const OUTPUT_LINES: usize = 40;

pub struct Build {
    pub rustdoc: Rustdoc,
    pub api: PublicApi,
}

#[derive(Deserialize)]
struct Header {
    format_version: u32,
}

pub fn ensure_toolchain(toolchain: &str) -> Result<(), Error> {
    if installed(toolchain)? {
        return Ok(());
    }
    let _install = install_lock(toolchain);
    if installed(toolchain)? {
        return Ok(());
    }
    eprintln!("truesight: installing {toolchain} to build rustdoc JSON");
    let status = Command::new("rustup")
        .args(["toolchain", "install", toolchain, "--profile", "minimal"])
        .status()
        .map_err(|source| Error::Io {
            path: PathBuf::from("rustup"),
            source,
        })?;
    if status.success() && installed(toolchain)? {
        Ok(())
    } else {
        Err(Error::Toolchain(toolchain.to_owned()))
    }
}

fn install_lock(toolchain: &str) -> Option<File> {
    let path = std::env::temp_dir().join(format!("truesight-{toolchain}.lock"));
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    file.lock().ok()?;
    Some(file)
}

fn installed(toolchain: &str) -> Result<bool, Error> {
    let output = Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .map_err(|source| Error::Io {
            path: PathBuf::from("rustup"),
            source,
        })?;
    let prefix = format!("{toolchain}-");
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .any(|name| name == toolchain || name.starts_with(&prefix)))
}

pub fn build(
    project: &Project,
    krate: &Crate,
    features: &[String],
    private: bool,
) -> Result<Build, Error> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let path = rustdoc_json::Builder::default()
        .toolchain(project.toolchain.as_str())
        .manifest_path(&krate.manifest)
        .target_dir(
            project
                .target_dir
                .join(build_key(&project.root, features, private)),
        )
        .no_default_features(true)
        .features(features)
        .document_private_items(private)
        .quiet(true)
        .color(rustdoc_json::Color::Never)
        .build_with_captured_output(&mut stdout, &mut stderr)
        .map_err(|error| Error::Rustdoc {
            package: krate.package.clone(),
            output: format!("{error}\n{}", tail(&stderr)),
        })?;
    let text = std::fs::read_to_string(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    let found = parse::<Header>(&path, &text)?.format_version;
    if !FORMATS.contains(&found) {
        return Err(Error::Format {
            path,
            found,
            min: *FORMATS.start(),
            max: *FORMATS.end(),
            toolchain: DEFAULT_TOOLCHAIN,
        });
    }
    let rustdoc: Rustdoc = parse(&path, &text)?;
    let api = public_api::Builder::from_rustdoc_json(&path)
        .omit_blanket_impls(true)
        .omit_auto_trait_impls(true)
        .omit_auto_derived_impls(false)
        .include_function_parameter_names(true)
        .build()
        .map_err(|source| Error::PublicApi { path, source })?;
    Ok(Build { rustdoc, api })
}

fn build_key(root: &Path, features: &[String], private: bool) -> String {
    let mut hasher = DefaultHasher::new();
    (root, features, private).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn parse<T: DeserializeOwned>(path: &Path, text: &str) -> Result<T, Error> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    deserializer.disable_recursion_limit();
    T::deserialize(&mut deserializer).map_err(|source| Error::Json {
        path: path.to_owned(),
        source,
    })
}

fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(OUTPUT_LINES);
    lines.get(start..).unwrap_or_default().join("\n")
}
