#[test]
fn graphics_require_a_graphical_interpreter_host() {
    for source in ["plot(1:3,1:3);", "plot3(1:3,1:3,1:3);", "surf(ones(2));", "title(\"Signal\");", "xlabel(\"Time\");", "ylabel(\"Value\");", "zlabel(\"Height\");", "legend(\"Signal\");", "grid(true);", "axis([0,1,0,1]);", "view(3);", "colormap(\"turbo\");", "shading(\"flat\");", "clf();"] {
        let error = sun_codegen_llvm::compile_to_llvm_ir(&sun_parser::parse(source).unwrap()).unwrap_err().to_string();
        assert!(error.contains("Sun Studio"), "{error}");
    }
}