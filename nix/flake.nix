{
  description = "The QUIC Copier (qcp) is an experimental high-performance remote file copy utility for long-distance internet connections.";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-24.11";

    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    fenix.inputs.rust-analyzer-src.follows = "";

    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    ...
  }@inputs: let
    inherit (inputs.nixpkgs) lib;
    forAllSystems = lib.genAttrs lib.systems.flakeExposed;
  in {
    packages = forAllSystems (
      system:
        with lib; let
          pkgs = inputs.nixpkgs.legacyPackages.${system};
          rustToolchain = with inputs.fenix.packages.${system}; with stable; combine [rustc rust-src cargo];
          crane = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;
          src = fileset.toSource {
            root = ../.;
            fileset = fileset.unions [
              ../Cargo.lock
              (fileset.fromSource (cleanSourceWith {
                src = ../.;
                filter = crane.filterCargoSources;
              }))
            ];
          };
          common = {
            pname = "qcp";
            version = "0.3.3";
            inherit src;
            strictDeps = true;

            stdenv = pkgs.stdenvAdapters.useMoldLinker pkgs.clangStdenv;

            doCheck = false;
            cargoCheckCommand = ":"; # skip checks

            nativeBuildInputs = with pkgs; [pkg-config];
            buildInputs = with pkgs; [openssl openssl.dev] ++ lib.optionals pkgs.stdenv.isDarwin [pkgs.libiconv];

            cargoExtraArgs = "-p qcp";

            CARGO_BUILD_RUSTFLAGS = concatStringsSep " " [
              "-Clinker=clang"
              "-Clink-arg=--ld-path=${pkgs.mold-wrapped}/bin/mold"
            ];
          };
        in rec {
          qcp = crane.buildPackage (common // { cargoArtifacts = crane.buildDepsOnly (common // { version = "0.0.0"; }); });
          default = qcp;
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
