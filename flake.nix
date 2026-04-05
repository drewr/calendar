{
  description = "Calendar tools — sheet generator and Google Calendar search";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      packages = forAllSystems (pkgs: {
        gcal-search = pkgs.rustPlatform.buildRustPackage {
          pname = "gcal-search";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
        };
      });

      apps = forAllSystems (pkgs:
        let
          python = pkgs.python3.withPackages (ps: [ ps.openpyxl ]);
          script = pkgs.writeShellScript "gen-calendar" ''
            exec ${python}/bin/python3 ${./gen_calendar.py} "$@"
          '';
        in {
          gen-calendar = { type = "app"; program = toString script; };
          gcal-search = { type = "app"; program = "${self.packages.${pkgs.system}.gcal-search}/bin/gcal-search"; };
          default = { type = "app"; program = "${self.packages.${pkgs.system}.gcal-search}/bin/gcal-search"; };
        });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (pkgs.python3.withPackages (ps: [ ps.openpyxl ]))
            pkgs.rustc
            pkgs.cargo
            pkgs.gcalcli
          ];
        };
      });
    };
}
