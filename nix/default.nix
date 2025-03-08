{
  lib,
  stdenv,
  fetchFromGitHub,
  rustPlatform,
}:

rustPlatform.buildRustPackage rec {
  pname = "qcp";
  version = "0.3.3";

  src = fetchFromGitHub {
    owner = "crazyscot";
    repo = pname;
    rev = "v${version}";
    hash = "sha256-NlRM8FGYBmvT7KDOYTyUWTeERa96UPebuyicncJ4ANY=";
  };

  cargoHash = "sha256-KfsNfvCPpm/6oaUa+H4raIxou+udIuYEWhng2ddi68Y=";

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

  postInstall = ''
    install -Dm644 $src/qcp/misc/qcp.1 $out/share/man/man1/qcp.1
    install -Dm644 $src/qcp/misc/qcp_config.5 $out/share/man/man5/qcp_config.5
    install -Dm644 $src/qcp/misc/20-qcp.conf $out/etc/sysctl.d/20-qcp.conf
    install -Dm644 $src/qcp/misc/qcp.conf $out/etc/qcp.conf
    install -Dm644 $src/README.md $out/share/doc/qcp/README.md
    install -Dm644 $src/LICENSE $out/share/doc/qcp/LICENSE
  '';
}
