use std::{collections::VecDeque, io};
use sun_runtime::{DebugAction, DebugBreakpoint, DebugController, DebugLocation, DebugSnapshot, DebugSource, DebugStopReason, RuntimeHost, Session};

struct Host { controller: DebugController, actions: VecDeque<DebugAction>, stops: Vec<(String, usize, Vec<String>, Vec<String>)> }
impl RuntimeHost for Host {
    fn write_line(&mut self, _: &str) -> io::Result<()> { Ok(()) }
    fn debug_enabled(&self) -> bool { true }
    fn debug_checkpoint(&mut self, location: &DebugLocation, depth: usize) -> io::Result<Option<DebugStopReason>> { Ok(self.controller.checkpoint(location, depth)) }
    fn debug_pause(&mut self, snapshot: &DebugSnapshot) -> io::Result<()> {
        let frame = snapshot.frames.last().unwrap();
        self.stops.push((frame.source.id.clone(), frame.location.line, snapshot.frames.iter().map(|frame| frame.name.clone()).collect(), frame.variables.iter().map(|value| format!("{}={}", value.name, value.value)).collect()));
        self.controller.resume(self.actions.pop_front().unwrap_or(DebugAction::Continue), snapshot.depth);
        Ok(())
    }
}

fn run(session: &mut Session, host: &mut Host, id: &str, source: &str) {
    let module = sun_parser::parse(source).unwrap();
    let debug_source = DebugSource::new(id, format!("{id}.sun"), source, &module);
    session.execute_with_source(&module, host, debug_source).unwrap();
}

#[test]
fn steps_into_functions_inspects_real_locals_and_steps_out() {
    let mut session = Session::new();
    let mut host = Host { controller: DebugController::new(Vec::new(), true), actions: [DebugAction::StepOver,DebugAction::StepInto,DebugAction::StepOut,DebugAction::Continue].into(), stops: Vec::new() };
    run(&mut session, &mut host, "script", "saved=10;\nanswer=twice(3);\ndisp(answer);\nfunction result=twice(value)\nlocal=value+1;\nresult=local*2;\nend");
    assert_eq!(host.stops.iter().map(|stop| stop.1).collect::<Vec<_>>(), [1,2,5,3]);
    assert_eq!(host.stops[2].2, ["<script>","twice"]);
    assert_eq!(host.stops[2].3, ["value=3"]);
    assert!(host.stops[3].3.contains(&"answer=8".into()));
}

#[test]
fn persistent_functions_keep_original_source_and_breakpoints() {
    let mut session = Session::new();
    let mut host = Host { controller: DebugController::new(Vec::new(), false), actions: VecDeque::new(), stops: Vec::new() };
    run(&mut session, &mut host, "definition", "function result=square(value)\nresult=value*value;\nend");
    host.controller.set_breakpoints(vec![DebugBreakpoint { source_id: "definition".into(), line: 2 }]);
    run(&mut session, &mut host, "caller", "answer=square(4);");
    assert_eq!(host.stops[0].0, "definition");
    assert_eq!(host.stops[0].2, ["<script>","square"]);
    assert_eq!(host.stops[0].3, ["value=4"]);
}

#[test]
fn loop_headers_are_revisited_and_source_columns_use_utf16() {
    let mut session = Session::new();
    let mut host = Host { controller: DebugController::new(vec![DebugBreakpoint { source_id: "loop".into(), line: 2 }], false), actions: VecDeque::new(), stops: Vec::new() };
    run(&mut session, &mut host, "loop", "value=0;\nwhile value<2\nvalue=value+1;\nend");
    assert_eq!(host.stops.len(), 3);
    let text = "text=\"\u{1f600}\"; value=1;\n";
    let module = sun_parser::parse(text).unwrap();
    let source = DebugSource::new("unicode", "unicode.sun", text, &module);
    assert_eq!(source.location(module.statements[1].span).column, 12);
}

#[test]
fn caught_function_errors_unwind_frames_before_the_next_call() {
    let mut session = Session::new();
    let mut host = Host { controller: DebugController::new(vec![DebugBreakpoint { source_id: "errors".into(), line: 8 }, DebugBreakpoint { source_id: "errors".into(), line: 12 }], false), actions: VecDeque::new(), stops: Vec::new() };
    run(&mut session, &mut host, "errors", "try\nanswer=fail(3);\ncatch failure\nrecovered=good(2);\nend\ndisp(recovered);\nfunction result=fail(value)\nlocal=value;\nerror(\"Test:Failure\",\"failed\");\nend\nfunction result=good(value)\nresult=value+1;\nend");
    assert_eq!(host.stops.len(), 2);
    assert_eq!(host.stops[0].2, ["<script>","fail"]);
    assert_eq!(host.stops[1].2, ["<script>","good"]);
}

#[test]
fn documented_debug_example_exposes_function_locals_without_leaking_them() {
    let mut session = Session::new();
    let mut host = Host { controller: DebugController::new(vec![DebugBreakpoint { source_id: "example".into(), line: 7 }], false), actions: VecDeque::new(), stops: Vec::new() };
    run(&mut session, &mut host, "example", include_str!("../../../examples/depuracao.sun"));
    assert_eq!(host.stops.len(), 1);
    assert_eq!(host.stops[0].2, ["<script>","dobro"]);
    assert!(host.stops[0].3.contains(&"local=4".into()));
    assert_eq!(session.variables()["resultado"], sun_runtime::Value::Number(8.0));
    assert!(!session.variables().contains_key("matriz"));
}