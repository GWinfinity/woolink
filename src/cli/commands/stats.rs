//! Stats command - Show index statistics

use std::path::PathBuf;

use clap::Args;

/// Show index statistics
#[derive(Args, Clone)]
pub struct Command {
    /// Index file
    #[arg(short, long, default_value = "woolink.idx")]
    pub index: PathBuf,
    
    /// Detailed statistics
    #[arg(short, long)]
    pub detailed: bool,
}

pub async fn run(cmd: Command) -> anyhow::Result<()> {
    println!("📊 woolink stats");
    println!("   Index: {:?}", cmd.index);
    
    if !cmd.index.exists() {
        println!("\n   Error: Index not found");
        println!("   Run 'woolink index' first.");
        return Ok(());
    }
    
    // Load and display stats
    match crate::symbol::MmapIndex::open(&cmd.index) {
        Ok(index) => {
            println!("\n📦 Symbol Index Statistics");
            println!("───────────────────────────");
            println!("  Total symbols: {}", index.symbol_count());
            println!("  Total packages: {}", index.package_count());
            
            if cmd.detailed {
                println!("\n📁 Packages:");
                for i in 0..index.package_count().min(20) {
                    if let Some(pkg) = index.get_package(crate::symbol::PackageId::new(i as u32)) {
                        println!("  • {} (ID: {})", pkg.path(), pkg.id());
                    }
                }
                
                if index.package_count() > 20 {
                    println!("  ... and {} more", index.package_count() - 20);
                }
            }
        }
        Err(e) => {
            println!("   Error: {}", e);
        }
    }
    
    Ok(())
}
