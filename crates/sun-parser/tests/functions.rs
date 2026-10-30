use sun_ast::StmtKind;
use sun_parser::{parse, ParseError};

#[test]
fn parses_functions_and_nested_control_flow() {
    let module = parse("result = twice(3);\nfunction result = twice(value)\nif value > 0\nresult = value * 2;\nreturn;\nend\nresult = 0;\nend\nfunction notify()\ndisp(42);\nend").unwrap();
    let StmtKind::Function(function) = &module.statements[1].kind else { panic!("expected function") };
    assert_eq!(function.name, "twice");
    assert_eq!(function.parameters, ["value"]);
    assert_eq!(function.output.as_deref(), Some("result"));
    assert_eq!(function.body.len(), 2);
    let StmtKind::Function(function) = &module.statements[2].kind else { panic!("expected function") };
    assert!(function.parameters.is_empty());
    assert_eq!(function.output, None);
}

#[test]
fn rejects_invalid_function_headers_and_scopes() {
    for source in [
        "function result = f(value, value); end",
        "function result = f(value,); end",
        "function result = f(value) result=1; end",
        "function [first, second] = f(); end",
        "function f(); end; function f(); end",
        "if 1; function f(); end; end",
        "for index=1:2; function f(); break; end; end",
        "function f(); function g(); end; end",
        "function f(); break; end",
        "function f(); continue; end",
    ] {
        assert!(parse(source).is_err(), "accepted {source}");
    }
}

#[test]
fn incomplete_function_requests_more_repl_input() {
    let source = "function result = square(value)\nresult = value * value;\n";
    assert!(matches!(parse(source), Err(ParseError::UnexpectedToken { offset, .. }) if offset == source.len()));
}