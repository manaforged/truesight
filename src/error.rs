use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{shown}: {source}", shown = .path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{shown}: {source}", shown = .path.display())]
    Toml {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error("cargo metadata: {0}")]
    Metadata(#[from] cargo_metadata::Error),
    #[error(
        "no package here: run inside a crate, or write a truesight.toml with `cargo truesight init`"
    )]
    NoPackage,
    #[error("package `{0}` is not a member of this workspace")]
    NotInWorkspace(String),
    #[error("package `{0}` has no library target")]
    NoLibrary(String),
    #[error("package `{name}` is not in truesight.toml, which lists {known}")]
    UnknownPackage { name: String, known: String },
    #[error("package `{package}` has no feature `{feature}`")]
    UnknownFeature { package: String, feature: String },
    #[error("gate `{gate}` is not enabled by the documented features of `{package}`")]
    GateNotEnabled { package: String, gate: String },
    #[error(
        "rustup could not install toolchain `{0}`; run `rustup toolchain install {0} --profile minimal`"
    )]
    Toolchain(String),
    #[error("rustdoc JSON for `{package}` did not build\n{output}")]
    Rustdoc { package: String, output: String },
    #[error(
        "{shown}: rustdoc JSON format {found} is not supported; truesight reads formats {min} to {max}, which `{toolchain}` writes",
        shown = .path.display()
    )]
    Format {
        path: PathBuf,
        found: u32,
        min: u32,
        max: u32,
        toolchain: &'static str,
    },
    #[error("{shown}: {source}", shown = .path.display())]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{shown}: {source}", shown = .path.display())]
    PublicApi {
        path: PathBuf,
        source: public_api::Error,
    },
    #[error("git {args}: {stderr}")]
    Git { args: String, stderr: String },
    #[error("{shown}: {message}", shown = .path.display())]
    Markdown { path: PathBuf, message: String },
    #[error("{shown} does not exist yet; run `cargo truesight sync`", shown = .0.display())]
    NoSpine(PathBuf),
    #[error("{shown} is not committed at `{reference}`", shown = .path.display())]
    NoSpineAt { reference: String, path: PathBuf },
    #[error("tag pattern `{pattern}` of `{package}` has no `{{version}}`")]
    TagPattern { package: String, pattern: String },
    #[error("{shown} already exists", shown = .0.display())]
    ConfigExists(PathBuf),
    #[error(
        "{shown} was not written by truesight, and `sync` deletes such files; move it out of the reference directory, then run `cargo truesight init` again",
        shown = .0.display()
    )]
    PagesExist(PathBuf),
    #[error(
        "book directory `{shown}` has no SUMMARY.md; set `book` in truesight.toml to the mdBook `src` directory",
        shown = .0.display()
    )]
    NoSummary(PathBuf),
}
