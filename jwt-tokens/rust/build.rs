fn main() {
    prost_build::compile_protos(&["../proto/token.proto"], &["../proto"]).unwrap();
}

