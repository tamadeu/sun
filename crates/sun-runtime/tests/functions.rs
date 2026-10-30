use std::{io, sync::{Arc, atomic::{AtomicBool, Ordering}}};
use sun_runtime::{execute, RuntimeError, RuntimeHost, Session, Value};

#[derive(Default)]
struct Host { lines: Vec<String>, cancelled: Option<Arc<AtomicBool>> }
impl RuntimeHost for Host {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.lines.push(line.into());
        if line == "cancel" { if let Some(flag) = &self.cancelled { flag.store(true, Ordering::Release); } }
        Ok(())
    }
    fn cancellation_flag(&self) -> Option<Arc<AtomicBool>> { self.cancelled.clone() }
}

fn run(session: &mut Session, source: &str, host: &mut Host) -> Result<(), RuntimeError> {
    session.execute(&sun_parser::parse(source).unwrap(), host)
}

#[test]
fn calls_forward_declarations_and_isolates_matrix_arguments() {
    let source = "value=99; A=[1,2;3,4]; result=change(A); disp(A); disp(result); disp(value);\nfunction result=change(A)\nvalue=7; A(1)=8; result=A;\nend";
    assert_eq!(execute(&sun_parser::parse(source).unwrap()).unwrap(), ["[1, 2; 3, 4]", "[8, 2; 3, 4]", "99"]);
}

#[test]
fn persists_redefines_and_clears_functions_separately_from_variables() {
    let mut session = Session::new();
    let mut host = Host::default();
    run(&mut session, "function result=twice(value); result=value*2; end", &mut host).unwrap();
    assert!(session.variables().is_empty());
    run(&mut session, "answer=twice(4);", &mut host).unwrap();
    assert_eq!(session.variables()["answer"], Value::Number(8.0));
    run(&mut session, "function result=twice(value); result=value*3; end", &mut host).unwrap();
    run(&mut session, "answer=twice(4);", &mut host).unwrap();
    assert_eq!(session.variables()["answer"], Value::Number(12.0));
    session.clear();
    assert!(run(&mut session, "answer=twice(4);", &mut host).is_err());
}

#[test]
fn recursive_calls_and_return_stop_only_the_current_function() {
    let source = "disp(factorial(6)); disp(odd(7)); disp(100);\nfunction result=factorial(value)\nif value<=1; result=1; return; end\nresult=value*factorial(value-1);\nend\nfunction result=odd(value)\nif value==0; result=0; return; end\nresult=even(value-1);\nend\nfunction result=even(value)\nif value==0; result=1; return; end\nresult=odd(value-1);\nend";
    assert_eq!(execute(&sun_parser::parse(source).unwrap()).unwrap(), ["720", "1", "100"]);
}

#[test]
fn no_output_functions_are_valid_statements_but_not_values() {
    let mut session = Session::new();
    let mut host = Host::default();
    run(&mut session, "function notify(text); disp(text); return; disp(999); end\nnotify(\"hello\");", &mut host).unwrap();
    assert_eq!(host.lines, ["hello"]);
    assert!(matches!(run(&mut session, "result=notify(\"not printed\");", &mut host), Err(RuntimeError::NoOutput(_))));
    assert_eq!(host.lines, ["hello"]);
}

#[test]
fn validates_arity_missing_output_and_local_variable_visibility() {
    let mut session = Session::new();
    let mut host = Host::default();
    run(&mut session, "outer=99; function result=missing(value); local=1; end\nfunction result=read_outer(); result=outer; end", &mut host).unwrap();
    assert!(matches!(run(&mut session, "missing();", &mut host), Err(RuntimeError::Arity(_, _))));
    assert!(matches!(run(&mut session, "missing(1);", &mut host), Err(RuntimeError::UnassignedOutput(_, _))));
    assert!(matches!(run(&mut session, "read_outer();", &mut host), Err(RuntimeError::UndefinedVariable(name)) if name=="outer"));
    assert!(!session.variables().contains_key("local"));
    assert_eq!(session.variables()["outer"], Value::Number(99.0));
}

#[test]
fn function_errors_are_catchable_and_do_not_pollute_the_workspace() {
    let source = "function result=fail(value); local=value; error(\"Test:Failure\",\"broken\"); end\ntry; answer=fail(7); catch failure; disp(failure.identifier); end\ndisp(42);";
    assert_eq!(execute(&sun_parser::parse(source).unwrap()).unwrap(), ["Test:Failure", "42"]);
}

