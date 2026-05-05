mod activity;
mod alphaxiv;
mod cli;
mod config;
mod date_range;
mod helper;
mod import_plan;
mod index;
mod lfz;
mod mirror;
mod mutation;
mod output;
mod paths;
mod setup;
mod skill;
mod zotero;

use anyhow::Result;

pub use cli::Cli;

pub fn run(cli: Cli) -> Result<()> {
    let context = cli.to_context()?;
    let value = cli::dispatch(&cli, &context)?;
    output::print_value(&value, cli.format)
}
