mod aliases;
mod artifacts;
mod blocks;
mod config;
mod coverage;
mod diff;
mod error;
mod examples;
mod features;
mod health;
mod history;
mod homes;
mod imports;
mod init;
mod intent;
mod journeys;
mod levels;
mod lint;
mod llms;
mod markdown;
mod migrate;
mod modules;
mod origin;
mod package;
mod pages;
mod path;
mod rewrite;
mod rustdoc;
mod show;
mod source;
mod surface;
mod unify;

use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::config::Project;
use crate::error::Error;

#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cargo {
    #[command(
        version,
        about = "API reference and health checks generated from what the compiler sees, with a gate that stops drift",
        after_help = "Run `cargo truesight` in a crate to print its public API.\nRun `cargo truesight init` to set up a workspace."
    )]
    Truesight(Cli),
}

#[derive(Args)]
struct Cli {
    #[arg(
        short,
        long,
        global = true,
        value_name = "NAME",
        help = "Work on one package"
    )]
    package: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Print the public API: a summary, or every item that matches")]
    Show(show::ShowArgs),
    #[command(about = "List API changes against the spine or a git ref")]
    Diff(diff::DiffArgs),
    #[command(about = "Report API shape findings and task map errors")]
    Lint,
    #[command(about = "Write the spine, reference pages, llms.txt, and Markdown blocks")]
    Sync,
    #[command(about = "Fail when a generated file is stale or a lint denies")]
    Check,
    #[command(about = "Print the API health: surface, app tier, journeys, docs, and findings")]
    Health,
    #[command(about = "Write a starter truesight.toml for this workspace")]
    Init,
    #[command(about = "Plan one path per public item: every alias and the line that makes it")]
    Unify,
    #[command(about = "Rewrite paths that moved since a git ref to their current home")]
    Migrate(migrate::MigrateArgs),
}

pub enum Outcome {
    Clean,
    Failed,
}

fn main() -> ExitCode {
    let Cargo::Truesight(cli) = Cargo::parse();
    match run(cli) {
        Ok(Outcome::Clean) => ExitCode::SUCCESS,
        Ok(Outcome::Failed) => ExitCode::from(1),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<Outcome, Error> {
    let command = cli
        .command
        .unwrap_or_else(|| Command::Show(show::ShowArgs::default()));
    let load = || Project::load(cli.package.as_deref());
    match command {
        Command::Init => init::run(),
        Command::Show(args) => show::run(&load()?, &args),
        Command::Diff(args) => diff::run(&load()?, &args),
        Command::Lint => lint::run(&load()?),
        Command::Sync => artifacts::sync(&load()?),
        Command::Check => artifacts::check(&load()?),
        Command::Health => health::run(&load()?),
        Command::Unify => unify::run(&load()?),
        Command::Migrate(args) => migrate::run(&load()?, &args),
    }
}
