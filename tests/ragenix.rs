use assert_cmd::{crate_name, Command};
use color_eyre::Result;
use copy_dir::copy_dir;
use indoc::{formatdoc, indoc};
use predicates::prelude::*;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

fn copy_example_to_tmpdir() -> Result<(TempDir, PathBuf)> {
    let base_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example_dir = base_dir.join("example");

    let dir = tempfile::tempdir()?;
    let dir_canonicalized = fs::canonicalize(&dir)?;
    let res_path = dir_canonicalized.join(example_dir.file_name().unwrap());
    copy_dir(example_dir, &res_path)?;

    Ok((dir, res_path))
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn edit_no_rekey_if_unchanged() -> Result<()> {
    let (_dir, path) = copy_example_to_tmpdir()?;

    let file = "github-runner.token.age";
    let file_full_path = path.join(file);
    let expected = format!(
        "{} wasn't changed, skipping re-encryption.\n",
        file_full_path.to_string_lossy()
    );

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--edit")
        .arg(file)
        .arg("--identity")
        .arg("keys/id_ed25519")
        .env("EDITOR", "true")
        .assert();

    assert.success().stdout(expected);

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn edit_works() -> Result<()> {
    let (_dir, path) = copy_example_to_tmpdir()?;

    let file = "github-runner.token.age";

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--edit")
        .arg(file)
        .arg("--identity")
        .arg("keys/id_ed25519")
        .env("EDITOR", "sed '-i' 's|.*|yaxifaxi|g'")
        .assert();

    assert.success().stdout("");

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn edit_new_entry() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let dir_path = fs::canonicalize(dir.path())?;
    let rules = indoc! {r#"
    {
        "pandora.age".publicKeys = [
            "age1qjzezkeazfdg4p9x0kjapjtreyyt74pg34ftzfypcdpy7wgh6acqxeyvwt"
        ];
    }
    "#};
    fs::File::create(dir.path().join("secrets.nix"))
        .and_then(|mut file| file.write_all(rules.as_bytes()))?;

    let pandora = dir_path.join("pandora");
    fs::File::create(&pandora).and_then(|mut file| file.write_all(b"wurzelpfropf!"))?;

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(dir.path())
        .arg("--edit")
        .arg("pandora.age")
        .env("EDITOR", format!("cp {}", &pandora.display()))
        .assert();

    assert.success().stdout("");
    assert!(fs::metadata(pandora).is_ok());
    let ciphertext = fs::read_to_string(dir.path().join("pandora.age"))?;
    assert!(predicate::str::starts_with("-----BEGIN AGE ENCRYPTED FILE-----").eval(&ciphertext));
    assert!(predicate::str::ends_with("-----END AGE ENCRYPTED FILE-----\n").eval(&ciphertext));

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn edit_new_entry_stdin() -> Result<()> {
    // # created: 2021-09-20T23:41:59+02:00
    // # public key: age1fjc9tyguvxfqh2ey2qqfc066g3gee7hlnhqn2g7yn4f6smymmsnq6xdn2t
    // AGE-SECRET-KEY-1C744H5LMUVHGVLX8HXAWA9ENXXXJ6R6F89V5AGEDXXD8GECQ624QQUXKHX
    let plaintext = "secret wurzelpfropf";

    let dir = tempfile::tempdir()?;
    let rules = indoc! {r#"
    {
        "pandora.age".publicKeys = [
            "age1fjc9tyguvxfqh2ey2qqfc066g3gee7hlnhqn2g7yn4f6smymmsnq6xdn2t"
        ];
    }
    "#};
    fs::write(dir.path().join("secrets.nix"), rules)?;

    let stdin_path = dir.path().join("stdin");
    fs::write(&stdin_path, plaintext)?;

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(dir.path())
        .arg("--edit")
        .arg("pandora.age")
        .env("EDITOR", "-")
        .pipe_stdin(stdin_path)?
        .assert();

    assert.success().stdout("");

    // Verify the plaintext of the encrypted file
    let privkey_path = dir.path().join("key.txt");
    fs::write(
        &privkey_path,
        "AGE-SECRET-KEY-1C744H5LMUVHGVLX8HXAWA9ENXXXJ6R6F89V5AGEDXXD8GECQ624QQUXKHX",
    )?;

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(dir.path())
        .arg("--identity")
        .arg(privkey_path)
        .arg("--edit")
        .arg("pandora.age")
        .env("EDITOR", "cat")
        .assert();

    assert.stdout(predicate::str::starts_with(plaintext));

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn edit_existing_entry_stdin() -> Result<()> {
    let plaintext = "secret wurzelpfropf";

    let (_dir, path) = copy_example_to_tmpdir()?;
    let stdin_path = path.join("stdin");
    fs::write(&stdin_path, plaintext)?;

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--edit")
        .arg("github-runner.token.age")
        .env("EDITOR", "-")
        .pipe_stdin(stdin_path)?
        .assert();

    assert.success();

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--identity")
        .arg("keys/id_ed25519")
        .arg("--edit")
        .arg("github-runner.token.age")
        .env("EDITOR", "cat")
        .assert();

    assert
        .success()
        .stdout(predicate::str::starts_with(plaintext));

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn edit_permissions_correct() -> Result<()> {
    let (_dir, path) = copy_example_to_tmpdir()?;
    let script = indoc! { r#"
        #!/usr/bin/env sh
        set -euo pipefail

        FILE="$(readlink -f "$1")"
        TMPDIR="$(dirname "$FILE")"

        file_permissions="$(stat -c '%a' "$FILE")"
        tmpdir_permissions="$(stat -c '%a' "$TMPDIR")"

        if [[ "$file_permissions" != "600" ]]; then
            >&2 printf '%s has wrong permissions %s\n' "$FILE" "$file_permissions"
            exit 1
        fi

        if [[ "$tmpdir_permissions" != "700" ]]; then
            >&2 printf '%s has wrong permissions %s\n' "$TMPDIR" "$tmpdir_permissions"
            exit 1
        fi
    "# };
    let script_path = path.join("verify.sh");

    fs::File::create(&script_path).and_then(|mut f| f.write_all(script.as_bytes()))?;

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--identity")
        .arg("keys/id_ed25519")
        .arg("--edit")
        .arg("github-runner.token.age")
        .env("EDITOR", format!("sh {}", script_path.display()))
        .assert();

    assert.success();

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn rekeying_works() -> Result<()> {
    let (_dir, path) = copy_example_to_tmpdir()?;

    let files = &["github-runner.token.age", "root.passwd.age"];
    let expected = files
        .iter()
        .map(|s| path.join(s))
        .map(|p| format!("Rekeying {}", p.display()))
        .collect::<Vec<String>>()
        .join("\n")
        + "\n";

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--rekey")
        .arg("--identity")
        .arg("keys/id_ed25519")
        .assert();

    assert.success().stdout(expected);

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn rekeying_ignores_not_existing_files() -> Result<()> {
    let (_dir, path) = copy_example_to_tmpdir()?;

    let missing_file = path.join("root.passwd.age");
    fs::remove_file(&missing_file)?;

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--rekey")
        .arg("--identity")
        .arg("keys/id_ed25519")
        .assert();

    assert.success().stdout(predicate::str::contains(format!(
        "Does not exist, ignored: {}",
        missing_file.display()
    )));

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn rekeying_works_default_identities() -> Result<()> {
    let (_dir, path) = copy_example_to_tmpdir()?;

    let keys_dir = path.join("keys");
    let ssh_dir = path.join(".ssh");
    fs::create_dir(&ssh_dir)?;

    for filename in &["id_rsa", "id_ed25519"] {
        fs::copy(keys_dir.join(filename), ssh_dir.join(filename))?;
    }

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--rekey")
        .env("HOME", &path)
        .assert();

    assert
        .success()
        .stdout(predicate::str::starts_with("Rekeying "));

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn rekeying_fails_no_given_identites() -> Result<()> {
    let (_dir, path) = copy_example_to_tmpdir()?;

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--rekey")
        .env("HOME", "/homeless-shelter")
        .assert();

    assert
        .failure()
        .stderr(predicate::str::contains("No usable identity or identities"));

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn rekeying_fails_no_valid_identites() -> Result<()> {
    let (_dir, path) = copy_example_to_tmpdir()?;
    let ssh_dir = path.join(".ssh");
    fs::create_dir(&ssh_dir)?;

    let empty_key_1 = path.join("empty-key-1");
    fs::File::create(&empty_key_1)?;

    let empty_key_2 = path.join("empty-key-2");
    fs::File::create(&empty_key_2)?;

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--rekey")
        .env("HOME", ssh_dir)
        .arg("--identity")
        .arg(empty_key_1)
        .arg(empty_key_2)
        .assert();

    assert
        .failure()
        .stderr(predicate::str::contains("No matching keys found"));

    Ok(())
}

#[test]
fn prints_schema() -> Result<()> {
    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd.arg("--schema").assert();

    let schema = include_str!("../src/ragenix/agenix.schema.json");
    assert.success().stdout(schema);

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn rejects_invalid_rules() -> Result<()> {
    let (_dir, path) = copy_example_to_tmpdir()?;

    fs::File::create(path.join("secrets.nix"))
        .and_then(|mut f| f.write_all(r#"{ wurzel = "pfropf"; }"#.as_bytes()))?;

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(&path)
        .arg("--edit")
        .arg("wurzel")
        .env("EDITOR", "true")
        .assert();

    assert.failure().stderr(indoc! {r#"
            error: secrets rules are invalid: './secrets.nix'
             - /wurzel: "pfropf" is not of type "object"
        "#});

    Ok(())
}

#[test]
#[cfg_attr(not(feature = "recursive-nix"), ignore)]
fn fails_for_invalid_recipient() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let invalid_key = "invalid-key abcdefghijklmnopqrstuvwxyz";
    let rules = formatdoc! {"
        {{
            \"wurzelpfropf.txt.age\".publicKeys = [ \"{}\" ];
        }}
    ", invalid_key };
    fs::File::create(dir.path().join("secrets.nix"))
        .and_then(|mut f| f.write_all(rules.as_bytes()))?;

    let mut cmd = Command::cargo_bin(crate_name!())?;
    let assert = cmd
        .current_dir(dir.path())
        .arg("--edit")
        .arg("wurzelpfropf.txt.age")
        .env("EDITOR", "true")
        .assert();

    assert
        .failure()
        .stderr(predicate::str::contains(format!("Invalid recipient: {invalid_key}")))
        .stderr(predicate::str::contains(
            "Make sure you use an ssh-ed25519, ssh-rsa or an X25519 public key, alternatively install an age plugin which supports your key",
        ));

    Ok(())
}
