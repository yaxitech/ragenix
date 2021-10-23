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

    let bash_outpath = generate_to(Bash, &mut app, crate_name!(), &outdir)?;
    println!(
        "cargo:rustc-env=RAGENIX_COMPLETIONS_BASH={}",
        bash_outpath.to_string_lossy()
    );

    let fish_outpath = generate_to(Fish, &mut app, crate_name!(), &outdir)?;
    println!(
        "cargo:rustc-env=RAGENIX_COMPLETIONS_FISH={}",
        fish_outpath.to_string_lossy()
    );

    let zsh_outpath = generate_to(Zsh, &mut app, crate_name!(), &outdir)?;
    println!(
        "cargo:rustc-env=RAGENIX_COMPLETIONS_ZSH={}",
        zsh_outpath.to_string_lossy()
    );

    Ok(())
}
