{
  description = "Tools and libraries for interacting with XISO/XDVDFS images";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
  };

  outputs = { self, nixpkgs }: 
  let
    supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    pkgsFor = nixpkgs.legacyPackages;
  in {
    packages = forAllSystems (system:
    let
      pkgs = pkgsFor.${system};
      code = pkgs.callPackage ./. { inherit pkgs; };
    in {
      cli = code.cli;
      default = code.cli;
    });
  };
}
