use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gitsift", version, about = "Git hunk sifter for code agents")]
#[command(propagate_version = true)]
pub struct Cli {
    /// Output format (toon = compact token-efficient, json = full structured)
    #[arg(long, value_enum, global = true, default_value_t = OutputFormat::Toon)]
    pub format: OutputFormat,

    /// Path to git repository
    #[arg(long, global = true, default_value = ".")]
    pub repo: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List unstaged hunks (or staged hunks with --staged)
    Diff {
        /// Filter by file path
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// Show staged changes (HEAD vs index) instead of unstaged
        #[arg(long)]
        staged: bool,
    },
    /// Stage selected hunks or lines
    Stage {
        /// Hunk IDs to stage (comma-separated)
        #[arg(long, value_delimiter = ',')]
        hunk_ids: Option<Vec<String>>,

        /// Read JSON selection from stdin
        #[arg(long)]
        from_stdin: bool,
    },
    /// Discard selected hunks (unstaged by default, or staged with --staged)
    Checkout {
        /// Hunk IDs to discard (comma-separated)
        #[arg(long, value_delimiter = ',')]
        hunk_ids: Option<Vec<String>>,

        /// Read JSON selection from stdin
        #[arg(long)]
        from_stdin: bool,

        /// Discard staged changes (index → HEAD) instead of unstaged (workdir → index)
        #[arg(long)]
        staged: bool,
    },
    /// Show staging status summary
    Status,
    /// Enter stdin/stdout JSON-lines protocol mode
    Protocol,
}

#[derive(Clone, Debug, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Toon,
    Json,
}
