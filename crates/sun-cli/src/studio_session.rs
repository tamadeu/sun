use std::{collections::HashMap, io::{self, BufRead, Read, Write}, path::PathBuf,
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, RecvTimeoutError, Sender}}, time::{Duration, Instant}};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use sun_parser::parse;
use sun_runtime::{CancellationFlag, RuntimeHost, Session, validate_parallel_blocks};
use super::studio::{emit, parse_diagnostic, variable, WORKSPACE_LIMIT};

const SOURCE_LIMIT: usize = 1024 * 1024;
const LINE_LIMIT: usize = 8 * SOURCE_LIMIT;
type Jobs = Arc<Mutex<HashMap<String, CancellationFlag>>>;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Execute { id: String, source: String, #[serde(default)] check: bool, cwd: Option<PathBuf> },
    Reset { id: String },
    Input { id: String, text: String },
    Cancel { id: String },
    Shutdown,
}

enum Control { Input { id: String }, Cancel { id: String }, Shutdown }
enum Message { Job { request: Request, flag: CancellationFlag }, Failure(String), Shutdown }

fn read_request(input: &mut impl BufRead) -> io::Result<Option<Request>> {
    let mut line = Vec::new();
    if input.take((LINE_LIMIT + 1) as u64).read_until(b'\n', &mut line)? == 0 { return Ok(None); }
    if line.len() > LINE_LIMIT { return Err(io::Error::new(io::ErrorKind::InvalidData, "session request exceeds limit")); }
    let request: Request = serde_json::from_slice(&line).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let id = match &request {
        Request::Execute { id, source, .. } => {
            if source.len() > SOURCE_LIMIT { return Err(io::Error::new(io::ErrorKind::InvalidData, "source exceeds 1 MiB")); }
            Some(id)
        }
        Request::Input { id, text } => {
            if text.len() > 4096 { return Err(io::Error::new(io::ErrorKind::InvalidData, "input exceeds limit")); }
            Some(id)
        }
        Request::Reset { id } | Request::Cancel { id } => Some(id),
        Request::Shutdown => None,
    };
    if id.is_some_and(|id| id.is_empty() || id.len() > 80 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid session request id"));
    }
    Ok(Some(request))
}

fn cancel_all(jobs: &Jobs) {
    if let Ok(jobs) = jobs.lock() { for flag in jobs.values() { flag.store(true, Ordering::Release); } }
}

fn read_commands(mut input: impl BufRead, messages: Sender<Message>, controls: Sender<Control>, jobs: Jobs) {
    let result = (|| -> io::Result<()> {
        while let Some(request) = read_request(&mut input)? {
            match request {
                Request::Input { id, .. } => { let _ = controls.send(Control::Input { id }); }
                Request::Cancel { id } => {
                    if let Ok(jobs) = jobs.lock() {
                        if let Some(flag) = jobs.get(&id) { flag.store(true, Ordering::Release); }
                    }
                    let _ = controls.send(Control::Cancel { id });
                }
                Request::Shutdown => { let _ = messages.send(Message::Shutdown); break; }
                request @ (Request::Execute { .. } | Request::Reset { .. }) => {
                    let id = match &request { Request::Execute { id, .. } | Request::Reset { id } => id.clone(), _ => unreachable!() };
                    let flag = Arc::new(AtomicBool::new(false));
                    {
                        let mut jobs = jobs.lock().map_err(|_| io::Error::other("session state unavailable"))?;
                        if !jobs.is_empty() { return Err(io::Error::new(io::ErrorKind::InvalidData, "only one active request is supported")); }
                        jobs.insert(id, flag.clone());
                    }
                    if messages.send(Message::Job { request, flag }).is_err() { break; }
                }
            }
        }
        Ok(())
    })();
    cancel_all(&jobs);
    let _ = controls.send(Control::Shutdown);
    if let Err(error) = result { let _ = messages.send(Message::Failure(error.to_string())); }
}

fn emit_job(output: &mut impl Write, id: &str, mut event: JsonValue) -> io::Result<()> {
    event.as_object_mut().expect("session events are objects").insert("jobId".into(), json!(id));
    emit(output, event)
}

struct SessionHost<'a, Output: Write> { id: &'a str, flag: CancellationFlag, controls: &'a Receiver<Control>, output: &'a mut Output }

