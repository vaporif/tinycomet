{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    cometbft-src = {
      url = "github:cometbft/cometbft/v0.38.21";
      flake = false;
    };
  };

  outputs = {
    nixpkgs,
    fenix,
    cometbft-src,
    ...
  }: let
    systems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
    forAllSystems = f:
      nixpkgs.lib.genAttrs systems (system:
        f {
          pkgs = nixpkgs.legacyPackages.${system};
          fenixPkgs = fenix.packages.${system};
        });
  in {
    formatter = nixpkgs.lib.genAttrs systems (system: nixpkgs.legacyPackages.${system}.alejandra);

    devShells = forAllSystems ({
      pkgs,
      fenixPkgs,
      ...
    }: let
      toolchain = fenixPkgs.stable.withComponents [
        "cargo"
        "clippy"
        "rustc"
        "rustfmt"
        "rust-src"
        "rust-analyzer"
      ];
      cometbft = pkgs.buildGoModule {
        name = "cometbft";
        src = cometbft-src;
        vendorHash = "sha256-BFm+AimN+fdUPz3+MNIvJyqp8dsn5JjNaipnYsHZiC8=";
        doCheck = false;
      };
    in {
      default = pkgs.mkShell {
        packages =
          [
            toolchain
            cometbft
            pkgs.just
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.apple-sdk_15
          ];

        env = {
          RUST_BACKTRACE = "1";
          RUST_SRC_PATH = "${toolchain}/lib/rustlib/src/rust/library";
        };
      };
    });
  };
}
