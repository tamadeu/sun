#[test]
fn graphics_require_a_graphical_interpreter_host() {
    for source in ["plot(1:3,1:3);", "title(\"Signal\");", "xlabel(\"Time\");", "ylabel(\"Value\");", "legend(\"Signal\");", "grid(true);", "axis([0,1,0,1]);", "clf();"] {
        let error = sun_codegen_llvm::compile_to_llvm_ir(&sun_parser::parse(source).unwrap()).unwrap_err().to_string();
        assert!(error.contains("Sun Studio"), "{error}");
    }
}