impl<Output: Write> SessionHost<'_, Output> {
    fn interrupted(&self) -> io::Result<()> {
        if self.flag.load(Ordering::Acquire) { Err(io::Error::new(io::ErrorKind::Interrupted, "execution cancelled")) } else { Ok(()) }
    }

    fn control(&self, control: Control) -> io::Result<bool> {
        match control {
            Control::Input { id } => Ok(id == self.id),
            Control::Cancel { id } if id == self.id => Err(io::Error::new(io::ErrorKind::Interrupted, "execution cancelled")),
            Control::Shutdown => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "session input closed")),
            _ => Ok(false),
        }
    }
}

impl<Output: Write> RuntimeHost for SessionHost<'_, Output> {
    fn figure_event(&mut self, event: &sun_runtime::FigureEvent) -> io::Result<()> {
        self.interrupted()?;
        emit_job(self.output, self.id, serde_json::to_value(event).map_err(io::Error::other)?)
    }

    fn cancellation_flag(&self) -> Option<CancellationFlag> { Some(self.flag.clone()) }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.interrupted()?;
        emit_job(self.output, self.id, json!({"type":"output","text":format!("{line}\n")}))
    }

    fn wait_for_input(&mut self) -> io::Result<()> {
        self.interrupted()?;
        emit_job(self.output, self.id, json!({"type":"input","prompt":"Execucao pausada."}))?;
        loop {
            let control = self.controls.recv().map_err(|_| io::Error::new(io::ErrorKind::UnexpectedEof, "session input closed"))?;
            self.interrupted()?;
            if self.control(control)? { break; }
        }
        emit_job(self.output, self.id, json!({"type":"resumed"}))
    }

    fn pause(&mut self, duration: Duration) -> io::Result<()> {
        let deadline = Instant::now().checked_add(duration).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "pause duration exceeds clock range"))?;
        loop {
            self.interrupted()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() { return Ok(()); }
            match self.controls.recv_timeout(remaining) {
                Ok(control) => { self.control(control)?; }
                Err(RecvTimeoutError::Timeout) => return Ok(()),
                Err(RecvTimeoutError::Disconnected) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "session input closed")),
            }
        }
    }
}

fn workspace(session: &Session, id: &str, output: &mut impl Write) -> io::Result<()> {
    let mut entries: Vec<_> = session.variables().iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    let variables: Vec<_> = entries.iter().take(WORKSPACE_LIMIT).map(|(name, value)| variable(name, value)).collect();
    let mut definitions: Vec<_> = session.functions().collect();
    definitions.sort_by(|left, right| left.name.cmp(&right.name));
    let functions: Vec<String> = definitions.iter().take(WORKSPACE_LIMIT).map(|function| {
        let output = function.output.as_ref().map(|name| format!("{name} = ")).unwrap_or_default();
        format!("{output}{}({})", function.name, function.parameters.join(", ")).chars().take(512).collect()
    }).collect();
    emit_job(output, id, json!({"type":"workspace","variables":variables,"functions":functions,"hasFigure":session.figure().is_some()}))?;
    if entries.len() > WORKSPACE_LIMIT {
        emit_job(output, id, json!({"type":"diagnostic","diagnostic":{"message":"Workspace limitado a 200 variaveis.","code":"Sun:WorkspaceLimit","severity":"warning"}}))?;
    }
    if definitions.len() > WORKSPACE_LIMIT {
        emit_job(output, id, json!({"type":"diagnostic","diagnostic":{"message":"Workspace limitado a 200 funcoes.","code":"Sun:WorkspaceLimit","severity":"warning"}}))?;
    }
    Ok(())
}

