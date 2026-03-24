//! CLI commands

use clap::Args;

// Temporarily stubbed out due to type issues
// mod index;
// mod query;
// mod stats;

/// Index command placeholder
#[derive(Args, Clone)]
pub struct IndexCommand {
    /// Input directory
    #[arg(default_value = ".")]
    pub path: std::path::PathBuf,
}

/// Query command placeholder  
#[derive(Args, Clone)]
pub struct QueryCommand {
    /// Query string
    pub pattern: String,
}

/// Stats command placeholder
#[derive(Args, Clone)]
pub struct StatsCommand {
    /// Index path
    #[arg(default_value = "index.wl")]
    pub index: std::path::PathBuf,
}

pub async fn index(_cmd: IndexCommand) -> anyhow::Result<()> {
    println!("Index command not yet implemented");
    Ok(())
}

pub async fn query(_cmd: QueryCommand) -> anyhow::Result<()> {
    println!("Query command not yet implemented");
    Ok(())
}

pub async fn stats(_cmd: StatsCommand) -> anyhow::Result<()> {
    println!("Stats command not yet implemented");
    Ok(())
}

/// Common options for all commands
#[derive(Args, Clone)]
pub struct CommonOptions {
    /// Path to project
    #[arg(short, long, default_value = ".")]
    pub path: std::path::PathBuf,

    /// Cache directory
    #[arg(long)]
    pub cache_dir: Option<std::path::PathBuf>,
}
