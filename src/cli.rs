use std::clone::Clone;
use std::ffi::OsString;

use clap::{
    crate_authors, crate_description, crate_name, crate_version, Arg, ArgAction, ArgGroup, Command,
    ValueHint,
};

#[allow(dead_code)] // False positive
#[derive(Debug, Clone)]
pub(crate) struct Opts {
    pub edit: Option<String>,
    pub editor: Option<String>,
    pub identities: Option<Vec<String>>,
    pub rekey: bool,
    pub rules: String,
    pub schema: bool,
    pub verbose: bool,
}

fn build() -> Command {
    Command::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .arg(
            Arg::new("edit")
                .help("edits the age-encrypted FILE using $EDITOR")
                .long("edit")
                .short('e')
                .num_args(1)
                .value_name("FILE")
                .requires("editor")
                .value_hint(ValueHint::FilePath),
        )
        .arg(
            Arg::new("rekey")
                .help("re-encrypts all secrets with specified recipients")
                .long("rekey")
                .short('r')
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("identity")
                .help("private key to use when decrypting")
                .long("identity")
                .short('i')
                .num_args(1..)
                .value_name("PRIVATE_KEY")
                .required(false)
                .value_hint(ValueHint::FilePath),
        )
        .arg(
            Arg::new("verbose")
                .help("verbose output")
                .long("verbose")
                .short('v')
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("schema")
                .help("Prints the JSON schema Agenix rules have to conform to")
                .long("schema")
                .short('s')
                .action(ArgAction::SetTrue),
        )
        .group(
            ArgGroup::new("action")
                .args(["edit", "rekey", "schema"])
                .required(true),
        )
        .arg(
            Arg::new("editor")
                .help("editor to use when editing FILE")
                .long("editor")
                .num_args(1)
                .env("EDITOR")
                .value_name("EDITOR")
                .value_hint(ValueHint::CommandString),
        )
        .arg(
            Arg::new("rules")
                .help("path to Nix file specifying recipient public keys")
                .long("rules")
                .num_args(1)
                .env("RULES")
                .value_name("RULES")
                .default_value("./secrets.nix")
                .value_hint(ValueHint::FilePath),
        )
}

/// Parse the command line arguments using Clap
#[allow(dead_code)] // False positive
pub(crate) fn parse_args<I, T>(itr: I) -> Opts
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let app = build();

    let matches = app.get_matches_from(itr);

    Opts {
        edit: matches.get_one::<String>("edit").cloned(),
        editor: matches.get_one::<String>("editor").cloned(),
        identities: matches
            .get_many::<String>("identity")
            .map(|vals| vals.cloned().collect::<Vec<_>>()),
        rekey: matches.get_flag("rekey"),
        rules: matches
            .get_one::<String>("rules")
            .cloned()
            .expect("Should never happen"),
        schema: matches.get_flag("schema"),
        verbose: matches.get_flag("verbose"),
    }
}
