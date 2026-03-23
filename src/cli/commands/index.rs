//! Index command - Build symbol index from Go modules

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use clap::Args;

use crate::bridge::{SymbolImporter, ImportConfig};

/// Build symbol index
#[derive(Args, Clone)]
pub struct Command {
    /// Path to Go project
    #[arg(default_value = ".")]
    pub path: PathBuf,
    
    /// Output file for index
    #[arg(short, long, default_value = "woolink.idx")]
    pub output: PathBuf,
    
    /// Include private symbols
    #[arg(long)]
    pub include_private: bool,
    
    /// Number of threads
    #[arg(short, long)]
    pub threads: Option<usize>,
}

pub async fn run(cmd: Command) -> anyhow::Result<()> {
    println!("🔍 woolink index");
    println!("   Path: {:?}", cmd.path);
    println!("   Output: {:?}", cmd.output);
    
    let start = Instant::now();
    
    // Setup rayon thread pool
    if let Some(n) = cmd.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .ok();
    }
    
    // Import from woofind
    let config = ImportConfig {
        include_private: cmd.include_private,
        include_docs: true,
        include_locations: true,
        batch_size: 10000,
        progress_interval: 1000,
    };
    
    let mut importer = SymbolImporter::new(config);
    
    // Try to load from woofind if available
    match woofind::index::IndexBuilder::new() {
        Ok(builder) => {
            println!("   Loading from woofind...");
            
            // Build index
            builder.build_from_directory(&cmd.path)?;
            
            // Import to woolink
            let universe = importer.import_from_woofind(&*builder.index())?;
            
            let stats = universe.read().stats();
            println!("\n✅ Index built:");
            println!("   Symbols: {}", stats.total_symbols);
            println!("   Packages: {}", stats.total_packages);
            println!("   Memory: {} MB", stats.memory_usage_bytes / 1024 / 1024);
        }
        Err(e) => {
            println!("   Note: woofind not available ({})", e);
            println!("   Creating empty index...");
        }
    }
    
    println!("\n⏱️  Completed in {:?}", start.elapsed());
    
    Ok(())
}
