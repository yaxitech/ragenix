//! Uses the age crate to encrypt, decrypt and rekey files

use std::{
    convert::Into,
    fs,
    io::{self, BufReader},
    path::Path,
};

use age::{
    armor::{ArmoredReader, ArmoredWriter, Format},
    cli_common::file_io::{InputReader, OutputFormat, OutputWriter},
    decryptor::RecipientsDecryptor,
};

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
        Ok(age::cli_common::read_identities(identities, None)?)
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
                OutputWriter::new(output, OutputFormat::Unknown, output_file_mode, false)?;
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
