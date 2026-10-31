use std::{collections::HashMap, io::{self, Cursor, Read}, path::Path, sync::{Arc, atomic::{AtomicBool, Ordering}}};
use sun_runtime::{RuntimeError, RuntimeHost, Session, Value};

#[derive(Default)]
struct Host {
    files: HashMap<String, Vec<u8>>, lines: Vec<String>, writes: usize,
    cancelled: Option<Arc<AtomicBool>>, cancel_on_read: bool, deny_writes: bool,
}
impl RuntimeHost for Host {
    fn write_line(&mut self, line: &str) -> io::Result<()> { self.lines.push(line.into()); Ok(()) }
    fn read_file(&mut self, path: &Path) -> io::Result<Box<dyn Read>> {
        if self.cancel_on_read { self.cancelled.as_ref().unwrap().store(true, Ordering::Release); }
        let bytes = self.files.get(path.to_str().unwrap()).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(Box::new(Cursor::new(bytes.clone())))
    }
    fn write_file_atomic(&mut self, path: &Path, contents: &[u8]) -> io::Result<()> {
        if self.deny_writes { return Err(io::Error::from(io::ErrorKind::PermissionDenied)); }
        self.writes += 1;
        self.files.insert(path.to_str().unwrap().into(), contents.to_vec());
        Ok(())
    }
    fn cancellation_flag(&self) -> Option<Arc<AtomicBool>> { self.cancelled.clone() }
}

fn run(session: &mut Session, host: &mut Host, source: &str) -> Result<(), RuntimeError> {
    session.execute(&sun_parser::parse(source).unwrap(), host)
}

#[test]
fn reads_bom_crlf_quoted_numeric_fields_and_writes_rows_without_headers() {
    let mut host = Host::default();
    host.files.insert("input.csv".into(), b"\xef\xbb\xbf 1 ,\"2\"\r\n-3.5,4e2\r\n".to_vec());
    let mut session = Session::new();
    run(&mut session, &mut host, "A=readmatrix(\"input.csv\"); writematrix(A,\"output.csv\"); B=readmatrix(\"output.csv\"); disp(B);").unwrap();
    assert_eq!(host.lines, ["[1, 2; -3.5, 400]"]);
    assert_eq!(host.files["output.csv"], b"1,2\n-3.5,400\n");
    assert_eq!(session.variables()["A"], session.variables()["B"]);
}

#[test]
fn malformed_csv_is_catchable_and_preserves_existing_bindings_and_files() {
    for bytes in [b"name,value\n1,2\n".as_slice(), b"1,2\n3\n", b"1,\n", b"1,nope\n", b"\xff,2\n", b"\"1\"0,2\n", b"1,\"2", b"1\"0,2\n"] {
        let mut host = Host::default();
        host.files.insert("bad.csv".into(), bytes.to_vec());
        let mut session = Session::new();
        run(&mut session, &mut host, "saved=42; try; saved=readmatrix(\"bad.csv\"); catch failure; disp(failure.identifier); end; disp(saved);").unwrap();
        assert_eq!(host.lines, ["Sun:InvalidCSV", "42"]);
        assert_eq!(host.writes, 0);
    }
}

#[test]
fn validates_arguments_before_writing_and_writematrix_has_no_output() {
    let mut host = Host::default();
    host.files.insert("existing.csv".into(), b"original".to_vec());
    let mut session = Session::new();
    for source in ["readmatrix();", "readmatrix(1);", "readmatrix(\"\");", "readmatrix(\"x\",1);", "writematrix([1]);", "writematrix(\"text\",\"existing.csv\");", "writematrix(true,\"existing.csv\");", "writematrix(zeros(0,3),\"existing.csv\");", "writematrix(zeros(3,0),\"existing.csv\");", "result=writematrix([1],\"existing.csv\");"] {
        assert!(run(&mut session, &mut host, source).is_err(), "accepted {source}");
        assert_eq!(host.files["existing.csv"], b"original");
    }
    assert_eq!(host.writes, 0);
    assert!(matches!(run(&mut session, &mut host, "readmatrix(\"missing.csv\");"), Err(RuntimeError::Io(_))));
}

#[test]
fn empty_csv_is_zero_by_zero_and_scalar_and_special_values_round_trip() {
    let mut host = Host::default();
    let mut session = Session::new();
    run(&mut session, &mut host, "writematrix([],\"empty.csv\"); E=readmatrix(\"empty.csv\"); disp(size(E)); writematrix(42,\"scalar.csv\"); disp(readmatrix(\"scalar.csv\")); writematrix([0/0,1/0,-1/0],\"special.csv\"); S=readmatrix(\"special.csv\"); disp(S);").unwrap();
    assert_eq!(host.files["empty.csv"], b"");
    assert_eq!(host.files["special.csv"], b"NaN,Inf,-Inf\n");
    assert_eq!(host.lines, ["[0, 0]", "[42]", "[NaN, Inf, -Inf]"]);
    assert!(matches!(session.variables()["E"], Value::Matrix(_)));
}

