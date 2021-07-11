{
  description = "A rust drop-in replacement for agenix";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils = {
      url = "github:numtide/flake-utils";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
    naersk = {
      url = "github:yaxitech/naersk/aarch64-darwin";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    agenix = {
      url = "github:ryantm/agenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, naersk, agenix }:
    let
      cargoTOML = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      name = cargoTOML.package.name;

      lib = import (nixpkgs + "/lib");

      # Recursively merge a list of attribute sets. Following elements take
      # precedence over previous elements if they have conflicting keys.
      recursiveMerge = with lib; foldl recursiveUpdate { };
      defaultSystems = flake-utils.lib.defaultSystems ++ [ "aarch64-darwin" ];
      eachDefaultSystem = flake-utils.lib.eachSystem (defaultSystems);
      eachLinuxSystem = flake-utils.lib.eachSystem (lib.filter (lib.hasSuffix "-linux") flake-utils.lib.defaultSystems);

      pkgsFor = system: import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlay self.overlay ];
      };
    in
    recursiveMerge [
      #
      # COMMON OUTPUTS FOR ALL SYSTEMS
      #
      (eachDefaultSystem (system:
        let
          pkgs = pkgsFor system;

          rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain;

          naersk-lib = naersk.lib."${system}".override {
            cargo = rust;
            rustc = rust;
          };
        in
        rec {
          # `nix build`
          packages.${name} = naersk-lib.buildPackage {
            pname = name;
            root = ./.;

            nativeBuildInputs = with pkgs; [
              pkg-config
              installShellFiles
            ];

            requiredSystemFeatures = lib.optionals (!pkgs.stdenv.isDarwin) [ "recursive-nix" ];

            buildInputs = with pkgs; [
              openssl
              nixFlakes
            ] ++ lib.optionals stdenv.isDarwin [
              libiconv
              darwin.Security
            ];

            doCheck = true;

            cargoTestCommands = x: x ++ [
              # clippy
              ''cargo clippy --all --all-features --tests -- -D clippy::pedantic''
              # rustfmt
              ''cargo fmt -- --check''
            ];

            overrideMain = _: {
              postInstall = ''
                # Provide a symlink from `agenix` to `ragenix` for compat
                ln -sr "$out/bin/ragenix" "$out/bin/agenix"

                installShellCompletion --bash target/release/build/ragenix-*/out/ragenix.bash
                installShellCompletion --zsh  target/release/build/ragenix-*/out/_ragenix
                installShellCompletion --fish target/release/build/ragenix-*/out/ragenix.fish
              '';
            };
          };
          defaultPackage = packages.${name};

          # `nix run`
          apps.${name} = flake-utils.lib.mkApp {
            drv = packages.${name};
          };
          defaultApp = apps.${name};

          # nix `check`
          checks.nixpkgs-fmt = pkgs.runCommand "check-nix-format" { } ''
            ${pkgs.nixpkgs-fmt}/bin/nixpkgs-fmt --check ${./.}
            mkdir $out #sucess
          '';

          checks.rekey = pkgs.runCommand "run-rekey"
            {
              buildInputs = [ pkgs.nixFlakes ];
              requiredSystemFeatures = lib.optionals (!pkgs.stdenv.isDarwin) [ "recursive-nix" ];
            }
            ''
              set -euo pipefail
              cp -r '${./.}/example/.' "$TMPDIR"
              chmod 600 *.age
              cd "$TMPDIR"

              ln -s "${./example/keys}" "$TMPDIR/.ssh"
              export HOME="$TMPDIR"

              ${pkgs.ragenix}/bin/ragenix --rekey
              ${pkgs.agenix}/bin/agenix   --rekey

              mkdir "$out"
            '';

          checks.schema = pkgs.runCommand "emit-schema" { } ''
            set -euo pipefail
            ${pkgs.ragenix}/bin/ragenix --schema > "$TMPDIR/agenix.schema.json"
            ${pkgs.diffutils}/bin/diff '${./src/ragenix/agenix.schema.json}' "$TMPDIR/agenix.schema.json"
            echo "Schema matches"
            mkdir "$out"
          '';

          checks.agenix-symlink = pkgs.runCommand "check-agenix-symlink" { } ''
            set -euo pipefail
            agenix="$(readlink -f '${pkgs.ragenix}/bin/agenix')"
            ragenix="$(readlink -f '${pkgs.ragenix}/bin/ragenix')"

            if [[ "$agenix" == "$ragenix" ]]; then
              echo "agenix symlinked to ragenix"
              mkdir $out
            else
              echo "agenix doesn't resolve to ragenix"
              echo "agenix: $agenix"
              echo "ragenix: $ragenix"
              exit 1
            fi
          '';

          checks.decrypt-with-age = pkgs.runCommand "decrypt-with-age" { } ''
            set -euo pipefail

            files=('${./example/root.passwd.age}' '${./example/github-runner.token.age}')

            for file in ''${files[@]}; do
              rage_output="$(${pkgs.rage}/bin/rage -i '${./example/keys/id_ed25519}' -d "$file")"
              age_output="$(${pkgs.age}/bin/age    -i '${./example/keys/id_ed25519}' -d "$file")"

              if [[ "$rage_output" != "$age_output" ]]; then
                printf 'Decrypted plaintext for %s differs for rage and age' "$file"
                exit 1
              fi
            done

            echo "rage and age decryption of examples successful and equal"
            mkdir $out
          '';

          # `nix develop`
          devShell = pkgs.mkShell {
            name = "${name}-dev-shell";

            nativeBuildInputs = [ rust pkgs.pkg-config pkgs.openssl ];

            buildInputs = with pkgs; lib.optionals stdenv.isDarwin [
              libiconv
              darwin.Security
            ];

            RUST_SRC_PATH = "${rust}/lib/rustlib/src/rust/library";

            shellHook = ''
              export PATH=$PWD/target/debug:$PATH
            '';
          };
        })
      )
      #
      # CHECKS SPECIFIC TO LINUX SYSTEMS
      #
      (eachLinuxSystem (system: {
        checks.nixos-module =
          let
            pythonTest = import (nixpkgs + "/nixos/lib/testing-python.nix") { system = "x86_64-linux"; };
            secretsConfig = import ./example/secrets-configuration.nix;
            ageSshKeysConfig = { lib, ... }: {
              # XXX: This is insecure and copies your private key plaintext to the Nix store
              #      NEVER DO THIS IN YOUR CONFIG!
              age.sshKeyPaths = lib.mkForce [
                ./example/keys/id_ed25519
              ];
            };
            secretPath = "/run/secrets/github-runner.token";
          in
          with pythonTest; makeTest {
            nodes = {
              client = { ... }: {
                imports = [
                  self.nixosModules.age
                  secretsConfig
                  ageSshKeysConfig
                ];
                nixpkgs.overlays = [ self.overlay ];
              };
            };

            testScript = ''
              start_all()
              client.wait_for_unit("multi-user.target")
              client.succeed('test -e "${secretPath}"')
              client.succeed(
                  '[[ "$(cat "${secretPath}")" == "wurzelpfropf!" ]] || exit 1'
              )
              client.succeed(
                  '[[ "$(stat -c "%a" "${secretPath}")" == "400"  ]] || exit 1'
              )
              client.succeed(
                  '[[ "$(stat -c "%U" "${secretPath}")" == "root" ]] || exit 1'
              )
              client.succeed(
                  '[[ "$(stat -c "%G" "${secretPath}")" == "root" ]] || exit 1'
              )
            '';
          };
      })
      )
      #
      # SYSTEM-INDEPENDENT OUTPUTS
      #
      {
        # Passthrough the agenix NixOS module
        inherit (agenix) nixosModules;

        # Overlay to add ragenix and replace agenix
        overlay = final: prev: rec {
          ragenix = self.packages.${prev.system}.ragenix;
          agenix = ragenix;
        };
      }
    ];
}