fn execute_request(session: &mut Session, request: &Request, flag: CancellationFlag, controls: &Receiver<Control>, output: &mut impl Write) -> io::Result<(bool, bool)> {
    let id = match request { Request::Execute { id, .. } | Request::Reset { id } => id, _ => unreachable!() };
    let mut ok = true;
    if !flag.load(Ordering::Acquire) {
        match request {
            Request::Reset { .. } => {
                session.clear();
                emit_job(output, id, json!({"type":"figures_cleared"}))?;
            }
            Request::Execute { source, check, cwd, .. } => {
                let result = (|| -> Result<(), JsonValue> {
                    if !check {
                        if let Some(cwd) = cwd {
                            std::env::set_current_dir(cwd).map_err(|error| json!({"type":"diagnostic","diagnostic":{"message":error.to_string(),"code":"Sun:IOError"}}))?;
                        }
                    }
                    let module = parse(source).map_err(|error| parse_diagnostic(source, &error))?;
                    validate_parallel_blocks(&module).map_err(|error| json!({"type":"diagnostic","diagnostic":{"message":error.to_string(),"code":error.identifier()}}))?;
                    if !check {
                        session.execute(&module, &mut SessionHost { id, flag: flag.clone(), controls, output }).map_err(|error| {
                            json!({"type":"diagnostic","diagnostic":{"message":error.to_string(),"code":error.identifier()}})
                        })?;
                    }
                    Ok(())
                })();
                if let Err(event) = result {
                    ok = false;
                    if !flag.load(Ordering::Acquire) { emit_job(output, id, event)?; }
                }
            }
            _ => unreachable!(),
        }
    }
    workspace(session, id, output)?;
    let cancelled = flag.load(Ordering::Acquire);
    Ok((ok && !cancelled, cancelled))
}

