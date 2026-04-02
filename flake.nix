{
  description = "Monlin terminal monitor";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs = {self, nixpkgs}: let
    eachSystem = nixpkgs.lib.genAttrs [
      "x86_64-linux"
      "aarch64-linux"
    ];
  in {
    packages = eachSystem (system: let
      pkgs = nixpkgs.legacyPackages.${system};
      monlin = pkgs.rustPlatform.buildRustPackage {
        pname = "monlin";
        version = "0.1.0";

        src = self;
        cargoLock.lockFile = ./Cargo.lock;
        doCheck = true;

        meta = with pkgs.lib; {
          description = "Compact terminal monitor for nxu panes and shells";
          license = licenses.mit;
          mainProgram = "monlin";
          platforms = platforms.linux;
        };
      };
    in {
      default = monlin;
      monlin = monlin;
    });
    checks = eachSystem (system: {
      default = self.packages.${system}.default;
    });
  };
}
