#[test]
fn unsupported_numeric_features_report_interpreter_guidance() {
    for source in ["true;", "false;", "values=1:3;", "zeros(2);", "ones(2);", "eye(2);", "mean([1,2]);", "min(1,2);", "max([1,2]);"] {
        let module = sun_parser::parse(source).unwrap();
        let error = sun_codegen_llvm::compile_to_llvm_ir(&module).unwrap_err().to_string();
        assert!(error.contains("sun run"), "{source}: {error}");
    }
}