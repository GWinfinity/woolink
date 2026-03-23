//! CLI module for woolink

use clap::{Parser, Subcommand};

mod commands;

use commands::{IndexCommand, QueryCommand, StatsCommand};

/// woolink - Global Symbol Table for Go
#[derive(Parser)]
#[command(name = "woolink")]
#[command(about = "跨包符号解析系统 - Woo 生态链组件")]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    
    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Build symbol index from Go modules
    #[command(name = "index")]
    Index(IndexCommand),
    
    /// Query symbols
    #[command(name = "query")]
    Query(QueryCommand),
    
    /// Show statistics
    #[command(name = "stats")]
    Stats(StatsCommand),
}

pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    // Initialize logging
    if cli.verbose {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    }
    
    match cli.command {
        Commands::Index(cmd) => commands::index(cmd).await,
        Commands::Query(cmd) => commands::query(cmd).await,
        Commands::Stats(cmd) => commands::stats(cmd).await,
    }
}
