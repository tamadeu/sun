#[test]
fn csv_builtins_require_the_interpreter() {
    for source in ["A=readmatrix(\"input.csv\");", "writematrix([1],\"output.csv\");"] {
        let module = sun_parser::parse(source).unwrap();
        let error = sun_codegen_llvm::compile_to_llvm_ir(&module).unwrap_err().to_string();
        assert!(error.contains("interpreter"));
        assert!(error.contains("sun run"));
    }
}