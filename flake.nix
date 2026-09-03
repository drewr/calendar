{
  description = "Calendar tools — sheet generator, Google Calendar search and planning";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      packages = forAllSystems (pkgs:
        let
          gcal-search-unwrapped = pkgs.rustPlatform.buildRustPackage {
            pname = "gcal-search";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
          };
        in {
          gcal-search = pkgs.writeShellScriptBin "gcal-search" ''
            export PATH="${pkgs.gcalcli}/bin:$PATH"
            exec ${gcal-search-unwrapped}/bin/gcal-search "$@"
          '';
          gcal-plan = pkgs.writeShellScriptBin "gcal-plan" ''
            export PATH="${pkgs.gcalcli}/bin:$PATH"
            exec ${gcal-search-unwrapped}/bin/gcal-plan "$@"
          '';
          gen-calendar = gcal-search-unwrapped;
        });

      apps = forAllSystems (pkgs: {
        gen-calendar = { type = "app"; program = "${self.packages.${pkgs.system}.gen-calendar}/bin/gen-calendar"; };
        gcal-search = { type = "app"; program = "${self.packages.${pkgs.system}.gcal-search}/bin/gcal-search"; };
        gcal-plan = { type = "app"; program = "${self.packages.${pkgs.system}.gcal-plan}/bin/gcal-plan"; };
        default = { type = "app"; program = "${self.packages.${pkgs.system}.gcal-search}/bin/gcal-search"; };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.rustc
            pkgs.cargo
            pkgs.gcalcli
          ];
        };
      });
    };
}
