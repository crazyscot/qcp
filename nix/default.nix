{
  lib,
  stdenv,
  fetchFromGitHub,
  rustPlatform,
}:

rustPlatform.buildRustPackage rec {
  pname = "qcp";
  version = "0.3.0";

  src = fetchFromGitHub {
    owner = "crazyscot";
    repo = pname;
    rev = "v${version}";
    hash = "sha256-9nJ01OPAU60veLpL2BlSSUTMu/xdUBDVkV0YEFNQ3FU=";
  };

  cargoHash = "sha256-Au8yTz4lelryGhq21dwq98HijImNahpmnEtKwEQF9jE=";

  checkFlags = [
    # SSH home directory tests will not work in nix sandbox
    "--skip=config::ssh::includes::test::home_dir"
  ];

  meta = {
    description = "The QUIC Copier (qcp) is an experimental high-performance remote file copy utility for long-distance internet connections.";
    homepage = "https://github.com/crazyscot/qcp";
    license = lib.licenses.agpl3Only;
    maintainers = with lib.maintainers; [ poptart ];
  };
}
