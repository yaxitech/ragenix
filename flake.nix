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
    agenix = {
      url = "github:ryantm/agenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, agenix }:
    let
      cargoTOML = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      name = cargoTOML.package.name;

      lib = nixpkgs.lib;

      # Recursively merge a list of attribute sets. Following elements take
      # precedence over previous elements if they have conflicting keys.
      recursiveMerge = with lib; foldl recursiveUpdate { };
      eachSystem = systems: f: flake-utils.lib.eachSystem systems (system: f (pkgsFor system));
      defaultSystems = flake-utils.lib.defaultSystems;
      eachDefaultSystem = eachSystem defaultSystems;
      eachLinuxSystem = eachSystem (lib.filter (lib.hasSuffix "-linux") flake-utils.lib.defaultSystems);

      pkgsFor = system: import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlay self.overlay ];
      };
    in
    recursiveMerge [
      #
      # COMMON OUTPUTS FOR ALL SYSTEMS
      #
      (eachDefaultSystem (pkgs:
        let
          rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain;

          buildRustPackage = (pkgs.makeRustPlatform {
            cargo = rust;
            rustc = rust;
          }).buildRustPackage;

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

          ragenix = { plugins ? [ ] }: buildRustPackage rec {
            pname = name;
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
            nativeBuildInputs = with pkgs; [
              pkg-config
              installShellFiles
            ] ++ lib.optionals (plugins != [ ]) [
              makeWrapper
            ];

            # runtime dependencies
            buildInputs = with pkgs; [
              openssl
              nixFlakes
            ] ++ lib.optionals stdenv.isDarwin [
              libiconv
              darwin.Security
            ] ++ plugins;

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
          };
        in
        rec {
          # `nix build`
          packages.${name} = pkgs.callPackage ragenix { };
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

          checks.tests-recursive-nix = packages.${name}.overrideAttrs (oldAttrs: {
            name = "tests-recursive-nix";
            cargoCheckFeatures = [ "recursive-nix" ];
            requiredSystemFeatures = [ "recursive-nix" ];
            checkInputs = [ pkgs.nixFlakes ];
            # No need to run the formatting checks again
            codeStyleConformanceCheck = "true";
          });

          checks.rekey = pkgs.runCommand "run-rekey"
            {
              buildInputs = [ pkgs.nixFlakes ];
              requiredSystemFeatures = [ "recursive-nix" ];
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

          checks.shell-completion = pkgs.runCommand "check-shell-completions" { } ''
            set -euo pipefail

            if [[ ! -e "${pkgs.ragenix}/share/bash-completion" ]]; then
              echo 'Failed to install bash completions'
            elif [[ ! -e "${pkgs.ragenix}/share/zsh" ]]; then
              echo 'Failed to install zsh completions'
            elif [[ ! -e "${pkgs.ragenix}/share/fish" ]]; then
              echo 'Failed to install fish completions'
            else
              echo '${name} shell completions installed successfully'
              mkdir $out
              exit 0
            fi

            exit 1
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

          checks.metadata = pkgs.runCommand "check-metadata" { } ''
            set -euo pipefail

            flakeDescription=${lib.escapeShellArg (import ./flake.nix).description}
            packageDescription=${lib.escapeShellArg cargoTOML.package.description}
            if [[ "$flakeDescription" != "$packageDescription" ]]; then
              echo 'The descriptions given in flake.nix and Cargo.toml do not match'
              exit 1
            fi

            flakePackageName=${pkgs.ragenix.pname}
            cargoName=${cargoTOML.package.name}
            if [[ "$flakePackageName" != "$cargoName" ]]; then
              echo 'The package name given in flake.nix and Cargo.toml do not match'
              exit 1
            fi

            echo 'All metadata checks completed successfully'
            mkdir $out # success
          '';

          checks.age-plugin =
            let
              rageExamplePlugin = pkgs.rage.overrideAttrs (old: rec {
                pname = "age-plugin-unencrypted";
                doCheck = false;
                cargoBuildFlags = [ "--example" pname ];
                installPhase = ''
                  set -euo pipefail
                  find target/**/release/examples -name ${pname} \
                    -exec install -D {} $out/bin/${pname} \;
                '';
              });
              plugins = [ rageExamplePlugin ];
              ragenixWithPlugins = pkgs.ragenix.override { inherit plugins; };
              pluginsSearchPath = lib.strings.makeBinPath plugins;
            in
            pkgs.runCommand "age-plugin"
              {
                buildInputs = with pkgs; [ nixFlakes rage ragenixWithPlugins ];
                requiredSystemFeatures = lib.optionals (!pkgs.stdenv.isDarwin) [ "recursive-nix" ];
              }
              ''
                set -euo pipefail
                cp -r '${./.}/example/.' "$TMPDIR"
                cd "$TMPDIR"

                # Encrypt with ragenix
                echo 'wurzelpfropf' | ragenix --rules ./secrets-plugin.nix --editor - --edit unencrypted.age

                # Decrypt with rage
                decrypted="$(PATH="${pluginsSearchPath}:$PATH" rage -i '${./example/keys/example_plugin_key.txt}' -d unencrypted.age)"
                if [[ "$decrypted" != "wurzelpfropf" ]]; then
                  echo 'Unexpected value for decryption with plugin'
                  exit 1
                fi

                # Rekey
                ragenix --rules ./secrets-plugin.nix -i '${./example/keys/example_plugin_key.txt}' --rekey

                mkdir $out # success
              '';

          # `nix develop`
          devShell = pkgs.mkShell {
            name = "${name}-dev-shell";

            nativeBuildInputs = [ rust ] ++ (with pkgs; [ pkg-config openssl rust-analyzer ]);

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
      (eachLinuxSystem (pkgs: {
        checks.nixos-module =
          let
            pythonTest = import ("${nixpkgs}/nixos/lib/testing-python.nix") { inherit (pkgs) system; };
            secretsConfig = import ./example/secrets-configuration.nix;
            secretPath = "/run/agenix/github-runner.token";
            ageIdentitiesConfig = { lib, ... }: {
              # XXX: This is insecure and copies your private key plaintext to the Nix store
              #      NEVER DO THIS IN YOUR CONFIG!
              age.identityPaths = lib.mkForce [ ./example/keys/id_ed25519 ];
            };
          in
          pythonTest.makeTest {
            machine.imports = [
              self.nixosModules.age
              secretsConfig
              ageIdentitiesConfig
            ];

            testScript = ''
              machine.start()
              machine.wait_for_unit("multi-user.target")
              machine.succeed('test -e "${secretPath}"')
              machine.succeed(
                  '[[ "$(cat "${secretPath}")" == "wurzelpfropf!" ]] || exit 1'
              )
              machine.succeed(
                  '[[ "$(stat -c "%a" "${secretPath}")" == "400"  ]] || exit 1'
              )
              machine.succeed(
                  '[[ "$(stat -c "%U" "${secretPath}")" == "root" ]] || exit 1'
              )
              machine.succeed(
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
