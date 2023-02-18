use std::{env, io::Error};

use clap_complete::{
    generate_to,
    Shell::{Bash, Fish, Zsh},
};

include!("src/cli.rs");

fn main() -> Result<(), Error> {
    // Read path to the `nix` binary from the environment
    let nix_bin_path = match env::var("RAGENIX_NIX_BIN_PATH") {
        Err(_) => {
            println!(
                "cargo:warning=Environment variable RAGENIX_NIX_BIN_PATH not given, using 'nix'"
            );
            "nix".to_string()
        }
        Ok(val) => val,
    };
    println!("cargo:rustc-env=RAGENIX_NIX_BIN_PATH={nix_bin_path}");

    // Make the paths to the shell completion files available as environment variables
    let Some(outdir) = env::var_os("OUT_DIR") else { return Ok(()) };

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