#[test]
fn recursion_limit_is_catchable_and_session_recovers() {
    let mut session = Session::new();
    let mut host = Host::default();
    run(&mut session, "function result=forever(); result=forever(); end", &mut host).unwrap();
    assert!(matches!(run(&mut session, "forever();", &mut host), Err(RuntimeError::RecursionLimit)));
    run(&mut session, "answer=42;", &mut host).unwrap();
    assert_eq!(session.variables()["answer"], Value::Number(42.0));
}

#[test]
fn cancellation_bypasses_handlers_inside_and_outside_functions() {
    let mut session = Session::new();
    let flag = Arc::new(AtomicBool::new(false));
    let mut host = Host { cancelled: Some(flag.clone()), ..Host::default() };
    let source = "saved=7; function result=stop()\ntry; disp(\"cancel\"); while 1; end; catch; disp(\"inner catch\"); end\nresult=1;\nend\ntry; answer=stop(); catch; disp(\"outer catch\"); end";
    assert!(matches!(run(&mut session, source, &mut host), Err(RuntimeError::Cancelled)));
    assert_eq!(host.lines, ["cancel"]);
    assert_eq!(session.variables()["saved"], Value::Number(7.0));
    assert!(!session.variables().contains_key("answer"));
    flag.store(false, Ordering::Release);
    run(&mut session, "disp(saved);", &mut host).unwrap();
    assert_eq!(host.lines.last().unwrap(), "7");
}

#[test]
fn user_functions_cannot_bypass_parallel_effect_validation() {
    for name in ["effect", "sqrt", "mean", "zeros", "min"] {
        let source = format!("A=[0,0]; function result={name}(value); disp(value); result=value; end\nparfor index=1:2; A(index)={name}(index); end");
        let mut session = Session::new();
        let mut host = Host::default();
        assert!(matches!(run(&mut session, &source, &mut host), Err(RuntimeError::InvalidParFor(_))));
        assert!(host.lines.is_empty());
        assert_eq!(session.variables()["A"], Value::Matrix(sun_numeric::SunMatrix::from_rows(vec![vec![0.0,0.0]]).unwrap()));
    }
}

#[test]
fn validates_parallel_blocks_inside_functions_before_registering() {
    let mut session = Session::new();
    let mut host = Host::default();
    assert!(matches!(run(&mut session, "saved=9; function invalid(); parfor index=1:2; disp(index); end; end", &mut host), Err(RuntimeError::InvalidParFor(_))));
    assert!(session.variables().is_empty());
    assert!(run(&mut session, "invalid();", &mut host).is_err());
}

#[test]
fn functions_use_independent_index_end_contexts() {
    let source = "A=[10,20,30]; function result=last(values); result=values(end); end\ndisp(A(last([1,2]))); disp(A(end));";
    assert_eq!(execute(&sun_parser::parse(source).unwrap()).unwrap(), ["20", "30"]);
}

#[test]
fn restores_depth_after_caught_recursion_and_checks_exact_limit() {
    let source = "function result=depth(value); if value==0; result=0; return; end; result=1+depth(value-1); end\ntry; depth(32); catch failure; disp(failure.identifier); end\ndisp(depth(31));";
    assert_eq!(execute(&sun_parser::parse(source).unwrap()).unwrap(), ["Sun:RecursionLimit", "31"]);
}

#[test]
fn matrix_variables_shadow_functions_but_scalars_are_not_callable() {
    let mut session = Session::new();
    let mut host = Host::default();
    run(&mut session, "function result=custom(value); disp(999); result=value; end\ncustom=[10,20]; disp(custom(2));", &mut host).unwrap();
    assert_eq!(host.lines, ["20"]);
    assert!(run(&mut session, "custom=7; custom(1);", &mut host).is_err());
    assert_eq!(host.lines, ["20"]);
    run(&mut session, "function result=sqrt(value); disp(999); result=value; end\nsqrt=[10,20]; A=[0,0]; parfor index=1:2; A(index)=sqrt(index); end; disp(A);", &mut host).unwrap();
    assert_eq!(host.lines.last().unwrap(), "[10, 20]");
}

#[test]
fn rejects_void_calls_nested_in_expressions_without_running_the_body() {
    let mut session = Session::new();
    let mut host = Host::default();
    run(&mut session, "function notify(); disp(999); end", &mut host).unwrap();
    for source in ["disp(notify());", "1+notify();", "[notify()];"] {
        assert!(matches!(run(&mut session, source, &mut host), Err(RuntimeError::NoOutput(_))));
    }
    assert!(host.lines.is_empty());
}

#[test]
fn clears_and_restores_index_end_when_a_callee_catches_an_error() {
    let source = "A=[10,20,30]; function result=first(); try; disp(end); catch failure; result=1; end; end\ndisp(A(first()+end-1));";
    assert_eq!(execute(&sun_parser::parse(source).unwrap()).unwrap(), ["30"]);
}