{
  description = "cfwdon development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );
    in
    {
      devShells = forAllSystems (
        pkgs:
        let
          # rust-toolchain.toml pins the channel, components, and the
          # wasm32-unknown-unknown target; flake.lock pins the exact release.
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        in
        {
          default = pkgs.mkShell {
            packages = [
              rustToolchain
              pkgs.nodejs_24
              pkgs.pnpm
              pkgs.wrangler
              pkgs.worker-build
              # Must match the wasm-bindgen version in Cargo.lock.
              pkgs.wasm-bindgen-cli_0_2_129
              pkgs.binaryen
              pkgs.cargo-generate
              pkgs.just
              pkgs.jq
              pkgs.python3
              pkgs.pkg-config
              pkgs.openssl
            ];

            OPENSSL_NO_VENDOR = "1";

            shellHook = ''
              export WASM_BINDGEN_BIN="$(command -v wasm-bindgen)"
              export WASM_OPT_BIN="$(command -v wasm-opt)"
            '';
          };
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
