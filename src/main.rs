mod cli;
mod git;
mod models;
mod output;
mod protocol;
mod toon;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Diff { file, staged } => {
            let file_filter = file.as_ref().and_then(|f| f.to_str());
            let diff = if *staged {
                git::diff::diff_staged(&cli.repo, file_filter)?
            } else {
                git::diff::diff_unstaged(&cli.repo, file_filter)?
            };
            output::print_diff(&diff, &cli.format);
        }
        Commands::Stage { hunk_ids, from_stdin } => {
            let request = if *from_stdin {
                serde_json::from_reader(std::io::stdin())?
            } else {
                models::StageRequest {
                    hunk_ids: hunk_ids.clone().unwrap_or_default(),
                    line_selections: vec![],
                }
            };
            let result = git::stage::stage_selection(&cli.repo, &request)?;
            output::print_stage_result(&result, &cli.format);
        }
        Commands::Checkout { hunk_ids, from_stdin, staged } => {
            let request = if *from_stdin {
                serde_json::from_reader(std::io::stdin())?
            } else {
                models::CheckoutRequest { hunk_ids: hunk_ids.clone().unwrap_or_default() }
            };
            let result = if *staged {
                git::checkout::checkout_staged(&cli.repo, &request)?
            } else {
                git::checkout::checkout_unstaged(&cli.repo, &request)?
            };
            output::print_checkout_result(&result, &cli.format);
        }
        Commands::Status => {
            let status = git::status::get_status(&cli.repo)?;
            output::print_status(&status, &cli.format);
        }
        Commands::Protocol => {
            protocol::run_protocol(&cli.repo)?;
        }
    }

    Ok(())
}
