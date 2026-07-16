//! Uses the age crate to encrypt, decrypt and rekey files

use std::{
    convert::Into,
    fs,
    io::{self, BufRead, BufReader},
    path::Path,
};

use age::{
    armor::{ArmoredReader, ArmoredWriter, Format},
    cli_common::{
        file_io::{InputReader, OutputFormat, OutputWriter},
        StdinGuard,
    },
    decryptor::RecipientsDecryptor,
};

use base64::prelude::{Engine, BASE64_STANDARD, BASE64_STANDARD_NO_PAD};
use sha2::{Digest, Sha256};

use color_eyre::{
    eyre::{eyre, Result, WrapErr},
    Help,
};

use tempfile::NamedTempFile;

fn get_age_decryptor<P: AsRef<Path>>(
    path: P,
) -> Result<RecipientsDecryptor<ArmoredReader<BufReader<InputReader>>>> {
    let s = path.as_ref().to_str().map(std::string::ToString::to_string);
    let input_reader = InputReader::new(s)?;
    let decryptor = age::Decryptor::new(ArmoredReader::new(input_reader))?;

    match decryptor {
        age::Decryptor::Passphrase(_) => {
            Err(eyre!(String::from("Agenix does not support passphrases")))
        }
        age::Decryptor::Recipients(decryptor) => Ok(decryptor),
    }
}

