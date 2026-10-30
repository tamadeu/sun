use std::io;
use sun_runtime::{execute, RuntimeHost, Session, Value};

#[derive(Default)]
struct Host(Vec<String>);
impl RuntimeHost for Host {
    fn write_line(&mut self, line: &str) -> io::Result<()> { self.0.push(line.into()); Ok(()) }
}

fn output(source: &str) -> Vec<String> { execute(&sun_parser::parse(source).unwrap()).unwrap() }

#[test]
fn booleans_are_distinct_values_and_control_flow_uses_them() {
    let mut session = Session::new();
    let mut host = Host::default();
    session.execute(&sun_parser::parse("enabled=true; compared=2<3; logical=true && !false; number=true+2; if enabled; disp(false); end").unwrap(), &mut host).unwrap();
    for name in ["enabled", "compared", "logical"] { assert_eq!(session.variables()[name], Value::Bool(true)); }
    assert_eq!(session.variables()["number"], Value::Number(3.0));
    assert_eq!(host.0, ["false"]);
    assert_eq!(output("disp(false && error(\"skipped\")); disp(true || error(\"skipped\"));"), ["false", "true"]);
}

#[test]
fn ranges_materialize_row_vectors_and_reuse_loop_endpoint_rules() {
    assert_eq!(output("values=1+1:2:3*3; disp(values); disp(3:-1:1); disp(0:0.1:0.3); disp(size(2:1)); disp(sum(1:3)); for value=(1:2:5); disp(value); end"),
        ["[2, 4, 6, 8]", "[3, 2, 1]", "[0, 0.1, 0.2, 0.3]", "[1, 0]", "6", "1", "3", "5"]);
    for source in ["x=1:0:3;", "x=1:(1/0):3;", "x=1:1e20;", "x=1:16777217;"] {
        assert!(execute(&sun_parser::parse(source).unwrap()).is_err(), "accepted {source}");
    }
}

#[test]
fn constructors_accept_scalar_rectangular_and_size_vector_forms() {
    assert_eq!(output("disp(zeros()); disp(ones(2)); disp(eye(2,3)); disp(zeros([1,3])); disp(size(ones(0,3))); disp(size(eye(3,0))); disp(size(zeros(0)));"),
        ["[0]", "[1, 1; 1, 1]", "[1, 0, 0; 0, 1, 0]", "[0, 0, 0]", "[0, 3]", "[3, 0]", "[0, 0]"]);
    for source in ["zeros(-1);", "ones(1.5);", "eye(1/0);", "zeros(1e100,0);", "ones(16777216,2);", "zeros([1,2,3]);", "eye(true);", "zeros(1,2,3);"] {
        assert!(execute(&sun_parser::parse(source).unwrap()).is_err(), "accepted {source}");
    }
}

#[test]
fn reductions_cover_all_elements_and_pairwise_min_max_broadcast() {
    assert_eq!(output("A=[1,8;-3,2]; disp(sum(A)); disp(mean(A)); disp(min(A)); disp(max(A)); disp(mean(1:5)); disp(min(A,0)); disp(max([1;4],[2,3])); disp(sum([])); disp(mean([])); disp(min([])); disp(max([])); disp(mean(true)); disp(isempty([]));"),
        ["8", "2", "-3", "8", "3", "[0, 0; -3, 0]", "[2, 3; 4, 4]", "0", "NaN", "[]", "[]", "1", "true"]);
    for source in ["mean();", "min(1,2,3);", "max(\"text\");", "mean([1],2);", "min(ones(2,3),ones(4,2));"] {
        assert!(execute(&sun_parser::parse(source).unwrap()).is_err(), "accepted {source}");
    }
}

#[test]
fn elementary_functions_map_matrices_and_preserve_empty_shapes() {
    for name in ["sqrt", "sin", "cos", "tan", "abs", "exp", "log"] {
        let source = format!("A=[1,2;3,4]; B={name}(A); disp(B(1,2)=={name}(2)); disp(size(B)); disp(size({name}(zeros(0,3))));");
        assert_eq!(output(&source), ["true", "[2, 2]", "[0, 3]"], "{name}");
    }
    assert_eq!(output("disp(sqrt([1,4;9,16])); disp(abs([-2,3])); disp(log([0,1])); disp(sqrt([-1]));"),
        ["[1, 2; 3, 4]", "[2, 3]", "[-inf, 0]", "[NaN]"]);
}

#[test]
fn range_vectors_concatenate_and_select_matrix_elements() {
    assert_eq!(output("A=[1:3,7; 4:6,8]; indices=1:2:7; disp(A(indices)); A(indices)=true; disp(A); disp(A([2,1],2:3)); disp([zeros(0,2),ones(0,3)]);"),
        ["[1, 2, 3, 7]", "[1, 1, 1, 1; 4, 5, 6, 8]", "[5, 6; 1, 1]", "[]"]);
    for source in ["A=1:3; disp(A([0,1]));", "A=1:3; disp(A([1,4]));", "A=1:3; A([1,4])=0;", "[ones(2,1),ones(3,1)];"] {
        assert!(execute(&sun_parser::parse(source).unwrap()).is_err(), "accepted {source}");
    }
}

#[test]
fn boolean_results_and_new_numeric_functions_work_in_parfor() {
    assert_eq!(output("A=zeros(1,3); parfor index=1:3; A(index)=index<3; end; disp(A); B=zeros(1,3); parfor index=1:3; B(index)=mean(1:index); end; disp(B);"),
        ["[1, 1, 0]", "[1, 1.5, 2]"]);
}

#[test]
fn index_expressions_are_evaluated_once_and_dispatch_on_their_values() {
    let source = "A=[10,20,30]; indices=ones(1,2); disp(A(indices)); disp(A(ones(1,2))); A(ones(1,2))=7; disp(A); function result=choose(); disp(\"once\"); result=[2,3]; end\ndisp(A(choose())); disp(A(ones(1,2),1));";
    assert_eq!(output(source), ["[10, 10]", "[10, 10]", "[7, 20, 30]", "once", "[20, 30]", "[7; 7]"]);
}

#[test]
fn documented_numeric_example_runs() {
    assert_eq!(output(include_str!("../../../examples/base_numerica.sun")), ["true", "[0, 0.5, 1, 1.5, 2]", "[1, 2; 3, 4]", "[1, 0; 0, 1]", "[1, 1; 1, 1]", "1", "1", "4", "[2, 2; 3, 4]", "[0, 3]", "true", "[1, 2]"]);
}