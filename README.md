# ragenix

[![Build status](https://img.shields.io/github/actions/workflow/status/yaxitech/ragenix/main.yaml?branch=main)](https://github.com/yaxitech/ragenix/actions?query=branch%3Amain)
[![License](https://img.shields.io/github/license/yaxitech/ragenix)](http://www.apache.org/licenses/LICENSE-2.0.html)
[![Written in Rust](https://img.shields.io/badge/code-rust-orange)](https://www.rust-lang.org)
[![ragenix(1)](https://img.shields.io/badge/man-ragenix(1)-blue)](https://htmlpreview.github.io/?https://github.com/yaxitech/ragenix/blob/main/docs/ragenix.1.html)

`ragenix` provides age-encrypted secrets for NixOS systems which live in the Nix store
and are decrypted on system activation. Using `ragenix` to create, edit and rekey secrets
is possible on any system which has Nix installed—with particular support for NixOS and macOS.

`ragenix` is a drop-in replacement for [@ryantm](https://github.com/ryantm)'s
[`agenix`](https://github.com/ryantm/agenix) written in Rust. It aims at being fully compatible
with its flake while offering more robust command line parsing, additional validation logic,
plugin support, shell completions, and solid tests.

**As opposed to `agenix`, `ragenix` only strives for supporting Nix Flakes**.

## Installation

As `ragenix` seeks to replace `agenix` without breaking compatibility, getting started with age-encrypted
secrets or switching from `agenix` to `ragenix` is easy: just follow the original [instructions from `agenix`](
https://github.com/ryantm/agenix#installation) while replacing references to
`github.com/ryantm/agenix` with `github.com/yaxitech/ragenix`. Everything else should remain the
same as the `ragenix` package provides aliases for a) an `agenix` package and b) the `agenix` binary.
The flake also exposes a NixOS and Darwin module which is passed through from the `agenix` flake.

## Usage

`ragenix` resembles the command line options and behavior of `agenix`.
For the full documentation, read the [ragenix(1) man page](https://htmlpreview.github.io/?https://github.com/yaxitech/ragenix/blob/main/docs/ragenix.1.html).

```
USAGE:
    ragenix [OPTIONS] <--edit <FILE>|--rekey|--schema>

OPTIONS:
    -e, --edit <FILE>                  edits the age-encrypted FILE using $EDITOR
        --editor <EDITOR>              editor to use when editing FILE [env: EDITOR=vim]
    -h, --help                         Print help information
    -i, --identity <PRIVATE_KEY>...    private key to use when decrypting
    -r, --rekey                        re-encrypts all secrets with specified recipients
        --rules <RULES>                path to Nix file specifying recipient public keys [env:
                                       RULES=] [default: ./secrets.nix]
    -s, --schema                       Prints the JSON schema Agenix rules have to conform to
    -v, --verbose                      verbose output
    -V, --version                      Print version information
```

The `ragenix` package also provides shell completions for `bash`, `zsh`, and `fish`. Make sure to install the package with either `nix profile install github:yaxitech/ragenix`, `environment.systemPackages` on NixOS or `home.packages` for home-manager.

## Contributions

We'd love to see PRs from you! Please consider the following guidelines:

- `ragenix` stays compatible to `agenix`. Please make sure your contributions
  don't introduce breaking changes.
- The secrets configuration happens through a Nix configuration.
- New features should support both NixOS and macOS, if applicable.
- Update the manpage, if necessary

The CI invokes `nix flake check`. Some of the checks invoke `nix` itself.
To allow those tests to run `nix`, you have to enable the `recursive-nix` feature.
On NixOS, you can put the following snippet into your `configuration.nix`:

```nix
{
  nix = {
    extraOptions = ''
      experimental-features = nix-command flakes recursive-nix
    '';
    systemFeatures = [ "recursive-nix" ];
  };
}
```

## Similar projects / acknowledgements 

The [`agenix-cli`](https://github.com/cole-h/agenix-cli) project is quite similar to ragenix. In fact, it
served as an inspiration (thanks!). Both projects have in common that they aim
at replacing the fragile shell script with a version written in Rust. In contrast to `ragenix`, however,
`agenix-cli` is not compatible to the original `agenix`. It uses a TOML configuration file to declare rules
on a repository level (similar to `.sops.yaml`). While having a global rules file might be
useful for some (particularly if you're looking to switch from [`sops-nix`](
https://github.com/Mic92/sops-nix)), we wanted to continue to define our rules using Nix expressions which
reside in different directories.
