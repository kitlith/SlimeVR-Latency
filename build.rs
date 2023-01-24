fn main() {
    protobuf_codegen::Codegen::new()
        // .protoc()
        // .protoc_path(&protoc_bin_vendored::protoc_bin_path().unwrap())
        .pure()
        .customize(
            protobuf_codegen::Customize::default()
                .lite_runtime(true)
                //.tokio_bytes(true)
        )
        .includes(&["src/bridge"])
        .input("src/bridge/ProtobufMessages.proto")
        .cargo_out_dir("protos")
        .run_from_script();
}
