{
  description = "The QUIC Copier (qcp) is an experimental high-performance remote file copy utility for long-distance internet connections.";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-25.05";
  };

  outputs =
    { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;
      forAllSystems = lib.genAttrs lib.systems.flakeExposed;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        rec {
          default = qcp;
          qcp = pkgs.callPackage ./default.nix { };
        }
      );

      apps = forAllSystems (system: rec {
        default = qcp;
        qcp = {
          type = "app";
          program = "${lib.getBin self.packages.${system}.qcp}/bin/qcp";
        };
      });
    };
}
