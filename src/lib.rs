mod cli;

use age::{
    armor::{ArmoredReader, ArmoredWriter, Format},
    cli_common::file_io,
    decryptor::RecipientsDecryptor,
};
use color_eyre::{
    eyre::{eyre, Result, WrapErr},
    Help, SectionExt,
};
use jsonschema::JSONSchema;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    io::{self, BufReader, Write},
    os::unix::prelude::PermissionsExt,
    path::{Component, Path, PathBuf},
    process,
};
use tempfile::NamedTempFile;

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

/// Validate and parse the given rules file path
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
    let identities = get_identities(identities)?;
    for entry in entries.iter() {
        let p = entry.path.to_string_lossy();
        if entry.path.exists() {
            writeln!(writer, "Rekeying {}", p)?;
            rekey_entry(entry, &identities)?;
        } else {
            writeln!(writer, "Does not exist, ignored: {}", p)?;
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

    if entry.path.exists() {
        // If the file already exists, first decrypt it, hash it, open it in `editor`,
        // hash the result, and if the hashes are equal, return.

        let identities = get_identities(identity_paths)?;
        decrypt(&entry.path, &input_path, &identities)?;

        // Calculate hash before editing
        let pre_edit_hash = sha256(&input_path)?;

        // Prompt user to edit file
        editor_hook(&input_path, &editor)?;

        // Calculate hash after editing
        let post_edit_hash = sha256(&input_path)?;

        // Return if the file wasn't changed when editing
        if pre_edit_hash == post_edit_hash {
            writeln!(
                writer,
                "{} wasn't changed, skipping re-encryption.",
                entry.path.to_string_lossy()
            )?;
            return Ok(());
        }
    } else {
        fs::File::create(&input_path)?;
        editor_hook(&input_path, &editor)?;
    }

    let mut input = file_io::InputReader::new(input_path.to_str().map(str::to_string))?;

    // Create an output to the user-requested location.
    let output = file_io::OutputWriter::new(
        entry.path.to_str().map(str::to_string),
        file_io::OutputFormat::Text,
        0o644,
    )?;

    let mut recipients: Vec<Box<dyn age::Recipient>> = vec![];
    for pubkey in &entry.public_keys {
        parse_recipient(&pubkey, &mut recipients)?;
    }
    let encryptor = age::Encryptor::with_recipients(recipients);

    let mut output = encryptor
        .wrap_output(
            ArmoredWriter::wrap_output(output, Format::AsciiArmor)
                .wrap_err("Failed to wrap output with age::ArmoredWriter")?,
        )
        .map_err(|err| eyre!(err))?;

    io::copy(&mut input, &mut output)?;
    output.finish().and_then(ArmoredWriter::finish)?;

    Ok(())
}

/// Normalize a path, removing things like `.` and `..`.
///
/// CAUTION: This does not resolve symlinks (unlike
/// [`std::fs::canonicalize`]). This may cause incorrect or surprising
/// behavior at times. This should be used carefully. Unfortunately,
/// [`std::fs::canonicalize`] can be hard to use correctly, since it can often
/// fail, or on Windows returns annoying device paths. This is a problem Cargo
/// needs to improve on.
///
/// [Copied from Cargo (ASL 2.0 / MIT)](
/// https://github.com/rust-lang/cargo/blob/58a961314437258065e23cb6316dfc121d96fb71/crates/cargo-util/src/paths.rs#L81-L106)
#[allow(clippy::option_if_let_else)]
#[allow(clippy::cloned_instead_of_copied)]
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                ret.pop();
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}

/// Hash a file using SHA-256
fn sha256<P: AsRef<Path>>(path: P) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().to_vec())
}

/// Returns the file paths to `$HOME/.ssh/{id_rsa,id_ed25519}` if each exists
fn get_default_identity_paths() -> Result<Vec<String>> {
    let home_path = home::home_dir().ok_or_else(|| eyre!("Could not determine home directory"))?;
    let ssh_dir = home_path.join(".ssh");

    let id_rsa = ssh_dir.join("id_rsa");
    let id_ed25519 = ssh_dir.join("id_ed25519");

    let filtered_paths = [id_rsa, id_ed25519]
        .iter()
        .filter(|x| x.exists())
        .filter_map(|x| x.to_str())
        .map(std::string::ToString::to_string)
        .collect();

    Ok(filtered_paths)
}

