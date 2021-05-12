use std::env;

use color_eyre::eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;
    ragenix::agenix(env::args(), &mut std::io::stdout(), &mut std::io::stderr())
}
