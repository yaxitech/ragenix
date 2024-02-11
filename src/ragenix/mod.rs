use color_eyre::{
    eyre::{eyre, Result, WrapErr},
    Help, SectionExt,
};
use jsonschema::JSONSchema;
use lazy_static::lazy_static;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::prelude::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process,
};

use crate::{age, util};

pub(crate) static AGENIX_JSON_SCHEMA_STRING: &str = std::include_str!("agenix.schema.json");

lazy_static! {
    static ref AGENIX_JSON_SCHEMA: serde_json::Value =
        serde_json::from_str(AGENIX_JSON_SCHEMA_STRING).expect("Valid schema!");
}

/// Reads the rules file using Nix to output the attribute set as a JSON string.
/// Return value is parsed into a serde JSON value.
fn nix_rules_to_json<P: AsRef<Path>>(path: P) -> Result<serde_json::Value> {
    let rules_filepath = path.as_ref().to_string_lossy();

    let nix_binary = env!("RAGENIX_NIX_BIN_PATH");
    let output = process::Command::new(nix_binary)
        .arg("--extra-experimental-features")
        .arg("nix-command")
        .arg("eval")
        .arg("--no-net")
        .arg("--json")
        .arg("--file")
        .arg(&*rules_filepath)
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
    if util::is_stdin(editor) {
        let mut src = io::stdin();
        let mut dst = OpenOptions::new()
            .mode(0o600)
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        io::copy(&mut src, &mut dst)?;
    } else {
        let (editor, args) = util::split_editor(editor)?;
        let cmd = process::Command::new(&editor)
            .args(args.unwrap_or_default())
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
    }

    Ok(())
}

#[derive(Debug)]
pub(crate) struct RagenixRule {
    pub path: PathBuf,
    pub public_keys: Vec<String>,
}

/// Validate conformance of the passed path to the JSON schema [`AGENIX_JSON_SCHEMA`].
pub(crate) fn validate_rules_file<P: AsRef<Path>>(path: P) -> Result<()> {
    if !path.as_ref().exists() {
        return Err(eyre!("{} does not exist!", path.as_ref().to_string_lossy()));
    }

    let instance = nix_rules_to_json(&path)?;
    let compiled = JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft7)
        .compile(&AGENIX_JSON_SCHEMA)?;
    let result = compiled.validate(&instance);

    if let Err(errors) = result {
        let error_msg = errors
            .into_iter()
            .map(|err| format!(" - {}: {err}", err.instance_path))
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
pub(crate) fn parse_rules<P: AsRef<Path>>(rules_path: P) -> Result<Vec<RagenixRule>> {
    let instance = nix_rules_to_json(&rules_path)?;

    // It's fine to force unwrap here as we validated the JSON schema
    let mut rules: Vec<RagenixRule> = Vec::new();
    for (rel_path, val) in instance.as_object().unwrap() {
        let dir = fs::canonicalize(rules_path.as_ref().parent().unwrap())?;
        let p = dir.join(rel_path);
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
pub(crate) fn rekey(
    entries: &[RagenixRule],
    identities: &[String],
    mut writer: impl Write,
) -> Result<()> {
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
pub(crate) fn edit(
    entry: &RagenixRule,
    identity_paths: &[String],
    editor: &str,
    mut writer: impl Write,
) -> Result<()> {
    let dir = tempfile::tempdir()?;
    fs::set_permissions(&dir, PermissionsExt::from_mode(0o700))?;

    let input_path = dir.path().join("input");
    let output_path = &entry.path;

    if !output_path.exists() || util::is_stdin(editor) {
        // If the target file does not yet exist, we don't have to decrypt the result for editing.
        // Likewise, if we're reading from stdin, we're going to replace the target file completely.
        fs::File::create(&input_path)?;
        editor_hook(&input_path, editor)?;
    } else {
        // If the file already exists, first decrypt it, hash it, open it in `editor`,
        // hash the result, and if the hashes are equal, return.
        let identities = age::get_identities(identity_paths)?;
        age::decrypt(output_path, &input_path, &identities)?;

        // Calculate hash before editing
        let pre_edit_hash = util::sha256(&input_path)?;

        // Prompt user to edit file
        editor_hook(&input_path, editor)?;

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
    }

    age::encrypt(input_path, output_path.clone(), &entry.public_keys)?;

    Ok(())
}
