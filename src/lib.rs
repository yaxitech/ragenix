mod age;
mod cli;
mod util;

use color_eyre::{
    eyre::{eyre, Result, WrapErr},
    Help, SectionExt,
};
use jsonschema::JSONSchema;
use std::{
    ffi::OsString,
    fs,
    io::Write,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
    process,
};

static AGENIX_JSON_SCHEMA: &str = std::include_str!("agenix.schema.json");

#[derive(Debug)]
struct RagenixRule {
    pub path: PathBuf,
    pub public_keys: Vec<String>,
}

/// Validate conformance of the passed path to the JSON schema [`AGENIX_JSON_SCHEMA`].
fn validate_rules_file<P: AsRef<Path>>(path: P) -> Result<()> {
    if !path.as_ref().exists() {
        return Err(eyre!("{} does not exist!", path.as_ref().to_string_lossy()));
    }

    let instance = nix_rules_to_json(&path)?;
    let schema =
        serde_json::from_str(AGENIX_JSON_SCHEMA).wrap_err("Failed to parse Agenix JSON schema")?;
    let compiled = JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft7)
        .compile(&schema)?;
    let result = compiled.validate(&instance);

    if let Err(errors) = result {
        let error_msg = errors
            .into_iter()
            .map(|err| format!(" - {}: {}", err.instance_path, err))
            .collect::<Vec<String>>()
            .join("\n");
        Err(eyre!(error_msg))
    } else {
        Ok(())
    }
}

/// Parse the given rules file path.
///
/// This method assumes that the passed file adheres to the [`AGENIX_JSON_SCHEMA`].
fn parse_rules<P: AsRef<Path>>(rules_path: P) -> Result<Vec<RagenixRule>> {
    let instance = nix_rules_to_json(&rules_path)?;

    // It's fine to force unwrap here as we validated the JSON schema
    let mut rules: Vec<RagenixRule> = Vec::new();
    for (rel_path, val) in instance.as_object().unwrap().iter() {
        let dir = fs::canonicalize(rules_path.as_ref().parent().unwrap())?;
        let p = dir.join(&rel_path);
        let public_keys = val.as_object().unwrap()["publicKeys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        let rule = RagenixRule {
            path: p,
            public_keys,
        };
        rules.push(rule);
    }

    Ok(rules)
}

/// Rekey all entries with the specified public keys
fn rekey(entries: &[RagenixRule], identities: &[String], mut writer: impl Write) -> Result<()> {
    let identities = age::get_identities(identities)?;
    for entry in entries {
        if entry.path.exists() {
            writeln!(writer, "Rekeying {}", entry.path.display())?;
            age::rekey(&entry.path, &identities, &entry.public_keys)?;
        } else {
            writeln!(writer, "Does not exist, ignored: {}", entry.path.display())?;
        }
    }
    Ok(())
}

/// Edit/create an age-encrypted file
///
/// If the file doesn't exist yet, a new file is created and opened in `editor`.
fn edit(
    entry: &RagenixRule,
    identity_paths: &[String],
    editor: &str,
    mut writer: impl Write,
) -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::set_permissions(&dir, PermissionsExt::from_mode(0o700))?;

    let input_path = dir.path().join("input");
    let output_path = &entry.path;

    if output_path.exists() {
        // If the file already exists, first decrypt it, hash it, open it in `editor`,
        // hash the result, and if the hashes are equal, return.
        let identities = age::get_identities(identity_paths)?;
        age::decrypt(output_path, &input_path, &identities)?;

        // Calculate hash before editing
        let pre_edit_hash = util::sha256(&input_path)?;

        // Prompt user to edit file
        editor_hook(&input_path, &editor)?;

        // Calculate hash after editing
        let post_edit_hash = util::sha256(&input_path)?;

        // Return if the file wasn't changed when editing
        if pre_edit_hash == post_edit_hash {
            writeln!(
                writer,
                "{} wasn't changed, skipping re-encryption.",
                output_path.display()
            )?;
            return Ok(());
        }
    } else {
        fs::File::create(&input_path)?;
        editor_hook(&input_path, &editor)?;
    }

    age::encrypt(input_path, output_path.clone(), &entry.public_keys)?;

    Ok(())
}

/// Reads the rules file using Nix to output the attribute set as a JSON string.
/// Return value is parsed into a serde JSON value.
fn nix_rules_to_json<P: AsRef<Path>>(path: P) -> Result<serde_json::Value> {
    let rules_filepath = path.as_ref().to_string_lossy();

    let output = process::Command::new("nix")
        .arg("eval")
        .arg("--experimental-features")
        .arg("nix-command flakes")
        .arg("--no-net")
        .arg("--impure")
        .arg("--json")
        .arg("--expr")
        .arg(format!("import {}", rules_filepath))
        .output()
        .wrap_err("failed to execute nix")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!("Failed to read {} as JSON", rules_filepath))
            .with_section(|| stderr.trim().to_string().header("Stderr:"));
    }

    let val = serde_json::from_slice(&output.stdout)?;
    Ok(val)
}

/// Open a file for editing.
///
/// [Copied from cole-h/agenix-rs (ASL 2.0 / MIT)](
/// https://github.com/cole-h/agenix-rs/blob/8e0554179f1ac692fb865c256e9d7fb91b6a692d/src/cli.rs#L236-L257)
fn editor_hook(path: &Path, editor: &str) -> Result<()> {
    let (editor, args) = util::split_editor(editor)?;

    let cmd = process::Command::new(&editor)
        .args(args.unwrap_or_else(Vec::new))
        .arg(path)
        .stdin(process::Stdio::inherit())
        .stdout(process::Stdio::inherit())
        .stderr(process::Stdio::piped())
        .output()
        .wrap_err_with(|| format!("Failed to spawn editor '{}'", &editor))?;

    if !cmd.status.success() {
        let stderr = String::from_utf8_lossy(&cmd.stderr);

        return Err(eyre!(
            "Editor '{}' exited with non-zero status code",
            &editor
        ))
        .with_section(|| stderr.trim().to_string().header("Stderr:"));
    }

    Ok(())
}

/// Run the program by parsing the command line arguments and writing
/// to the passed writer
#[allow(clippy::missing_errors_doc)]
#[allow(clippy::missing_panics_doc)]
pub fn run<I, T>(itr: I, mut writer: impl Write, mut writer_err: impl Write) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let opts = cli::parse_args(itr);

    if opts.schema {
        write!(writer, "{}", AGENIX_JSON_SCHEMA)?;
    } else {
        if let Err(report) = validate_rules_file(&opts.rules) {
            writeln!(
                writer_err,
                "error: secrets rules are invalid: '{}'\n{}",
                &opts.rules, report
            )?;
            process::exit(1);
        }

        let rules = parse_rules(&opts.rules)?;
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
            edit(&rule, &identities, editor, &mut writer)?;
        } else if opts.rekey {
            rekey(&rules, &identities, &mut writer)?;
        }
    }

    Ok(())
}
