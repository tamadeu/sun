use std::io;
use sun_runtime::{FigureEvent, RuntimeError, RuntimeHost, Scene3D, Session};

#[derive(Default)]
struct Host(Vec<FigureEvent>);
impl RuntimeHost for Host {
    fn write_line(&mut self, _: &str) -> io::Result<()> { Ok(()) }
    fn figure_event(&mut self, event: &FigureEvent) -> io::Result<()> { self.0.push(event.clone()); Ok(()) }
}
fn run(session: &mut Session, host: &mut Host, source: &str) -> Result<(), RuntimeError> {
    session.execute(&sun_parser::parse(source).unwrap(), host)
}

#[test]
fn plot3_builds_curves_and_preserves_nonfinite_gaps() {
    let mut session = Session::new(); let mut host = Host::default();
    run(&mut session, &mut host, "plot3(1:4,[1,2,0/0,4],[0,1,2,3]); zlabel(\"Height\"); axis([0,5,0,5,0,4]);").unwrap();
    let figure = session.figure().unwrap();
    let Some(Scene3D::Curves { curves, bounds }) = &figure.scene else { panic!("expected 3D curves") };
    assert_eq!(curves[0].vertices, [[1.0,1.0,0.0],[2.0,2.0,1.0],[4.0,4.0,3.0]]);
    assert_eq!(curves[0].segments, [[0,1]]);
    assert_eq!(bounds[2], [0.0,3.0]);
    assert_eq!(figure.layout.z_label, "Height");
    assert_eq!(figure.layout.z_range, Some([0.0,4.0]));
}

#[test]
fn surfaces_accept_vectors_grids_and_default_coordinates() {
    for source in ["surf([1,2;3,4]);", "surf([1,2],[1,2],[1,2;3,4]);", "surf([1,2;1,2],[1,1;2,2],[1,2;3,4]);"] {
        let mut session = Session::new(); let mut host = Host::default();
        run(&mut session, &mut host, source).unwrap();
        let Some(Scene3D::Surface { vertices, triangles, rows, cols, .. }) = &session.figure().unwrap().scene else { panic!("expected surface") };
        assert_eq!(vertices.len(), 4); assert_eq!(triangles.len(), 2); assert_eq!((*rows,*cols), (2,2));
        assert_eq!(vertices[3], [2.0,2.0,4.0]);
    }
}

#[test]
fn invalid_3d_calls_do_not_replace_the_current_figure() {
    let mut session = Session::new(); let mut host = Host::default();
    run(&mut session, &mut host, "plot(1,2);").unwrap(); let previous = session.figure().unwrap().clone();
    for source in ["plot3(1,2);", "plot3(1:2,1:3,1:2);", "plot3([],[],[]);", "plot3(true,1,2);", "plot3([0/0],1,2);", "plot3(1:10001,1:10001,1:10001);", "surf([1,2]);", "surf([1,2],[1,2,3],[1,2;3,4]);", "surf([0,0],[0,0],ones(2));", "surf(ones(101,100));", "zlabel(\"bad\");"] {
        assert!(run(&mut session, &mut host, source).is_err(), "accepted {source}");
        assert_eq!(session.figure().unwrap(), &previous);
    }
    assert_eq!(host.0.len(), 1);
}

#[test]
fn matrix_columns_create_multiple_3d_curves_and_labels_persist() {
    let mut session = Session::new(); let mut host = Host::default();
    run(&mut session, &mut host, "plot3([1,2;3,4],[2,3;4,5],[0,1;1,2]);").unwrap();
    run(&mut session, &mut host, "legend(\"First\",\"Second\"); grid(true); zlabel(\"Z\"); axis(\"auto\");").unwrap();
    let Some(Scene3D::Curves { curves, .. }) = &session.figure().unwrap().scene else { panic!("expected curves") };
    assert_eq!(curves.len(), 2); assert_eq!(curves[1].vertices[1], [4.0,5.0,2.0]);
    assert_eq!(session.figure().unwrap().layout.legend, ["First","Second"]);
}

#[test]
fn isolated_extreme_vertices_do_not_reject_or_shrink_valid_surface_cells() {
    let mut session = Session::new(); let mut host = Host::default();
    run(&mut session, &mut host, "X=[0,1,0;0,1,0;0,0,1e300]; Y=[0,0,0;1,1,0;0,0,1e300]; Z=[0,0,0/0;0,0,0/0;0/0,0/0,0]; surf(X,Y,Z);").unwrap();
    let Some(Scene3D::Surface { vertices, triangles, bounds, .. }) = &session.figure().unwrap().scene else { panic!("expected surface") };
    assert_eq!(vertices.len(),4); assert_eq!(triangles.len(),2);
    assert_eq!(*bounds, [[0.0,1.0],[0.0,1.0],[0.0,0.0]]);
}

#[test]
fn documented_curve_example_emits_a_full_helix() {
    let mut session = Session::new(); let mut host = Host::default();
    run(&mut session, &mut host, include_str!("../../../examples/graficos_3d.sun")).unwrap();
    let Some(Scene3D::Curves { curves, .. }) = &session.figure().unwrap().scene else { panic!("expected curves") };
    assert_eq!(curves[0].vertices.len(),121);assert_eq!(curves[0].segments.len(),120);
    assert_eq!(session.figure().unwrap().layout.z_label,"Altura");
}

#[test]
fn documented_surface_example_emits_a_bounded_paraboloid() {
    let mut session = Session::new(); let mut host = Host::default();
    run(&mut session, &mut host, include_str!("../../../examples/superficie_3d.sun")).unwrap();
    let Some(Scene3D::Surface { vertices, triangles, rows, cols, .. }) = &session.figure().unwrap().scene else { panic!("expected surface") };
    assert_eq!((*rows,*cols),(21,21));assert_eq!(vertices.len(),441);assert_eq!(triangles.len(),800);
    assert_eq!(session.figure().unwrap().layout.z_range,Some([0.0,8.0]));
}

#[test]
fn triangle_validation_preserves_cells_with_very_different_axis_scales() {
    let mut session = Session::new(); let mut host = Host::default();
    run(&mut session, &mut host, "surf([0,1e300;0,1e300],[0,1e-300;1e-300,2e-300],zeros(2));").unwrap();
    let Some(Scene3D::Surface { vertices, triangles, .. }) = &session.figure().unwrap().scene else { panic!("expected surface") };
    assert_eq!(vertices.len(),4);assert_eq!(triangles.len(),2);
}