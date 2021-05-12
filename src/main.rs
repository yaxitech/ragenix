use std::env;

use color_eyre::eyre::{eyre, Result};
use std::{ffi::OsString, fs, io::Write, path::Path, process};

mod age;
mod agenix;
mod cli;
mod util;

fn main() -> Result<()> {
    color_eyre::install()?;
    agenix(env::args(), &mut std::io::stdout(), &mut std::io::stderr())
}

/// Run the program by parsing the command line arguments and writing
/// to the passed writer
pub(crate) fn agenix<I, T>(itr: I, mut writer: impl Write, mut writer_err: impl Write) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let opts = cli::parse_args(itr);

    if opts.schema {
        write!(writer, "{}", agenix::AGENIX_JSON_SCHEMA)?;
    } else {
        if let Err(report) = agenix::validate_rules_file(&opts.rules) {
            writeln!(
                writer_err,
                "error: secrets rules are invalid: '{}'\n{}",
                &opts.rules, report
            )?;
            process::exit(1);
        }

        let rules = agenix::parse_rules(&opts.rules)?;
        if opts.verbose {
            writeln!(writer, "{:#?}", rules)?;
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
            agenix::edit(&rule, &identities, editor, &mut writer)?;
        } else if opts.rekey {
            agenix::rekey(&rules, &identities, &mut writer)?;
        }
    }

    Ok(())
}