pub fn run() -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    emit(&mut output, json!({"type":"ready","protocol":2,"version":env!("CARGO_PKG_VERSION")}))?;
    let (messages_tx, messages) = mpsc::channel();
    let (controls_tx, controls) = mpsc::channel();
    let jobs: Jobs = Arc::new(Mutex::new(HashMap::new()));
    let reader_jobs = jobs.clone();
    std::thread::spawn(move || read_commands(io::BufReader::new(io::stdin()), messages_tx, controls_tx, reader_jobs));
    let mut session = Session::new();
    while let Ok(message) = messages.recv() {
        match message {
            Message::Job { request, flag } => {
                let id = match &request { Request::Execute { id, .. } | Request::Reset { id } => id.clone(), _ => unreachable!() };
                let (ok, cancelled) = execute_request(&mut session, &request, flag, &controls, &mut output)?;
                jobs.lock().map_err(|_| io::Error::other("session state unavailable"))?.remove(&id);
                emit_job(&mut output, &id, json!({"type":"finished","ok":ok,"cancelled":cancelled}))?;
            }
            Message::Failure(message) => return Err(io::Error::new(io::ErrorKind::InvalidData, message)),
            Message::Shutdown => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn execute(session: &mut Session, id: &str, source: &str, check: bool) -> Vec<JsonValue> {
        let mut output = Vec::new();
        let (_sender, controls) = mpsc::channel();
        let flag = Arc::new(AtomicBool::new(false));
        let (ok, cancelled) = execute_request(session, &Request::Execute { id: id.into(), source: source.into(), check, cwd: None }, flag, &controls, &mut output).unwrap();
        emit_job(&mut output, id, json!({"type":"finished","ok":ok,"cancelled":cancelled})).unwrap();
        String::from_utf8(output).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }

    #[test]
    fn figure_state_and_layout_events_persist_and_reset_explicitly() {
        let mut session = Session::new();
        let first = execute(&mut session, "plot", "plot(1:3,4:6);", false);
        assert_eq!(first[0]["type"], "figure");
        assert_eq!(first[1]["hasFigure"], true);
        let update = execute(&mut session, "labels", "title(\"Signal\"); legend(\"Series\");", false);
        assert_eq!(update[0]["type"], "figure_update");
        assert!(update.iter().all(|event| event["jobId"] == "labels"));
        assert!(execute(&mut session, "check", "clf();", true).iter().all(|event| event["type"] != "figures_cleared"));
        assert_eq!(session.figure().unwrap().layout.title, "Signal");
        let (_sender, controls) = mpsc::channel();
        let mut output = Vec::new();
        execute_request(&mut session, &Request::Reset { id: "reset".into() }, Arc::new(AtomicBool::new(false)), &controls, &mut output).unwrap();
        let events: Vec<JsonValue> = String::from_utf8(output).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(events[0]["type"], "figures_cleared");
        assert_eq!(events[1]["hasFigure"], false);
        assert!(session.figure().is_none());
    }

    #[test]
    fn functions_persist_check_does_not_register_and_reset_removes_them() {
        let mut session = Session::new();
        let defined = execute(&mut session, "define", "function result=square(value); result=value*value; end", false);
        let workspace = defined.iter().find(|event| event["type"] == "workspace").unwrap();
        assert_eq!(workspace["functions"], json!(["result = square(value)"]));
        assert_eq!(workspace["variables"], json!([]));
        execute(&mut session, "check", "function result=square(value); result=999; end", true);
        let called = execute(&mut session, "call", "disp(square(5));", false);
        assert_eq!(called[0]["text"], "25\n");
        execute(&mut session, "error", "error(\"stop\");", false);
        assert_eq!(execute(&mut session, "again", "disp(square(6));", false)[0]["text"], "36\n");
        let (_sender, controls) = mpsc::channel();
        let mut output = Vec::new();
        execute_request(&mut session, &Request::Reset { id: "reset".into() }, Arc::new(AtomicBool::new(false)), &controls, &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        let event: JsonValue = serde_json::from_str(text.lines().last().unwrap()).unwrap();
        assert_eq!(event["functions"], json!([]));
        assert_eq!(execute(&mut session, "cleared", "square(1);", false).last().unwrap()["ok"], false);
    }

    #[test]
    fn requests_share_variables_without_replaying_prior_output() {
        let mut session = Session::new();
        let first = execute(&mut session, "first", "value = 1; disp(value);", false);
        let second = execute(&mut session, "second", "value = value + 1; disp(value);", false);
        assert_eq!(first[0]["text"], "1\n");
        assert_eq!(second[0]["text"], "2\n");
        assert!(second.iter().all(|event| event["jobId"] == "second"));
        assert_eq!(second.last().unwrap()["ok"], true);
    }

    #[test]
    fn syntax_check_and_runtime_errors_do_not_destroy_session() {
        let mut session = Session::new();
        execute(&mut session, "setup", "value = 7;", false);
        assert_eq!(execute(&mut session, "syntax", "value = ;", false).last().unwrap()["ok"], false);
        execute(&mut session, "check", "value = 99;", true);
        let failed = execute(&mut session, "error", "value = 8; error(\"stop\");", false);
        assert_eq!(failed.last().unwrap()["ok"], false);
        let next = execute(&mut session, "next", "disp(value);", false);
        assert_eq!(next[0]["text"], "8\n");
    }

    #[test]
    fn input_is_job_scoped_and_reset_clears_workspace() {
        let mut session = Session::new();
        let (sender, controls) = mpsc::channel();
        sender.send(Control::Input { id: "old".into() }).unwrap();
        sender.send(Control::Input { id: "pause".into() }).unwrap();
        let mut output = Vec::new();
        let request = Request::Execute { id: "pause".into(), source: "pause(); value = 7;".into(), check: false, cwd: None };
        assert_eq!(execute_request(&mut session, &request, Arc::new(AtomicBool::new(false)), &controls, &mut output).unwrap(), (true, false));
        execute_request(&mut session, &Request::Reset { id: "reset".into() }, Arc::new(AtomicBool::new(false)), &controls, &mut output).unwrap();
        assert!(session.variables().is_empty());
    }

    #[test]
    fn cancelled_request_preserves_prior_workspace() {
        let mut session = Session::new();
        execute(&mut session, "setup", "value = 7;", false);
        let (_sender, controls) = mpsc::channel();
        let mut output = Vec::new();
        let request = Request::Execute { id: "cancel".into(), source: "value = 99;".into(), check: false, cwd: None };
        assert_eq!(execute_request(&mut session, &request, Arc::new(AtomicBool::new(true)), &controls, &mut output).unwrap(), (false, true));
        assert!(matches!(session.variables().get("value"), Some(sun_runtime::Value::Number(7.0))));
    }

    #[test]
    fn rejects_bad_ids_commands_and_extra_fields() {
        for line in [r#"{"type":"execute","id":"bad id","source":""}"#, r#"{"type":"unknown"}"#,
            r#"{"type":"reset","id":"reset","extra":1}"#] {
            assert!(read_request(&mut io::Cursor::new(line)).is_err());
        }
    }

    #[test]
    fn check_does_not_change_the_process_working_directory() {
        let mut session = Session::new();
        let (_sender, controls) = mpsc::channel();
        let request = Request::Execute { id: "check".into(), source: "error(\"not executed\");".into(), check: true,
            cwd: Some(PathBuf::from("/sun-studio-directory-that-does-not-exist")) };
        assert_eq!(execute_request(&mut session, &request, Arc::new(AtomicBool::new(false)), &controls, &mut Vec::new()).unwrap(), (true, false));
    }
}