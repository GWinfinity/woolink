//! Query command - Search symbols

use std::path::PathBuf;

use clap::Args;

/// Query symbols
#[derive(Args, Clone)]
pub struct Command {
    /// Symbol name to search
    pub name: String,
    
    /// Package path (optional)
    #[arg(short, long)]
    pub package: Option<String>,
    
    /// Query from index file
    #[arg(short, long, default_value = "woolink.idx")]
    pub index: PathBuf,
    
    /// Show definition location
    #[arg(long)]
    pub location: bool,
    
    /// Show documentation
    #[arg(long)]
    pub doc: bool,
    
    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    pub format: OutputFormat,
}

#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

pub async fn run(cmd: Command) -> anyhow::Result<()> {
    println!("🔍 Query: {}", cmd.name);
    
    if let Some(pkg) = &cmd.package {
        println!("   Package: {}", pkg);
    }
    
    // Try to load index
    if !cmd.index.exists() {
        println!("   Error: Index not found at {:?}", cmd.index);
        println!("   Run 'woolink index' first.");
        return Ok(());
    }
    
    // Load mmap index
    match crate::symbol::MmapIndex::open(&cmd.index) {
        Ok(index) => {
            println!("   Loaded index: {} symbols", index.symbol_count());
            
            // Search for symbol
            let view = index.as_view();
            let mut found = 0;
            
            for sym in view.iter() {
                let name = sym.name();
                if name == cmd.name {
                    found += 1;
                    
                    match cmd.format {
                        OutputFormat::Text => {
                            println!("\n  📌 {}", name);
                            println!("     Package ID: {}", sym.package_id());
                            
                            if cmd.location {
                                let loc = sym.definition();
                                println!("     Location: file:{}, offset:{}", 
                                    loc.file_id, loc.offset);
                            }
                            
                            if cmd.doc {
                                if let Some(doc) = sym.doc() {
                                    println!("     Doc: {}", doc.lines().next().unwrap_or(""));
                                }
                            }
                            
                            if let Some(sig) = sym.signature() {
                                println!("     Signature: {}", sig);
                            }
                        }
                        OutputFormat::Json => {
                            // JSON output would go here
                        }
                    }
                }
            }
            
            if found == 0 {
                println!("\n  ❌ No symbols found");
            } else {
                println!("\n  ✅ Found {} symbol(s)", found);
            }
        }
        Err(e) => {
            println!("   Error loading index: {}", e);
        }
    }
    
    Ok(())
}