/// Parses a recipient from a string.
/// [Copied from str4d/rage (ASL-2.0)](
/// https://github.com/str4d/rage/blob/85c0788dc511f1410b4c1811be6b8904d91f85db/rage/src/bin/rage/main.rs)
fn parse_recipient(
    s: &str,
    recipients: &mut Vec<Box<dyn age::Recipient + Send>>,
    plugin_recipients: &mut Vec<age::plugin::Recipient>,
) -> Result<()> {
    if let Ok(pk) = s.parse::<age::x25519::Recipient>() {
        recipients.push(Box::new(pk));
        Ok(())
    } else if let Some(pk) = { s.parse::<age::ssh::Recipient>().ok().map(Box::new) } {
        recipients.push(pk);
        Ok(())
    } else if let Ok(pk) = s.parse::<age::plugin::Recipient>() {
        plugin_recipients.push(pk);
        Ok(())
    } else {
        Err(eyre!("Invalid recipient: {}", s))
            .with_suggestion(|| "Make sure you use an ssh-ed25519, ssh-rsa or an X25519 public key, alternatively install an age plugin which supports your key")
    }
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

/// Searches plugins and transforms `age::plugin::Recipient` to `age::Recipients`
fn merge_plugin_recipients_and_recipients(
    recipients: &mut Vec<Box<dyn age::Recipient + Send>>,
    plugin_recipients: &[age::plugin::Recipient],
) -> Result<()> {
    // Get names of all required plugins from the recipients
    let mut plugin_names = plugin_recipients
        .iter()
        .map(age::plugin::Recipient::plugin)
        .collect::<Vec<_>>();
    plugin_names.sort_unstable();
    plugin_names.dedup();

    // Add to recipients
    for plugin_name in plugin_names {
        recipients.push(Box::new(age::plugin::RecipientPluginV1::new(
            plugin_name,
            plugin_recipients,
            // Rage allows for symmetric encryption, but this is not actually something which fits
            // into ragenix's design
            &Vec::<age::plugin::Identity>::new(),
            age::cli_common::UiCallbacks,
        )?));
    }
    Ok(())
}

/// Get all the identities from the given paths and the default locations.
///
/// Default locations are `$HOME/.ssh/id_rsa` and `$HOME/.ssh/id_ed25519`.
pub(crate) fn get_identities(identity_paths: &[String]) -> Result<Vec<Box<dyn age::Identity>>> {
    let mut identities: Vec<String> = identity_paths.to_vec();
    let mut default_identities = get_default_identity_paths()?;

    identities.append(&mut default_identities);

    if identities.is_empty() {
        Err(eyre!("No usable identity or identities"))
    } else {
        // Error out if an identity is tried to be read from stdin
        let mut stdin_guard = StdinGuard::new(true);
        Ok(age::cli_common::read_identities(
            identities,
            None,
            &mut stdin_guard,
        )?)
    }
}

/// Decrypt an age-encrypted file to a plaintext file.
///
/// The output file is created with a mode of `0o600`.
pub(crate) fn decrypt<P: AsRef<Path>>(
    input_file: P,
    output_file: P,
    identities: &[Box<dyn age::Identity>],
) -> Result<()> {
    let output_file_mode: u32 = 0o600;
    let decryptor = get_age_decryptor(input_file)?;
    decryptor
        .decrypt(identities.iter().map(|i| i.as_ref() as &dyn age::Identity))
        .map_err(Into::into)
        .and_then(|mut plaintext_reader| {
            let output = output_file
                .as_ref()
                .to_str()
                .map(std::string::ToString::to_string);
            let mut ciphertext_writer =
                OutputWriter::new(output, true, OutputFormat::Unknown, output_file_mode, false)?;
            io::copy(&mut plaintext_reader, &mut ciphertext_writer)?;
            Ok(())
        })
}

/// Encrypt a plaintext file to an age-encrypted file.
///
/// The output file is created with a mode of `0o644`.
pub(crate) fn encrypt<P: AsRef<Path>>(
    input_file: P,
    output_file: P,
    public_keys: &[String],
) -> Result<()> {
    let output_file_mode: u32 = 0o644;
    let mut input = InputReader::new(input_file.as_ref().to_str().map(str::to_string))?;

    // Create an output to the user-requested location.
    let output = OutputWriter::new(
        output_file.as_ref().to_str().map(str::to_string),
        true,
        OutputFormat::Text,
        output_file_mode,
        false,
    )?;

    let mut recipients: Vec<Box<dyn age::Recipient + Send>> = vec![];
    let mut plugin_recipients: Vec<age::plugin::Recipient> = vec![];

    for pubkey in public_keys {
        parse_recipient(pubkey, &mut recipients, &mut plugin_recipients)?;
    }

    merge_plugin_recipients_and_recipients(&mut recipients, &plugin_recipients)?;

    let encryptor =
        age::Encryptor::with_recipients(recipients).ok_or(eyre!("Missing recipients"))?;

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

/// The recipients of an age file as far as its header reveals them.
///
/// SSH recipients are identified by the key fingerprint tag age embeds in
/// their stanzas. X25519 recipients are unlinkable by design, so only their
/// number is known.
#[derive(Debug, PartialEq, Eq)]
struct RecipientFingerprint {
    ssh_tags: Vec<String>,
    x25519_count: usize,
}

/// Compute the tag age uses in stanzas of SSH recipients: the unpadded
/// base64 encoding of the first four bytes of the SHA-256 digest of the
/// SSH public key blob.
fn ssh_recipient_tag(pubkey: &str) -> Result<String> {
    let blob_b64 = pubkey
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| eyre!("Invalid SSH public key: {}", pubkey))?;
    let blob = BASE64_STANDARD.decode(blob_b64)?;
    let digest = Sha256::digest(&blob);
    Ok(BASE64_STANDARD_NO_PAD.encode(&digest[..4]))
}

/// Compute the [`RecipientFingerprint`] an age file would have if it were
/// encrypted to exactly the given public keys.
///
/// Returns `Ok(None)` if any key is a plugin recipient as those cannot be
/// identified in an age header.
fn fingerprint_public_keys(public_keys: &[String]) -> Result<Option<RecipientFingerprint>> {
    let mut ssh_tags: Vec<String> = vec![];
    let mut x25519_count: usize = 0;

    for pubkey in public_keys {
        if pubkey.parse::<age::x25519::Recipient>().is_ok() {
            x25519_count += 1;
        } else if pubkey.parse::<age::ssh::Recipient>().is_ok() {
            ssh_tags.push(ssh_recipient_tag(pubkey)?);
        } else {
            return Ok(None);
        }
    }

    ssh_tags.sort_unstable();
    Ok(Some(RecipientFingerprint {
        ssh_tags,
        x25519_count,
    }))
}

/// Read the [`RecipientFingerprint`] from the header of an age-encrypted file.
///
/// Returns `Ok(None)` if the file is not a valid age file or contains a
/// stanza which cannot be attributed to an SSH or X25519 recipient.
fn fingerprint_encrypted_file<P: AsRef<Path>>(path: P) -> Result<Option<RecipientFingerprint>> {
    let s = path.as_ref().to_str().map(std::string::ToString::to_string);
    let input_reader = InputReader::new(s)?;
    let mut reader = BufReader::new(ArmoredReader::new(input_reader));

    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim_end() != "age-encryption.org/v1" {
        return Ok(None);
    }

    let mut ssh_tags: Vec<String> = vec![];
    let mut x25519_count: usize = 0;

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            // Header ended without a MAC line; not a valid age file
            return Ok(None);
        }
        if line.starts_with("---") {
            break;
        }
        if let Some(stanza) = line.strip_prefix("-> ") {
            let mut args = stanza.split_whitespace();
            match (args.next(), args.next()) {
                (Some("X25519"), Some(_)) => x25519_count += 1,
                (Some("ssh-ed25519" | "ssh-rsa"), Some(tag)) => ssh_tags.push(tag.to_string()),
                (Some("scrypt"), _) => return Ok(None),
                // Any other stanza is grease or belongs to a plugin recipient;
                // neither identifies a recipient we could compare to the rules
                _ => (),
            }
        }
        // All other lines are stanza body lines which don't identify recipients
    }

    ssh_tags.sort_unstable();
    Ok(Some(RecipientFingerprint {
        ssh_tags,
        x25519_count,
    }))
}

