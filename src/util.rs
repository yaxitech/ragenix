//! Util functions

use std::{
    fs::File,
    io,
    path::{Component, Path, PathBuf},
};

use color_eyre::eyre::{eyre, Result};
use sha2::{Digest, Sha256};

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
pub(crate) fn normalize_path(path: &Path) -> PathBuf {
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
pub(crate) fn sha256<P: AsRef<Path>>(path: P) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().to_vec())
}

#[cfg(test)]
mod test_sha256 {
    use hex_literal::hex;
    use std::io::Write;

    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn hashes_files_correctly() -> Result<()> {
        let tmpfile = NamedTempFile::new()?;
        tmpfile.as_file().write_all(b"wurzelpfropf")?;
        let result = sha256(tmpfile.path())?;
        assert_eq!(
            result[..],
            hex!("8be65cca515ad097c953967d18d635d86dd78a142ed3d077526bed11c6bec67b")
        );

        let tmpfile = NamedTempFile::new()?;
        tmpfile.as_file().write_all(b"yaxifaxi")?;
        let result = sha256(tmpfile.path())?;
        assert_eq!(
            result[..],
            hex!("a5fa47e8f93604b1ff552d0d4013440e35fbf7f5f00b4a115a100891e4266c63")
        );

        Ok(())
    }
}

/// Test if an editor string is a single hyphen to read from stdin
pub(crate) fn is_stdin(editor: &str) -> bool {
    split_editor(editor).is_ok_and(|(program, args)| program == "-" && args.is_none())
}

/// Split editor into binary and (shell) arguments
pub(crate) fn split_editor(editor: &str) -> Result<(String, Option<Vec<String>>)> {
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
        let actual = split_editor(r"sed -i 's/.*/ x  /'")?;
        let expected = (
            String::from("sed"),
            Some(vec![String::from("-i"), String::from("s/.*/ x  /")]),
        );
        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn parse_editor_stdin() -> Result<()> {
        let actual = split_editor(r" - ")?;
        let expected = (String::from("-"), None);
        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn err_for_empty_editor() {
        let result = split_editor("");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Editor is empty");
    }
}
