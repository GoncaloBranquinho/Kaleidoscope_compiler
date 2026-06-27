{
  description = "Kaleipl dev environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
  };

  outputs = { self, nixpkgs }:
  let
    system = "x86_64-linux";
    pkgs = import nixpkgs { inherit system; };

    llvm = pkgs.llvmPackages_20;
  in {
    devShells.${system}.default = pkgs.mkShell {
      packages = [
        pkgs.rustc
        pkgs.cargo

        llvm.llvm
        llvm.clang

        pkgs.libffi
        pkgs.zlib
        pkgs.libxml2
        pkgs.ncurses
        pkgs.pkg-config
        pkgs.emacs
      ];

      LLVM_SYS_201_PREFIX = "${llvm.llvm.dev}";

      LIBCLANG_PATH =
        "${llvm.libclang.lib}/lib";

      shellHook = ''
        echo "LLVM version:"
        llvm-config --version
      '';
    };
  };
}
