{ pkgs ? import <nixpkgs> { } }: {
  cli = let
    manifest = (pkgs.lib.importTOML ./Cargo.toml).workspace.package;
  in pkgs.rustPlatform.buildRustPackage {
    version = manifest.version;

    src = pkgs.lib.cleanSource ./.;

    cargoLock = {
      lockFile = ./Cargo.lock;
    };

    nativeBuildInputs = [ pkgs.pkg-config ];
    PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";

    pname = "xdvdfs";
    cargoBuildFlags = "--package xdvdfs-cli";

    cargoTestFlags = [
      # Only run checks for CLI and its dependency
      # We don't want to run checks on -web or -fsd here
      "--package xdvdfs-cli"
      "--package xdvdfs"
    ];
  };
}
