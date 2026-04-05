{
  description = "Monlin terminal monitor";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs = {self, nixpkgs}: let
    systems = [
      "x86_64-linux"
      "aarch64-linux"
    ];
    eachSystem = nixpkgs.lib.genAttrs systems;
    forSystem = system: let
      pkgs = nixpkgs.legacyPackages.${system};
      staticPkgs = pkgs.pkgsStatic;
      commonArgs = {
        version = "0.6.0";
        src = self;
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [pkgs.installShellFiles];

        meta = with pkgs.lib; {
          description = "Compact terminal monitor for nxu panes and shells";
          license = licenses.mit;
          mainProgram = "monlin";
          platforms = platforms.linux;
        };
      };
      monlin = pkgs.rustPlatform.buildRustPackage (commonArgs // {
        pname = "monlin";
        doCheck = true;
        postInstall = ''
          installShellCompletion --cmd monlin \
            --zsh <("$out/bin/monlin" completion zsh)
        '';
      });
      monlin-static = staticPkgs.rustPlatform.buildRustPackage (commonArgs // {
        pname = "monlin-static";
        doCheck = false;
        nativeBuildInputs = [pkgs.installShellFiles];
        postInstall = ''
          installShellCompletion --cmd monlin \
            --zsh <("$out/bin/monlin" completion zsh)
        '';
      });
      fmt = pkgs.runCommand "monlin-fmt-check" {
        nativeBuildInputs = [pkgs.cargo pkgs.rustfmt];
        src = self;
      } ''
        cp -r "$src" source
        chmod -R +w source
        cd source
        cargo fmt --check
        touch "$out"
      '';
      clippy = pkgs.rustPlatform.buildRustPackage (commonArgs // {
        pname = "monlin-clippy";
        doCheck = true;
        nativeBuildInputs = [pkgs.clippy];
        buildPhase = ''
          runHook preBuild
          touch monlin-clippy
          runHook postBuild
        '';
        checkPhase = ''
          runHook preCheck
          cargo clippy --offline --workspace --all-targets
          runHook postCheck
        '';
        installPhase = ''
          runHook preInstall
          touch "$out"
          runHook postInstall
        '';
      });
      tests = pkgs.rustPlatform.buildRustPackage (commonArgs // {
        pname = "monlin-test";
        doCheck = true;
        buildPhase = ''
          runHook preBuild
          touch monlin-test
          runHook postBuild
        '';
        checkPhase = ''
          runHook preCheck
          cargo test --offline --quiet
          runHook postCheck
        '';
        installPhase = ''
          runHook preInstall
          touch "$out"
          runHook postInstall
        '';
      });
    in {
      packages = {
        default = monlin;
        monlin = monlin;
        monlin-static = monlin-static;
      };
      checks = {
        default = monlin;
        fmt = fmt;
        clippy = clippy;
        test = tests;
      };
      devShells = {
        default = pkgs.mkShell {
          inputsFrom = [
            monlin
            clippy
          ];
          packages = with pkgs; [
            cargo-llvm-cov
            llvmPackages_21.llvm
            rustfmt
            rust-analyzer
          ];
          shellHook = ''
            if [ -d .git ]; then
              git config core.hooksPath .githooks
            fi
          '';
        };
      };
    };
  in {
    packages = eachSystem (system: (forSystem system).packages);
    checks = eachSystem (system: (forSystem system).checks);
    devShells = eachSystem (system: (forSystem system).devShells);
  };
}
