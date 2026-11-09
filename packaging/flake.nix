{
  description = "Islet — Dynamic Island / Floating Media HUD for Wayland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "islet";
            version = "0.1.0";
            src = ./..;
            cargoLock.lockFile = ../Cargo.lock;

            nativeBuildInputs = with pkgs; [ pkg-config wrapGAppsHook4 ];
            buildInputs = with pkgs; [ gtk4 gtk4-layer-shell libadwaita cairo glib ];
          };
        });
    };
}
