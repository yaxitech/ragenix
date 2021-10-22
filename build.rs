use std::{env, io::Error};

use clap_generate::{
    generate_to,
    generators::{Bash, Fish, Zsh},
};

include!("src/cli.rs");

fn main() -> Result<(), Error> {
    let outdir = match env::var_os("OUT_DIR") {
        None => return Ok(()),
        Some(outdir) => outdir,
    };

    let mut app = build();
    app.set_bin_name(crate_name!());

    let bin_name = crate_name!();
    generate_to(Bash, &mut app, bin_name, &outdir)?;
    generate_to(Fish, &mut app, bin_name, &outdir)?;
    generate_to(Zsh, &mut app, bin_name, &outdir)?;

    Ok(())
}