#[test]
fn default_host_atomically_replaces_a_real_file_without_leaving_temporary_files() {
    struct DiskHost;
    impl RuntimeHost for DiskHost { fn write_line(&mut self, _: &str) -> io::Result<()> { Ok(()) } }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("matrix.csv");
    std::fs::write(&path, "previous").unwrap();
    let filename = path.to_str().unwrap().replace('\\', "\\\\").replace('"', "\\\"");
    let module = sun_parser::parse(&format!("writematrix([1,2;3,4],\"{filename}\"); result=readmatrix(\"{filename}\");")).unwrap();
    let mut session = Session::new();
    session.execute(&module, &mut DiskHost).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"1,2\n3,4\n");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn csv_operations_are_rejected_in_parallel_bodies_and_headers_before_effects() {
    for source in [
        "writematrix([99],\"output.csv\"); A=zeros(1,2); parfor index=1:2; A(index)=sum(readmatrix(\"input.csv\")); end",
        "A=zeros(1,2); parfor index=1:2; temporary=writematrix([1],\"output.csv\"); A(index)=1; end",
        "A=zeros(1,2); parfor index=1:sum(readmatrix(\"input.csv\")); A(index)=1; end",
    ] {
        let mut host = Host::default();
        let mut session = Session::new();
        assert!(matches!(run(&mut session, &mut host, source), Err(RuntimeError::InvalidParFor(_))));
        assert_eq!(host.writes, 0);
        assert!(session.variables().is_empty());
    }
}

#[test]
fn cancelled_reads_bypass_catch_and_leave_the_session_usable() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut host = Host { cancelled: Some(flag.clone()), cancel_on_read: true, ..Host::default() };
    host.files.insert("input.csv".into(), b"1,2\n3,4\n".to_vec());
    let mut session = Session::new();
    let result = run(&mut session, &mut host, "saved=42; try; saved=readmatrix(\"input.csv\"); catch; disp(\"not reached\"); end");
    assert!(matches!(result, Err(RuntimeError::Cancelled)));
    assert!(host.lines.is_empty());
    assert_eq!(session.variables()["saved"], Value::Number(42.0));
    flag.store(false, Ordering::Release);
    host.cancel_on_read = false;
    run(&mut session, &mut host, "A=readmatrix(\"input.csv\"); disp(A);").unwrap();
    assert_eq!(host.lines, ["[1, 2; 3, 4]"]);
}

#[test]
fn denied_writes_are_io_errors_and_keep_existing_contents() {
    let mut host = Host { deny_writes: true, ..Host::default() };
    host.files.insert("output.csv".into(), b"previous".to_vec());
    let mut session = Session::new();
    run(&mut session, &mut host, "try; writematrix([1],\"output.csv\"); catch failure; disp(failure.identifier); end").unwrap();
    assert_eq!(host.lines, ["Sun:IOError"]);
    assert_eq!(host.files["output.csv"], b"previous");
}

#[test]
fn float_values_round_trip_without_losing_f64_precision_or_signed_zero() {
    let mut host = Host::default();
    let mut session = Session::new();
    run(&mut session, &mut host, "A=[0.1,-0.0,1e-300,1e300,9007199254740991]; writematrix(A,\"precise.csv\"); B=readmatrix(\"precise.csv\");").unwrap();
    let (Value::Matrix(left), Value::Matrix(right)) = (&session.variables()["A"], &session.variables()["B"]) else { panic!("expected matrices") };
    for index in 1..=left.element_count() { assert_eq!(left.get_linear(index).unwrap().to_bits(), right.get_linear(index).unwrap().to_bits()); }
}

#[test]
fn excessive_columns_and_record_bytes_are_rejected_before_numeric_allocation() {
    for bytes in [vec![b','; 65_536], vec![b'1'; 1024 * 1024 + 1]] {
        let mut host = Host::default();
        host.files.insert("oversized.csv".into(), bytes);
        let mut session = Session::new();
        assert!(matches!(run(&mut session, &mut host, "readmatrix(\"oversized.csv\");"), Err(RuntimeError::Csv(_))));
    }
    let mut host = Host::default();
    let mut session = Session::new();
    for source in ["writematrix(ones(1,65537),\"wide.csv\");", "writematrix(ones(1,65536)*1e300,\"large.csv\");"] {
        assert!(matches!(run(&mut session, &mut host, source), Err(RuntimeError::Csv(_))));
    }
    assert_eq!(host.writes, 0);
}

#[cfg(unix)]
#[test]
fn atomic_writes_preserve_permissions_and_reject_symlink_destinations() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    struct DiskHost;
    impl RuntimeHost for DiskHost { fn write_line(&mut self, _: &str) -> io::Result<()> { Ok(()) } }
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.csv");
    std::fs::write(&target, b"original").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    DiskHost.write_file_atomic(&target, b"1,2\n").unwrap();
    assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o640);
    let link = directory.path().join("link.csv");
    symlink(&target, &link).unwrap();
    assert!(DiskHost.write_file_atomic(&link, b"changed").is_err());
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read(&target).unwrap(), b"1,2\n");
    assert!(DiskHost.read_file(directory.path()).is_err());
}