/// Get all the identities from the given paths and the default locations
fn get_identities(identity_paths: &[String]) -> Result<Vec<Box<dyn age::Identity>>> {
    let mut identities: Vec<String> = identity_paths.to_vec();
    let mut default_identities = get_default_identity_paths()?;

    identities.append(&mut default_identities);

    if identities.is_empty() {
        Err(eyre!("No usable identity or identities"))
    } else {
        age::cli_common::read_identities(identities, |s| eyre!(s), |s, e| eyre!("{}: {:?}", s, e))
    }
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

fn get_age_decryptor<P: AsRef<Path>>(
    path: P,
) -> Result<RecipientsDecryptor<ArmoredReader<BufReader<file_io::InputReader>>>> {
    let s = path.as_ref().to_str().map(std::string::ToString::to_string);
    let input_reader = file_io::InputReader::new(s)?;
    let decryptor = age::Decryptor::new(ArmoredReader::new(input_reader))?;

    match decryptor {
        age::Decryptor::Passphrase(_) => {
            Err(eyre!(String::from("Agenix does not support passphrases")))
        }
        age::Decryptor::Recipients(decryptor) => Ok(decryptor),
    }
}

fn decrypt<P: AsRef<Path>>(
    input_file: P,
    output_file: P,
    identities: &[Box<dyn age::Identity>],
) -> Result<()> {
    let decryptor = get_age_decryptor(input_file)?;
    decryptor
        .decrypt(identities.iter().map(|i| i.as_ref() as &dyn age::Identity))
        .map_err(|e| e.into())
        .and_then(|mut plaintext_reader| {
            let output = output_file
                .as_ref()
                .to_str()
                .map(std::string::ToString::to_string);
            let mut ciphertext_writer =
                file_io::OutputWriter::new(output, file_io::OutputFormat::Unknown, 0o600)?;
            io::copy(&mut plaintext_reader, &mut ciphertext_writer)?;
            Ok(())
        })
}

/// Parses a recipient from a string.
/// [Copied from str4d/rage (ASL-2.0)](
/// https://github.com/str4d/rage/blob/85c0788dc511f1410b4c1811be6b8904d91f85db/rage/src/bin/rage/main.rs)
fn parse_recipient(s: &str, recipients: &mut Vec<Box<dyn age::Recipient>>) -> Result<()> {
    if let Ok(pk) = s.parse::<age::x25519::Recipient>() {
        recipients.push(Box::new(pk));
        Ok(())
    } else if let Some(pk) = { s.parse::<age::ssh::Recipient>().ok().map(Box::new) } {
        recipients.push(pk);
        Ok(())
    } else {
        Err(eyre!("Invalid recipient: {}", s))
            .with_suggestion(|| "Make sure you use an ssh-ed25519, ssh-rsa or an X25519 public key")
    }
}

/// Split editor into binary and (shell) arguments
fn split_editor(editor: &str) -> Result<(String, Option<Vec<String>>)> {
    let mut splitted: Vec<String> = shlex::split(editor)
        .ok_or_else(|| eyre!("Could not parse editor"))?
        .iter()
        .map(String::from)
        .collect();

    if splitted.is_empty() {
        Err(eyre!("Editor is empty"))
    } else {
        let binary = splitted.first().unwrap().clone();
        let args = if splitted.len() >= 2 {
            Some(splitted.split_off(1))
        } else {
            None
        };
        Ok((binary, args))
    }
}

#[cfg(test)]
mod test_split_editor {
    use super::*;

    #[test]
    fn parse_editor_no_args() -> Result<()> {
        let actual = split_editor("vim")?;
        let expected = (String::from("vim"), None);
        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn parse_editor_one_arg() -> Result<()> {
        let actual = split_editor("vim -R")?;
        let expected = (String::from("vim"), Some(vec![String::from("-R")]));
        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn parse_editor_complex_1() -> Result<()> {
        let actual = split_editor(r#"sed -i "s/.*/ x  /""#)?;
        let expected = (
            String::from("sed"),
            Some(vec![String::from("-i"), String::from("s/.*/ x  /")]),
        );
        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn parse_editor_complex_2() -> Result<()> {
        let actual = split_editor(r#"sed -i 's/.*/ x  /'"#)?;
        let expected = (
            String::from("sed"),
            Some(vec![String::from("-i"), String::from("s/.*/ x  /")]),
        );
        assert_eq!(actual, expected);
        Ok(())
    }
}

/// Open a file for editing.
///
/// [Copied from cole-h/agenix-rs (ASL 2.0 / MIT)](
/// https://github.com/cole-h/agenix-rs/blob/8e0554179f1ac692fb865c256e9d7fb91b6a692d/src/cli.rs#L236-L257)
fn editor_hook(path: &Path, editor: &str) -> Result<()> {
    let (editor, args) = split_editor(editor)?;

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

/// Re-encrypt a file in memory.
///
/// Decrypts the [`entry`]'s file and stream-encrypts the contents into a temporary
/// file. Afterward, the temporary file replaces the file at the input path.
///
/// Plaintext is never written to persistent storage but only processed in memory.
fn rekey_entry(entry: &RagenixRule, identities: &[Box<dyn age::Identity>]) -> Result<()> {
    let decryptor = get_age_decryptor(&entry.path)?;
    decryptor
        .decrypt(identities.iter().map(|i| i.as_ref() as &dyn age::Identity))
        .map_err(|e| e.into())
        .and_then(|mut plaintext_reader| {
            // Create a temporary file to write the re-encrypted data to
            let outfile = NamedTempFile::new()?;

            // Create an encryptor for the (new) recipients to encrypt the file for
            let mut recipients: Vec<Box<dyn age::Recipient>> = vec![];
            for pubkey in &entry.public_keys {
                parse_recipient(&pubkey, &mut recipients)?;
            }
            let encryptor = age::Encryptor::with_recipients(recipients);
            let mut ciphertext_writer = encryptor
                .wrap_output(
                    ArmoredWriter::wrap_output(&outfile, Format::AsciiArmor)
                        .wrap_err("Failed to wrap output with age::ArmoredWriter")?,
                )
                .map_err(|err| eyre!(err))?;

            // Do the re-encryption
            io::copy(&mut plaintext_reader, &mut ciphertext_writer)?;
            ciphertext_writer.finish().and_then(ArmoredWriter::finish)?;

            // Re-encrpytion is done, now replace the original file
            fs::copy(outfile, &entry.path)?;

            Ok(())
        })
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
            let path_normalized = normalize_path(Path::new(path));
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
