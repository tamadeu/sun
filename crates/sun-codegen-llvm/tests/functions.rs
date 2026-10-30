#[test]
fn rejects_user_functions_with_interpreter_guidance() {
    let module = sun_parser::parse("function result = square(value); result=value*value; end").unwrap();
    let error = sun_codegen_llvm::compile_to_llvm_ir(&module).unwrap_err().to_string();
    assert!(error.contains("user functions"));
    assert!(error.contains("sun run"));
}