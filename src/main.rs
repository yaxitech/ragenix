use color_eyre::eyre::{eyre, Result};
use std::{env, fs, path::Path, process};

mod age;
mod cli;
mod ragenix;
mod util;

fn main() -> Result<()> {
    color_eyre::install()?;
    let opts = cli::parse_args(env::args());

    if opts.schema {
        print!("{}", ragenix::AGENIX_JSON_SCHEMA_STRING);
    } else {
        if let Err(report) = ragenix::validate_rules_file(&opts.rules) {
            eprintln!(
                "error: secrets rules are invalid: '{}'\n{report}",
                &opts.rules
            );
            process::exit(1);
        }

        let rules = ragenix::parse_rules(&opts.rules)?;
        if opts.verbose {
            println!("{rules:#?}");
        }

        let identities = opts.identities.unwrap_or_default();

        if let Some(path) = &opts.edit {
            let path_normalized = util::normalize_path(Path::new(path));
            let edit_path = std::env::current_dir()
                .and_then(fs::canonicalize)
                .map(|p| p.join(path_normalized))?;
            let rule = rules
                .into_iter()
                .find(|x| x.path == edit_path)
                .ok_or_else(|| eyre!("No rule for the given file {}", path))?;

            // `EDITOR`/`--editor` is mandatory if action is `--edit`
            let editor = &opts.editor.unwrap();
            ragenix::edit(&rule, &identities, editor, &mut std::io::stdout())?;
        } else if opts.rekey {
            ragenix::rekey(&rules, &identities, &mut std::io::stdout())?;
        }
    }

    Ok(())
}
