mod app_process;
mod bench;
mod case;
mod check;
mod compare;
mod mock;
mod normalize;
mod record;
mod runner;
mod webhook;

use anyhow::Result;
use app_process::AppProgram;
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

const LOG_DIRECTORY: &str = "target/replay/logs";

#[derive(Parser)]
#[command(
    name = "replay",
    about = "Replay recorded webhook cases against mergeable behind a mock GitHub API"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct CaseSelection {
    #[arg(long = "case", value_name = "NAME")]
    names: Vec<String>,
    #[arg(long, default_value = "app-cases/cases")]
    cases_dir: PathBuf,
}

#[derive(Args)]
struct RunSettings {
    #[arg(long, default_value_t = default_jobs())]
    jobs: usize,
    #[arg(long, default_value_t = 4000)]
    quiet_ms: u64,
    #[arg(long)]
    reuse_process: bool,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |cpus| cpus.get() / 2)
        .max(1)
}

impl RunSettings {
    fn run_options(&self) -> runner::RunOptions {
        runner::RunOptions {
            quiet_period: Duration::from_millis(self.quiet_ms),
            jobs: self.jobs,
            reuse_process: self.reuse_process,
            log_directory: PathBuf::from(LOG_DIRECTORY),
        }
    }
}

#[derive(Subcommand)]
enum Command {
    Record {
        #[command(flatten)]
        selection: CaseSelection,
        #[command(flatten)]
        run_settings: RunSettings,
        #[arg(long)]
        missing_only: bool,
        #[arg(long, required = true, num_args = 1.., allow_hyphen_values = true, value_name = "COMMAND")]
        reference_command: Vec<String>,
    },
    Check {
        #[arg(long)]
        bin: PathBuf,
        #[command(flatten)]
        selection: CaseSelection,
        #[command(flatten)]
        run_settings: RunSettings,
        #[arg(long)]
        json: Option<PathBuf>,
    },
    Bench {
        #[arg(
            long,
            required_unless_present = "reference_command",
            conflicts_with = "reference_command"
        )]
        bin: Option<PathBuf>,
        #[arg(long, num_args = 1.., allow_hyphen_values = true, value_name = "COMMAND")]
        reference_command: Vec<String>,
        #[command(flatten)]
        selection: CaseSelection,
        #[arg(long, default_value_t = 500)]
        requests: usize,
        #[arg(long, default_value_t = 16)]
        concurrency: usize,
        #[arg(long)]
        json: Option<PathBuf>,
    },
}

fn write_json_report(path: &Path, report: &impl Serialize) -> Result<()> {
    let mut text = serde_json::to_string_pretty(report)?;
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Record {
            selection,
            run_settings,
            missing_only,
            reference_command,
        } => {
            let mut cases = case::load_cases(&selection.cases_dir, &selection.names)?;
            if missing_only {
                cases.retain(|case| case.definition.expected.is_none());
            }
            let program = AppProgram::reference(reference_command)?;
            record::record(program, cases, &run_settings.run_options()).await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Check {
            bin,
            selection,
            run_settings,
            json,
        } => {
            let cases = case::load_cases(&selection.cases_dir, &selection.names)?;
            let report = check::check(&bin, cases, &run_settings.run_options()).await?;
            if let Some(json_path) = json {
                write_json_report(&json_path, &report)?;
            }
            Ok(if report.failed > 0 {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        Command::Bench {
            bin,
            reference_command,
            selection,
            requests,
            concurrency,
            json,
        } => {
            let cases = case::load_cases(&selection.cases_dir, &selection.names)?;
            let program = match bin {
                Some(binary) => AppProgram::binary(&binary)?,
                None => AppProgram::reference(reference_command)?,
            };
            let settings = bench::BenchSettings {
                requests,
                concurrency,
            };
            let report = bench::bench(
                &program,
                &cases,
                &settings,
                &Path::new(LOG_DIRECTORY).join("bench.log"),
            )
            .await?;
            bench::print_bench_table(&report);
            if let Some(json_path) = json {
                write_json_report(&json_path, &report)?;
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
