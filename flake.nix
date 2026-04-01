{
  description = "Monlin CPU monitor";

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
        pname = "nxu-cpu";
        version = "0.1.0";

        src = self;
        cargoLock.lockFile = ./Cargo.lock;

        meta = with pkgs.lib; {
          description = "Tiny CPU monitor for nxu tmux side panes";
          license = licenses.mit;
          mainProgram = "nxu-cpu";
          platforms = platforms.linux;
        };
      };
    in {
      default = monlin;
      nxu-cpu = monlin;
    });
  };
}
