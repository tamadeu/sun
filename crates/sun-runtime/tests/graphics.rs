use std::io;
use sun_runtime::{FigureEvent, RuntimeError, RuntimeHost, Session};

#[derive(Default)]
struct Host { events: Vec<FigureEvent>, deny: bool }
impl RuntimeHost for Host {
    fn write_line(&mut self, _: &str) -> io::Result<()> { Ok(()) }
    fn figure_event(&mut self, event: &FigureEvent) -> io::Result<()> {
        if self.deny { return Err(io::Error::other("figure delivery failed")); }
        self.events.push(event.clone()); Ok(())
    }
}

fn run(session: &mut Session, host: &mut Host, source: &str) -> Result<(), RuntimeError> {
    session.execute(&sun_parser::parse(source).unwrap(), host)
}

#[test]
fn hosts_without_graphics_report_an_explicit_error() {
    struct TextHost;
    impl RuntimeHost for TextHost { fn write_line(&mut self, _: &str) -> io::Result<()> { Ok(()) } }
    let error = TextHost.figure_event(&FigureEvent::FiguresCleared).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert!(error.to_string().contains("Sun Studio"));
}

#[test]
fn plots_matrix_columns_and_updates_layout_across_commands() {
    let mut session = Session::new();
    let mut host = Host::default();
    run(&mut session, &mut host, "x=0:2; plot(x,[1,4;2,5;3,6]);",).unwrap();
    run(&mut session, &mut host, "title(\"Curves\"); xlabel(\"Time\"); ylabel(\"Value\"); legend(\"First\",\"Second\"); grid(true); axis([0,2,0,8]);").unwrap();
    let figure = session.figure().unwrap();
    assert_eq!(figure.traces.len(), 2);
    assert_eq!(figure.traces[0].x, [Some(0.0),Some(1.0),Some(2.0)]);
    assert_eq!(figure.traces[1].y, [Some(4.0),Some(5.0),Some(6.0)]);
    assert_eq!(figure.layout.title, "Curves");
    assert_eq!(figure.layout.legend, ["First","Second"]);
    assert!(figure.layout.grid);
    assert_eq!(figure.layout.x_range, Some([0.0,2.0]));
    assert_eq!(figure.layout.axis_revision, 1);
    assert_eq!(host.events.len(), 7);
    assert!(matches!(host.events[1], FigureEvent::FigureUpdate { figure_id: 1, .. }));
    run(&mut session, &mut host, "axis(\"auto\"); axis(\"auto\");").unwrap();
    assert_eq!(session.figure().unwrap().layout.axis_revision, 3);
}

#[test]
fn invalid_commands_preserve_the_previous_figure_and_emit_nothing() {
    let mut session = Session::new();
    let mut host = Host::default();
    run(&mut session, &mut host, "plot(1:3,4:6);").unwrap();
    let previous = session.figure().unwrap().clone();
    for source in ["plot(1:2,1:3);", "plot([],[]);", "plot(true,1);", "plot(ones(2,2),ones(2,2));", "plot(1:10001,1:10001);", "plot(1:3,zeros(3,17));", "plot([0/0],[1]);", "title(7);", "legend(\"one\",\"two\");", "axis([0,0,0,1]);", "grid(3);", "saved=plot(1,2);"] {
        assert!(run(&mut session, &mut host, source).is_err(), "accepted {source}");
        assert_eq!(session.figure().unwrap(), &previous);
    }
    assert_eq!(host.events.len(), 1);
}

#[test]
fn replacement_clear_and_nonfinite_gaps_have_explicit_semantics() {
    let mut session = Session::new();
    let mut host = Host::default();
    run(&mut session, &mut host, "plot([1;2;3],[1;0/0;4]); grid(\"on\"); grid(); legend(\"off\"); axis(\"auto\");").unwrap();
    assert_eq!(session.figure().unwrap().traces[0].y, [Some(1.0),None,Some(4.0)]);
    assert!(!session.figure().unwrap().layout.grid);
    run(&mut session, &mut host, "plot(1,2);").unwrap();
    assert_eq!(session.figure().unwrap().id, 2);
    assert_eq!(session.figure().unwrap().layout.title, "");
    run(&mut session, &mut host, "clf();").unwrap();
    assert!(session.figure().is_none());
    assert!(matches!(host.events.last(), Some(FigureEvent::FiguresCleared)));
    assert!(run(&mut session, &mut host, "title(\"missing figure\");").is_err());
    run(&mut session, &mut host, "plot(1,2);").unwrap();
    session.clear();
    assert!(session.figure().is_none());
}

#[test]
fn host_failure_does_not_commit_a_new_figure() {
    let mut session = Session::new();
    let mut host = Host { deny: true, ..Host::default() };
    assert!(matches!(run(&mut session, &mut host, "plot(1,2);"), Err(RuntimeError::Io(_))));
    assert!(session.figure().is_none());
}

#[test]
fn plotting_effects_are_rejected_before_parallel_execution() {
    for name in ["plot", "plot3", "surf", "title", "xlabel", "ylabel", "zlabel", "legend", "grid", "axis", "clf"] {
        let source = format!("plot(1,2); A=zeros(1,2); parfor index=1:2; temporary={name}(index); A(index)=1; end");
        let mut session = Session::new();
        let mut host = Host::default();
        assert!(matches!(run(&mut session, &mut host, &source), Err(RuntimeError::InvalidParFor(_))));
        assert!(host.events.is_empty());
    }
}

#[test]
fn documented_graphics_example_emits_two_full_curves() {
    let mut session = Session::new();
    let mut host = Host::default();
    run(&mut session, &mut host, include_str!("../../../examples/graficos.sun")).unwrap();
    let figure = session.figure().unwrap();
    assert_eq!(figure.traces.len(), 2);
    assert_eq!(figure.traces[0].x.len(), 61);
    assert_eq!(figure.layout.title, "Seno e cosseno");
    assert_eq!(figure.layout.legend, ["Seno","Cosseno"]);
    assert_eq!(figure.layout.y_range, Some([-1.2,1.2]));
    assert_eq!(host.events.len(), 7);
}