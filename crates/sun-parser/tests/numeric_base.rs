use sun_ast::{ExprKind, StmtKind};
use sun_parser::parse;

#[test]
fn parses_boolean_literals_and_ranges_as_expressions() {
    let module = parse("enabled=true; disabled=false; values=1+1:2:3*3; total=sum(1:3); for index=(1:3); end").unwrap();
    assert!(matches!(&module.statements[0].kind, StmtKind::Assign { value, .. } if value.kind == ExprKind::Bool(true)));
    assert!(matches!(&module.statements[2].kind, StmtKind::Assign { value, .. } if matches!(value.kind, ExprKind::IndexRange { .. })));
    assert!(matches!(&module.statements[4].kind, StmtKind::For { .. }));
    for source in ["true=1;", "false=0;", "x=1:2:3:4;", "x=1:;", "x=:;"] {
        assert!(parse(source).is_err(), "accepted {source}");
    }
}