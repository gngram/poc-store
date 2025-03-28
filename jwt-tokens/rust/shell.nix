{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = [
    pkgs.rustc      # Rust compiler
    pkgs.cargo      # Rust package manager
    pkgs.rustfmt    # Formatter
    pkgs.clippy     # Linter
    pkgs.pkg-config # For linking native libraries
    pkgs.openssl    # If you need OpenSSL
    pkgs.protobuf
  ];
}
