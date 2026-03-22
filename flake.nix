{
  description = "Calendar sheet generator — adds 14-week sheets to an Excel workbook";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      apps = forAllSystems (pkgs:
        let
          python = pkgs.python3.withPackages (ps: [ ps.openpyxl ]);
          script = pkgs.writeShellScript "gen-calendar" ''
            exec ${python}/bin/python3 ${./gen_calendar.py} "$@"
          '';
        in {
          gen-calendar = { type = "app"; program = toString script; };
          default = { type = "app"; program = toString script; };
        });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (pkgs.python3.withPackages (ps: [ ps.openpyxl ]))
          ];
        };
      });
    };
}