/// Check whether a file is already encrypted to exactly the given public
/// keys, as far as its age header reveals.
///
/// SSH recipients are compared by their fingerprint tags. As age hides the
/// identity of X25519 recipients, they are only compared by count; replacing
/// an X25519 recipient with another one goes unnoticed. Rules with plugin
/// recipients never match, and plugin stanzas in the file are ignored as
/// they cannot be told apart from grease.
pub(crate) fn encrypted_to_recipients<P: AsRef<Path>>(
    path: P,
    public_keys: &[String],
) -> Result<bool> {
    match fingerprint_public_keys(public_keys)? {
        Some(expected) => Ok(fingerprint_encrypted_file(path)? == Some(expected)),
        None => Ok(false),
    }
}

/// Re-encrypt a file in memory using the given public keys.
///
/// Decrypts the file and stream-encrypts the contents into a temporary
/// file. Afterward, the temporary file replaces the file at the input path.
///
/// Plaintext is never written to persistent storage but only processed in memory.
pub(crate) fn rekey<P: AsRef<Path>>(
    file: P,
    identities: &[Box<dyn age::Identity>],
    public_keys: &[String],
) -> Result<()> {
    let mut recipients: Vec<Box<dyn age::Recipient + Send>> = vec![];
    let mut plugin_recipients: Vec<age::plugin::Recipient> = vec![];

    for pubkey in public_keys {
        parse_recipient(pubkey, &mut recipients, &mut plugin_recipients)?;
    }
    let decryptor = get_age_decryptor(&file)?;
    decryptor
        .decrypt(identities.iter().map(|i| i.as_ref() as &dyn age::Identity))
        .map_err(Into::into)
        .and_then(|mut plaintext_reader| {
            // Create a temporary file to write the re-encrypted data to
            let outfile = NamedTempFile::new()?;

            // Merge plugin recipients
            merge_plugin_recipients_and_recipients(&mut recipients, &plugin_recipients)?;

            // Create an encryptor for the (new) recipients to encrypt the file for
            let encryptor =
                age::Encryptor::with_recipients(recipients).ok_or(eyre!("Missing recipients"))?;
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
            fs::copy(outfile, file)?;

            Ok(())
        })
}

#[cfg(test)]
mod test_encrypted_to_recipients {
    use super::*;

