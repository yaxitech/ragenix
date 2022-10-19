{ rustPlatform
, lib
, stdenv
, darwin
, installShellFiles
, makeWrapper
, nix
, openssl
, pkg-config
, self ? ./.
, plugins ? [ ]
}:
let
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
            pathWithoutPrefix == "/docs/ragenix.1.html" ||
            pathWithoutPrefix == "/docs/ragenix.1.ronn" ||
            pathWithoutPrefix == "/flake.lock" ||
            pathWithoutPrefix == "/flake.nix"
          );
    };
  };
  rustSource = filterRustSource self;
  cargoTOML = with builtins; fromTOML (readFile ./Cargo.toml);
in
rustPlatform.buildRustPackage {
  pname = cargoTOML.package.name;
  version = cargoTOML.package.version;
  src = rustSource;

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
    darwin.Security
  ];

  # Absolute path to the `nix` binary, used in `build.rs`
  RAGENIX_NIX_BIN_PATH = "${nix}/bin/nix";

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

    installManPage docs/ragenix.1
  '';

  # Make the plugins available in ragenix' PATH
  postFixup = lib.optionalString (plugins != [ ]) ''
    wrapProgram "$out/bin/ragenix" --suffix PATH : ${lib.strings.makeBinPath plugins}
  '';
}
