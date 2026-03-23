//! woolink - Global Symbol Table CLI

use anyhow::Result;

mod cli;

#[tokio::main]
async fn main() -> Result<()> {
    cli::run().await
}