    const X25519: &str = "age1wl3fqfvyml0c5eaj00j0frad4vhspgx9t8sngq4342j7rzjw4pqs80euxk";
    const SSH_ED25519: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILoPdkEfhcsmW6Lg86GMrEJZnYfFBb7fL9G/IXK7pDQd";
    const SSH_RSA: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDHd3yBYhZbBkMqycy/SOgx9d79TV5Q76czfkmKUKVzywUJbJCwZ4wMA+ff7QzBufZRoAWpGeQb+rssLQEOwR+VX30Fw7K92W4kK6BCF5phP6AUCo07e3vjGqKvgJ4+8LYvcCB17bYf8pJhb4GoOGLrlJNKbGZOhfYE0eGFu/fWsVybQasC2naieKfqHOwS9kNK0N1gSnWh0qu3Du9vBAbQBEE13mPGe4zEdIzTogM068xgKhfJUWqu1xCyVBVJNdz9Xw0NLaWQJon8YXDe62ifxLj3LgndwKm91cN9mmL0klcGB5O8K2mPE0ZGFMDuxdcllUchQgYXdNxEWB4EvpkvpQbiO+fjgMpHeEEiNPd/v06amSBqK+QlIGEkPAElELphPLiTJmHVqxc5NaffVc7F+zM+c3+aWB5Fqgk1jcnqm8HmlLEvPPT1S00c80SkY1V3lUUOirFlciP/pEivJejA5Yj2i1NEEELnrCdBw/xQ4jfesIxcqmBhxk5dWeBbfGs=";

    fn example_file() -> &'static str {
        concat!(env!("CARGO_MANIFEST_DIR"), "/example/root.passwd.age")
    }

    #[test]
    fn computes_tags_age_writes_to_stanzas() -> Result<()> {
        // Expected values taken from the header of `example/root.passwd.age`
        assert_eq!(ssh_recipient_tag(SSH_ED25519)?, "a6H7Ng");
        assert_eq!(ssh_recipient_tag(SSH_RSA)?, "1NDNnA");
        Ok(())
    }

    #[test]
    fn matches_unchanged_recipients() -> Result<()> {
        let public_keys: Vec<String> = [X25519, SSH_ED25519, SSH_RSA].map(String::from).to_vec();
        assert!(encrypted_to_recipients(example_file(), &public_keys)?);
        Ok(())
    }

    #[test]
    fn detects_changed_recipients() -> Result<()> {
        // Removed SSH recipient
        let public_keys: Vec<String> = [X25519, SSH_ED25519].map(String::from).to_vec();
        assert!(!encrypted_to_recipients(example_file(), &public_keys)?);

        // Removed X25519 recipient
        let public_keys: Vec<String> = [SSH_ED25519, SSH_RSA].map(String::from).to_vec();
        assert!(!encrypted_to_recipients(example_file(), &public_keys)?);

        // Additional X25519 recipient
        let public_keys: Vec<String> = [
            X25519,
            SSH_ED25519,
            SSH_RSA,
            "age1fjc9tyguvxfqh2ey2qqfc066g3gee7hlnhqn2g7yn4f6smymmsnq6xdn2t",
        ]
        .map(String::from)
        .to_vec();
        assert!(!encrypted_to_recipients(example_file(), &public_keys)?);

        Ok(())
    }

    #[test]
    fn never_matches_plugin_recipients() -> Result<()> {
        let public_keys: Vec<String> = [
            X25519,
            SSH_ED25519,
            SSH_RSA,
            "age1yubikey1qwt50d05nh5vutpdzmlg5wn80xq5negm4uj9y5q0jvzgxrq2yn2vsy5g5gd",
        ]
        .map(String::from)
        .to_vec();
        assert!(!encrypted_to_recipients(example_file(), &public_keys)?);
        Ok(())
    }

    #[test]
    fn never_matches_non_age_files() -> Result<()> {
        let rules_file = concat!(env!("CARGO_MANIFEST_DIR"), "/example/secrets.nix");
        let public_keys: Vec<String> = [X25519].map(String::from).to_vec();
        assert!(!encrypted_to_recipients(rules_file, &public_keys)?);
        Ok(())
    }
}
