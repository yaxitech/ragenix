{ craneLib
, lib
, stdenv
, darwin
, installShellFiles
, makeWrapper
, nix
, openssl
, pkg-config
, plugins ? [ ]
  # Allowing running the tests without the "recursive-nix" feature to allow
  # building the package without having a recursive-nix-enabled Nix.
, enableRecursiveNixTests ? false
}:
let
  commonArgs = {
    src = lib.cleanSourceWith rec {
      src = craneLib.path ./.;
      filter = path: type:
        let pathWithoutPrefix = lib.removePrefix (toString src) path; in
        lib.hasPrefix "/docs/" pathWithoutPrefix ||
        lib.hasPrefix "/example/" pathWithoutPrefix ||
        lib.hasPrefix "/src/" pathWithoutPrefix ||
        craneLib.filterCargoSources path type;
    };

    # build dependencies
    nativeBuildInputs = [
      pkg-config
      installShellFiles
    ] ++ lib.optionals (plugins != [ ]) [
      makeWrapper
    ];

    # runtime dependencies
    buildInputs = [
      openssl
    ] ++ lib.optionals stdenv.isDarwin [
      darwin.Security
    ];

    # Absolute path to the `nix` binary, used in `build.rs`
    RAGENIX_NIX_BIN_PATH = lib.getExe nix;
  };
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (commonArgs // {
  inherit cargoArtifacts;

  cargoTestExtraArgs = lib.optionalString (!enableRecursiveNixTests) "--no-default-features";
  requiredSystemFeatures = lib.optionals enableRecursiveNixTests [ "recursive-nix" ];

  postInstall = ''
    set -euo pipefail

    # Provide a symlink from `agenix` to `ragenix` for compat
    ln -sr "$out/bin/ragenix" "$out/bin/agenix"

    # Stdout of build.rs
    buildOut=$(grep -m 1 -Rl 'RAGENIX_COMPLETIONS_BASH=/' target/ | head -n 1)
    printf "found build script output at %s\n" "$buildOut"

    set +u # required due to `installShellCompletion`'s implementation
    installShellCompletion --bash "$(grep -oP 'RAGENIX_COMPLETIONS_BASH=\K.+' "$buildOut")"
    installShellCompletion --zsh  "$(grep -oP 'RAGENIX_COMPLETIONS_ZSH=\K.+'  "$buildOut")"
    installShellCompletion --fish "$(grep -oP 'RAGENIX_COMPLETIONS_FISH=\K.+' "$buildOut")"

    installManPage docs/ragenix.1
  '';

  # Make the plugins available in ragenix' PATH
  postFixup = lib.optionalString (plugins != [ ]) ''
    wrapProgram "$out/bin/ragenix" --suffix PATH : ${lib.strings.makeBinPath plugins}
  '';

  passthru.cargoArtifacts = cargoArtifacts;
})
