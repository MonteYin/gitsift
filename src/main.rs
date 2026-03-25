mod cli;
mod git;
mod models;
mod output;
mod protocol;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Diff { file } => {
            let diff = git::diff::diff_unstaged(&cli.repo, file.as_ref().and_then(|f| f.to_str()))?;
            output::print_diff(&diff, &cli.format);
        }
        Commands::Stage {
            hunk_ids,
            from_stdin,
        } => {
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
