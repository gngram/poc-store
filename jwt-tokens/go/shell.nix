{ 
  pkgs ? import <nixpkgs> {}
}:
pkgs.stdenv.mkDerivation {
    name = "go-shell";

    buildInputs = [ 
      pkgs.go
      pkgs.protobuf
      pkgs.protoc-gen-go
      pkgs.protoc-gen-go-grpc
    ];

    shellHook = ''
        export GOROOT="${pkgs.go}/share/go" #replace with your go version.
        export PATH="$PATH:$GOROOT/bin"
        go version
        go env
    '';
}
