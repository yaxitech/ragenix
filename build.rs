use std::env;

use clap_generate::{
    generators::{Bash, Fish, Zsh},
    Generator,
};

include!("src/cli.rs");

fn generate_completion<G: Generator>(app: &mut App, outdir: &OsString) {
    clap_generate::generate_to::<G, _, _>(app, crate_name!(), outdir).unwrap();
}

fn main() {
    let outdir = match env::var_os("OUT_DIR") {
        None => return,
        Some(outdir) => outdir,
    };

    let mut app = build();
    app.set_bin_name(crate_name!());

    generate_completion::<Bash>(&mut app, &outdir);
    generate_completion::<Fish>(&mut app, &outdir);
    generate_completion::<Zsh>(&mut app, &outdir);
}
