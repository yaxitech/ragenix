# ragenix

`ragenix` provides age-encrypted secrets for NixOS systems which live in the Nix store
and are decrypted on system activation. Using `ragenix` to create, edit and rekey secrets
is possible on any system which has Nix installed—with particular support for NixOS and macOS.

`ragenix` is a drop-in replacement for [@ryantm](https://github.com/ryantm)'s
[`agenix`](https://github.com/ryantm/agenix) written in Rust. It aims at being fully compatible
with its flake while offering more robust command line parsing, additional validation logic
and solid tests.

**As opposed to `agenix`, `ragenix` only strives for supporting Nix Flakes**.

## Installation

As `ragenix` seeks to replace `agenix` without breaking compatability, getting started with age-encrypted
secrets or switching from `agenix` to `ragenix` is easy: just follow the original [instructions from `agenix`](
https://github.com/ryantm/agenix#installation) while replacing references to
`github.com/ryantm/agenix` with `github.com/yaxitech/ragenix`. Everything else should remain the
same as the `ragenix` package provides aliases for a) an `agenix` package and b) the `agenix` binary.
The flake also exposes a NixOS module which is passed through from the `agenix` flake.

## Create, edit and rekey secrets

`ragenix` resembles the command line options and behavior of `agenix`:

* By default, `ragenix` looks for a Nix rules file in `./secrets.nix`. You may change this path by setting the `RULES`
  environment variable accordingly. As a `ragenix` addon, you may also use the `--rules` command line option.
* The Nix rules reference age-encrypted files relative to the rules file. For example, a `./secrets/secrets.nix` file with the
  following content would instruct `ragenix` to look for `mysecret.age` in `./secrets/`: 
  `{ "mysecret.age".publicKeys = "age1hunh4g..."; }`.
* If a file given in the secrets rules does not exist:
  - `--edit`: the file is created prior to opening it for editing.
  - `--rekey`: the file is ignored.
* `ragenix` opens a file for editing using `$EDITOR`. Again, you may use `--editor` instead of the
  environment variable.
* Prior to editing/rekeying, `ragenix` verifies the validity of the rules file using [this JSON schema](
  ./src/agenix.schema.json). The schema is also available to third party applications with
  the `--schema` command line switch. For an example rules file, please refer to the [`agenix` README](
  https://github.com/ryantm/agenix#tutorial) or take a look at the files in the [`example`](./example) directory
  of this repository.

## Contributions

We'd love to see PRs from you! Please consider the following guidelines:

- `ragenix` stays compatible to `agenix`. Please make sure your contributions
  don't introduce breaking changes.
- The secrets configuration happens through a Nix configuration.
- New features should support both NixOS and macOS, if applicable.

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

The [`agenix-rs`](https://github.com/cole-h/agenix-rs) project is quite similar to ragenix. In fact, it
served as an inspiration (thanks!). Both projects have in common that they aim
at replacing the fragile shell script with a version written in Rust. In contrast to `ragenix`, however,
`agenix-rs` is not compatible to the original `agenix`. It uses a TOML configuration file to declare rules
on a repository level (similar to `.sops.yaml`). While having a global rules file might be
useful for some (particularly if you're looking to switch from [`sops-nix`](
https://github.com/Mic92/sops-nix)), we wanted to continue to define our rules using Nix expressions which
reside in different directories.
