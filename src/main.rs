use std::env;

use color_eyre::eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;
    ragenix::run(env::args(), &mut std::io::stdout())
}
