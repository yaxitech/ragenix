{
  description = "A rust drop-in replacement for agenix";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    agenix = {
      url = "github:ryantm/agenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane, agenix }:
    let
      cargoTOML = lib.importTOML ./Cargo.toml;
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
        overlays = [
          rust-overlay.overlays.default
          self.overlays.default
          (final: prev: {
            rustToolchain = final.rust-bin.fromRustupToolchainFile ./rust-toolchain;
            craneLib = (crane.mkLib prev).overrideToolchain final.rustToolchain;
          })
        ];
        config = { allowAliases = false; };
      };
    in
    recursiveMerge [
      #
      # COMMON OUTPUTS FOR ALL SYSTEMS
      #
      (eachDefaultSystem (pkgs: rec {
        # `nix build`
        packages.${name} = pkgs.callPackage ./default.nix { };
        packages.default = packages.${name};

        # `nix run`
        apps.${name} = flake-utils.lib.mkApp {
          drv = packages.${name};
        };
        apps.default = apps.${name};

        # Regenerate the roff and HTML manpages and commit the changes, if any
        apps.update-manpage = flake-utils.lib.mkApp {
          drv = pkgs.writeShellApplication {
            name = "update-manpage";
            runtimeInputs = with pkgs; [ ronn git ];
            text = ''
              ronn docs/ragenix.1.ronn

              git diff --quiet -- docs/ragenix.1*          || changes=1
              git diff --staged --quiet -- docs/ragenix.1* || changes=1

              if [[ -z "''${changes:-}" ]]; then
                echo 'No changes to commit'
              else
                echo 'Committing changes'
                git commit -m "docs: update manpage" docs/ragenix.1*
              fi
            '';
          };
        };

        # nix `check`
        checks.clippy = pkgs.craneLib.cargoClippy {
          inherit (self.packages.${pkgs.stdenv.system}.${name}) src cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets --tests --examples -- -D clippy::pedantic -D warnings";
          doInstallCargoArtifacts = false;
          RAGENIX_NIX_BIN_PATH = lib.getExe pkgs.nix;
        };

        checks.fmt = pkgs.craneLib.cargoFmt {
          inherit (self.packages.${pkgs.stdenv.system}.${name}) src cargoArtifacts;
          doInstallCargoArtifacts = false;
          RAGENIX_NIX_BIN_PATH = lib.getExe pkgs.nix;
        };

        checks.nixpkgs-fmt = pkgs.runCommand "check-nix-format" { } ''
          ${pkgs.nixpkgs-fmt}/bin/nixpkgs-fmt --check ${self}
          mkdir $out #sucess
        '';

        checks.schema = pkgs.runCommand "emit-schema" { } ''
          set -euo pipefail
          ${pkgs.ragenix}/bin/ragenix --schema > "$TMPDIR/agenix.schema.json"
          ${pkgs.diffutils}/bin/diff '${self}/src/ragenix/agenix.schema.json' "$TMPDIR/agenix.schema.json"
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

        checks.shell-files = pkgs.runCommand "check-shell-files" { } ''
          set -euo pipefail

          if [[ ! -e "${pkgs.ragenix}/share/bash-completion" ]]; then
            echo 'Failed to install bash completions'
          elif [[ ! -e "${pkgs.ragenix}/share/zsh" ]]; then
            echo 'Failed to install zsh completions'
          elif [[ ! -e "${pkgs.ragenix}/share/fish" ]]; then
            echo 'Failed to install fish completions'
          elif [[ ! -e "${pkgs.ragenix}/share/man/man1/ragenix.1.gz" ]]; then
            echo 'Failed to install manpage'
          else
            echo '${name} shell files installed successfully'
            mkdir $out
            exit 0
          fi

          exit 1
        '';

        checks.decrypt-with-age = pkgs.runCommand "decrypt-with-age" { } ''
          set -euo pipefail

          # Required to prevent a panic in the locale_config crate
          # https://github.com/yaxitech/ragenix/issues/76
          ${lib.optionalString pkgs.stdenv.isDarwin ''export LANG="en_US.UTF-8"''}

          files=('${self}/example/root.passwd.age' '${self}/example/github-runner.token.age')

          for file in ''${files[@]}; do
            rage_output="$(${pkgs.rage}/bin/rage -i '${self}/example/keys/id_ed25519' -d "$file")"
            age_output="$(${pkgs.age}/bin/age    -i '${self}/example/keys/id_ed25519' -d "$file")"

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

          flakeDescription=${lib.escapeShellArg (import "${self}/flake.nix").description}
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

        # Make sure the roff and HTML manpages are up-to-date
        checks.manpage = pkgs.runCommand "check-manpage"
          {
            buildInputs = with pkgs; [ ronn diffutils ];
          } ''
          set -euo pipefail

          echo "Generate roff and HTML manpage"
          ln -s ${self}/docs/ragenix.1.ronn .
          ronn ragenix.1.ronn

          echo "roff: strip date"
          tail -n '+5' ${self}/docs/ragenix.1 > ragenix.1.old
          tail -n '+5'              ragenix.1 > ragenix.1.new

          diff -u ragenix.1.{old,new} > diff \
            || (printf "roff: error, not up-to-date:\n\n%s\n" "$(cat diff)" >&2 && exit 1)

          echo "html: strip date"
          grep -v "<li class='tc'>" ${self}/docs/ragenix.1.html > ragenix.1.html.old
          grep -v "<li class='tc'>"              ragenix.1.html > ragenix.1.html.new

          diff -u ragenix.1.html.{old,new} > diff \
            || (printf "html: error, not up-to-date:\n\n%s\n" "$(cat diff)" >&2 && exit 1)

          echo 'Manpage is up-to-date'
          mkdir -p $out
        '';

        # `nix develop`
        devShells.default = pkgs.mkShell {
          name = "${name}-dev-shell";

          inputsFrom = [ self.packages."${pkgs.stdenv.system}".ragenix ];

          nativeBuildInputs = with pkgs; [
            ronn
            rust-analyzer
          ];

          RAGENIX_NIX_BIN_PATH = lib.getExe pkgs.nix;

          RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";

          shellHook = ''
            export PATH=$PWD/target/debug:$PATH
          '';
        };
      })
      )
      #
      # PACKAGES SPECIFIC TO LINUX SYSTEMS
      #
      (eachLinuxSystem (pkgs: {
        packages.ragenix-static = self.packages.${pkgs.stdenv.system}.ragenix.override {
          target = pkgs.pkgsStatic.stdenv.targetPlatform.rust.cargoShortTarget;
        };
        packages.default = self.packages.${pkgs.stdenv.system}.ragenix-static;
      }))
      #
      # CHECKS SPECIFIC TO LINUX SYSTEMS
      #
      (eachLinuxSystem (pkgs: {
        checks.nixos-module =
          let
            pythonTest = import ("${nixpkgs}/nixos/lib/testing-python.nix") {
              inherit (pkgs.stdenv.hostPlatform) system;
            };
            secretsConfig = import "${self}/example/secrets-configuration.nix";
            secretPath = "/run/agenix/github-runner.token";
            ageIdentitiesConfig = { lib, ... }: {
              # XXX: This is insecure and copies your private key plaintext to the Nix store
              #      NEVER DO THIS IN YOUR CONFIG!
              age.identityPaths = lib.mkForce [ "${self}/example/keys/id_ed25519" ];
            };
          in
          pythonTest.runTest {
            name = "ragenix-nixos-module";

            nodes.machine.imports = [
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

        checks.tests-recursive-nix = (pkgs.ragenix.override {
          enableRecursiveNixTests = true;
        }).overrideAttrs (_: {
          name = "tests-recursive-nix";
        });

        checks.rekey = pkgs.runCommand "run-rekey"
          {
            requiredSystemFeatures = [ "recursive-nix" ];
          }
          ''
            set -euo pipefail
            cp -r '${self}/example/.' "$TMPDIR"
            chmod 600 *.age

            ln -s "${self}/example/keys" "$TMPDIR/.ssh"
            export HOME="$TMPDIR"

            ${pkgs.ragenix}/bin/ragenix --rekey
            ${pkgs.agenix}/bin/agenix   --rekey

            mkdir "$out"
          '';

        checks.age-plugin =
          let
            rageExamplePlugin = pkgs.rage.overrideAttrs (_: rec {
              pname = "age-plugin-unencrypted";
              doCheck = false;
              cargoBuildFlags = [ "--example" pname ];
              installPhase = ''
                set -euo pipefail
                find target/**/release/examples -name ${pname} \
                  -exec install -D {} $out/bin/${pname} \;
                # strip.sh uses unset variables
                set +u
              '';
            });
            plugins = [ rageExamplePlugin ];
            ragenixWithPlugins = pkgs.ragenix.override { inherit plugins; };
            pluginsSearchPath = lib.strings.makeBinPath plugins;
          in
          pkgs.runCommand "age-plugin"
            {
              buildInputs = [ pkgs.rage ragenixWithPlugins ];
              requiredSystemFeatures = [ "recursive-nix" ];
            }
            ''
              set -euo pipefail
              cp -r '${self}/example/.' "$TMPDIR"
              cd "$TMPDIR"

              # Encrypt with ragenix
              echo 'wurzelpfropf' | ragenix --rules ./secrets-plugin.nix --editor - --edit unencrypted.age

              # Decrypt with rage
              decrypted="$(PATH="${pluginsSearchPath}:$PATH" rage -i '${self}/example/keys/example_plugin_key.txt' -d unencrypted.age)"
              if [[ "$decrypted" != "wurzelpfropf" ]]; then
                echo 'Unexpected value for decryption with plugin'
                exit 1
              fi

              # Rekey
              ragenix --rules ./secrets-plugin.nix -i '${self}/example/keys/example_plugin_key.txt' --rekey

              mkdir $out # success
            '';
      })
      )
      #
      # SYSTEM-INDEPENDENT OUTPUTS
      #
      {
        # Passthrough the agenix NixOS, Darwin and Home Manager modules
        inherit (agenix) nixosModules darwinModules homeManagerModules;

        # Overlay to add ragenix and replace agenix
        overlays.default = _final: prev: rec {
          ragenix = self.packages.${prev.stdenv.hostPlatform.system}.default;
          agenix = ragenix;
        };
      }
    ];
}
