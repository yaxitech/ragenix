{ rustPlatform
, lib
, stdenv
, darwin
, installShellFiles
, libiconv
, makeWrapper
, nixFlakes
, openssl
, pkg-config
, plugins ? [ ]
}:
let
  cargoTOML = builtins.fromTOML (builtins.readFile ./Cargo.toml);
  # Filter out VCS files and files unrelated to the Rust ragenix package
  filterRustSource = src: with lib; cleanSourceWith {
    filter = cleanSourceFilter;
    src = cleanSourceWith {
      inherit src;
      filter = name: type:
        let pathWithoutPrefix = removePrefix (toString src) name; in
          ! (
            hasPrefix "/.github" pathWithoutPrefix ||
            pathWithoutPrefix == "/.gitignore" ||
            pathWithoutPrefix == "/LICENSE" ||
            pathWithoutPrefix == "/README.md" ||
            pathWithoutPrefix == "/flake.lock" ||
            pathWithoutPrefix == "/flake.nix"
          );
    };
  };
in
rustPlatform.buildRustPackage rec {
  pname = cargoTOML.package.name;
  version = cargoTOML.package.version;
  src = filterRustSource ./.;

  cargoLock.lockFile = ./Cargo.lock;

  preBuildPhases = [ "codeStyleConformanceCheck" ];

  codeStyleConformanceCheck = ''
    header "Checking Rust code formatting"
    cargo fmt -- --check

    header "Running clippy"
    # clippy - use same checkType as check-phase to avoid double building
    if [ "''${cargoCheckType}" != "debug" ]; then
        cargoCheckProfileFlag="--''${cargoCheckType}"
    fi
    argstr="''${cargoCheckProfileFlag} --workspace --all-features --tests "
    cargo clippy -j $NIX_BUILD_CORES \
       $argstr -- \
       -D clippy::pedantic \
       -D warnings
  '';

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
    libiconv
    darwin.Security
  ];

  # Absolute path to the `nix` binary, used in `build.rs`
  RAGENIX_NIX_BIN_PATH = "${nixFlakes}/bin/nix";

  # Run the tests without the "recursive-nix" feature to allow
  # building the package without having a recursive-nix-enabled Nix.
  checkNoDefaultFeatures = true;
  doCheck = true;

  postInstall = ''
    set -euo pipefail

    # Provide a symlink from `agenix` to `ragenix` for compat
    ln -sr "$out/bin/ragenix" "$out/bin/agenix"

    # Stdout of build.rs
    buildOut=$(find "$tmpDir/build" -type f -regex ".*\/ragenix-[a-z0-9]+\/output")

    set +u # required due to `installShellCompletion`'s implementation
    installShellCompletion --bash "$(grep -oP 'RAGENIX_COMPLETIONS_BASH=\K.*' $buildOut)"
    installShellCompletion --zsh  "$(grep -oP 'RAGENIX_COMPLETIONS_ZSH=\K.*' $buildOut)"
    installShellCompletion --fish "$(grep -oP 'RAGENIX_COMPLETIONS_FISH=\K.*' $buildOut)"
  '';

  # Make the plugins available in ragenix' PATH
  postFixup = lib.optionalString (plugins != [ ]) ''
    wrapProgram "$out/bin/ragenix" --prefix PATH : ${lib.strings.makeBinPath plugins}
  '';
}
