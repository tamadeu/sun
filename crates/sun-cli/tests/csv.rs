use std::{io::Write, process::{Command, Stdio}};

#[test]
fn run_resolves_relative_csv_paths_and_check_has_no_file_effects() {
    let directory = tempfile::tempdir().unwrap();
    let source = "A=readmatrix(\"input.csv\"); writematrix(A*2,\"output.csv\"); B=readmatrix(\"output.csv\"); disp(B);";
    std::fs::write(directory.path().join("program.sun"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sun")).current_dir(directory.path()).args(["check","program.sun"]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!directory.path().join("output.csv").exists());
    std::fs::write(directory.path().join("input.csv"), "1,2\r\n3,4\r\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sun")).current_dir(directory.path()).args(["run","program.sun"]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "[2, 4; 6, 8]\n");
    assert_eq!(std::fs::read(directory.path().join("output.csv")).unwrap(), b"2,4\n6,8\n");
}

#[test]
fn repl_shares_imported_matrices_and_exports_later_commands() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("input.csv"), "1,2\n3,4\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_sun")).current_dir(directory.path()).arg("repl")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"A=readmatrix(\"input.csv\");\nwritematrix(A*3,\"output.csv\");\ndisp(sum(A));\n:quit\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8(output.stdout).unwrap().contains("10\n"));
    assert_eq!(std::fs::read(directory.path().join("output.csv")).unwrap(), b"3,6\n9,12\n");
}

#[test]
fn documented_csv_example_runs_in_an_isolated_directory() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("example.sun"), include_str!("../../../examples/csv.sun")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sun")).current_dir(directory.path()).args(["run","example.sun"]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "[1, 2; 3, 4]\n[2, 4; 6, 8]\n20\n");
    assert_eq!(std::fs::read(directory.path().join("matriz_exportada.csv")).unwrap(), b"1,2\n3,4\n");
    assert_eq!(std::fs::read(directory.path().join("matriz_dobrada.csv")).unwrap(), b"2,4\n6,8\n");
}