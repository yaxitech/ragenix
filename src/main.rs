use color_eyre::eyre::{eyre, Result};
use std::{env, path::PathBuf, process};

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
            let edit_path = util::canonicalize_rule_path(path)?;
            let rule = rules
                .into_iter()
                .find(|x| x.path == edit_path)
                .ok_or_else(|| eyre!("No rule for the given file {}", path))?;

            // `EDITOR`/`--editor` is mandatory if action is `--edit`
            let editor = &opts.editor.unwrap();
            ragenix::edit(&rule, &identities, editor, &mut std::io::stdout())?;
        } else if opts.rekey {
            ragenix::rekey(&rules, &identities, true, &mut std::io::stdout())?;
        } else if let Some(paths) = opts.rekey_chosen {
            let paths_normalized = paths
                .into_iter()
                .map(util::canonicalize_rule_path)
                .collect::<Result<Vec<PathBuf>>>()?;
            let chosen_rules = rules
                .into_iter()
                .filter(|x| paths_normalized.contains(&x.path))
                .collect::<Vec<ragenix::RagenixRule>>();

            ragenix::rekey(&chosen_rules, &identities, false, &mut std::io::stdout())?;
        }
    }

    Ok(())
}
