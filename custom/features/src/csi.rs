//! CSiBridge v25: the selection dialog's state and the data source behind it
//! (workflow and API: docs/08-csi-bridge.md in the Factory repository).
//!
//! The source is the csi-agent (a separate program using CSi's API, protocol in
//! docs/09-agent-protocol.md). `FACTORY_CSI_DEMO=1` uses clearly marked demo data instead.
//! Talking to the agent runs on a worker thread, so the UI never waits for CSiBridge.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, PoisonError, RwLock};

use serde_json::{Value, json};

use crate::model::{self, LoadCase, LoadCombination};

/// Run status of a load case (`Analyze.GetCaseStatus`: 1 Not run, 2 Could not start,
/// 3 Not finished, 4 Finished).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseStatus {
    NotRun,
    CouldNotStart,
    NotFinished,
    Finished,
}

impl CaseStatus {
    /// From the API's status number.
    pub fn from_api(status: i32) -> Self {
        match status {
            4 => Self::Finished,
            3 => Self::NotFinished,
            2 => Self::CouldNotStart,
            _ => Self::NotRun,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NotRun => "not run",
            Self::CouldNotStart => "could not start",
            Self::NotFinished => "not finished",
            Self::Finished => "finished",
        }
    }
}

/// A running CSiBridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub pid: u32,
    /// Its window title (program version and model name).
    pub title: String,
}

/// What CSiBridge reports when the dialog opens.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// e.g. "CSiBridge 25.3.1".
    pub program: String,
    /// The model file.
    pub model: String,
    /// Selected area objects (`GetSelected`, ObjectType 5).
    pub shells: usize,
    /// Selected frame objects (`GetSelected`, ObjectType 2).
    pub frames: usize,
    /// Load cases: name, type, run status.
    pub cases: Vec<(String, String, CaseStatus)>,
    /// Load combinations: name, type, (load case or combination, scale factor).
    pub combos: Vec<(String, String, Vec<(String, f64)>)>,
}

/// Where the dialog's data comes from.
pub trait Source: Send + Sync {
    /// Shown in the dialog title ("CSiBridge", or a demo marker).
    fn title(&self) -> String;
    /// The running instances.
    fn instances(&self) -> Result<Vec<Instance>, String>;
    /// What the instance `pid` reports.
    fn read(&self, pid: u32) -> Result<Snapshot, String>;
    /// The agent that transfers geometry and results; `None` = this source cannot (demo).
    fn agent(&self) -> Option<PathBuf> {
        None
    }
}

/// The csi-agent (agents/csi-agent in the Factory repository).
pub struct AgentSource {
    pub exe: Option<PathBuf>,
}

impl AgentSource {
    /// Finds the agent: `FACTORY_CSI_AGENT`, else `csi-agent\csi-agent.exe` or `csi-agent.exe`
    /// next to Factory's executable.
    pub fn find() -> Self {
        let from_env = std::env::var_os("FACTORY_CSI_AGENT").map(PathBuf::from).filter(|p| p.is_file());
        let next_to_exe = || {
            let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
            [dir.join("csi-agent").join("csi-agent.exe"), dir.join("csi-agent.exe")].into_iter().find(|p| p.is_file())
        };
        Self { exe: from_env.or_else(next_to_exe) }
    }

    /// Send one request; the response object when `ok`, else its error text.
    fn call(&self, mut request: Value) -> Result<Value, String> {
        let exe = self
            .exe
            .as_ref()
            .ok_or("csi-agent.exe was not found next to Factory (or in FACTORY_CSI_AGENT). Build it: dotnet build agents\\csi-agent -c Release")?;
        let mut command = Command::new(exe);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|e| format!("Could not start csi-agent: {e}"))?;
        request["v"] = json!(1);
        request["id"] = json!(1);
        {
            let mut stdin = child.stdin.take().ok_or("csi-agent has no input")?;
            writeln!(stdin, "{request}").map_err(|e| format!("csi-agent: {e}"))?;
        } // closing stdin ends the agent after its answer
        let stdout = child.stdout.take().ok_or("csi-agent has no output")?;
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).map_err(|e| format!("csi-agent: {e}"))?;
        let _ = child.wait();
        let response: Value = serde_json::from_str(&line).map_err(|_| format!("csi-agent gave no valid answer: {}", line.trim()))?;
        if response["v"] != json!(1) {
            return Err(format!("csi-agent speaks protocol {} (Factory speaks 1)", response["v"]));
        }
        if response["ok"] == json!(true) {
            Ok(response)
        } else {
            Err(response["error"].as_str().unwrap_or("csi-agent failed").to_string())
        }
    }
}

impl Source for AgentSource {
    fn title(&self) -> String {
        "CSiBridge".into()
    }

    fn instances(&self) -> Result<Vec<Instance>, String> {
        let r = self.call(json!({"cmd": "instances"}))?;
        Ok(r["instances"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|i| Instance { pid: i["pid"].as_u64().unwrap_or(0) as u32, title: i["title"].as_str().unwrap_or("").trim().to_string() })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn read(&self, pid: u32) -> Result<Snapshot, String> {
        parse_snapshot(&self.call(json!({"cmd": "snapshot", "pid": pid}))?)
    }

    fn agent(&self) -> Option<PathBuf> {
        self.exe.clone()
    }
}

/// A `snapshot` response (docs/09-agent-protocol.md) as a [`Snapshot`].
pub fn parse_snapshot(r: &Value) -> Result<Snapshot, String> {
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    let count = |v: &Value| v.as_u64().unwrap_or(0) as usize;
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    Ok(Snapshot {
        program: text(&r["program"]),
        model: text(&r["model"]),
        shells: count(&r["shells"]),
        frames: count(&r["frames"]),
        cases: list(&r["cases"])
            .iter()
            .map(|c| (text(&c["name"]), text(&c["type"]), CaseStatus::from_api(c["status"].as_i64().unwrap_or(1) as i32)))
            .collect(),
        combos: list(&r["combos"])
            .iter()
            .map(|c| (text(&c["name"]), text(&c["type"]), list(&c["items"]).iter().map(|i| (text(&i["name"]), i["factor"].as_f64().unwrap_or(0.0))).collect()))
            .collect(),
    })
}

/// Demo data, marked as such everywhere it appears. Not from any real model.
pub struct DemoSource;

impl Source for DemoSource {
    fn title(&self) -> String {
        "CSiBridge – DEMO DATA".into()
    }

    fn instances(&self) -> Result<Vec<Instance>, String> {
        Ok(vec![Instance { pid: 1, title: "DEMO".into() }])
    }

    fn read(&self, _pid: u32) -> Result<Snapshot, String> {
        let case = |n: &str, t: &str, s| (n.to_string(), t.to_string(), s);
        let combo = |n: &str, f: &[(&str, f64)]| (n.to_string(), "Linear Additive".to_string(), f.iter().map(|(c, x)| (c.to_string(), *x)).collect());
        Ok(Snapshot {
            program: "DEMO".into(),
            model: "demo data, not from any model".into(),
            shells: 8000,
            frames: 1060,
            cases: vec![
                case("DEMO-DEAD", "Linear Static", CaseStatus::Finished),
                case("DEMO-SDL", "Linear Static", CaseStatus::Finished),
                case("DEMO-LM1", "Moving Load", CaseStatus::Finished),
                case("DEMO-TEMP", "Linear Static", CaseStatus::NotRun),
            ],
            combos: vec![
                combo("DEMO-ULS", &[("DEMO-DEAD", 1.35), ("DEMO-SDL", 1.35), ("DEMO-LM1", 1.35)]),
                combo("DEMO-SLS", &[("DEMO-DEAD", 1.0), ("DEMO-SDL", 1.0), ("DEMO-LM1", 1.0)]),
            ],
        })
    }
}

/// The source in use: demo when `FACTORY_CSI_DEMO=1`, else the agent.
pub fn source() -> Box<dyn Source> {
    if std::env::var("FACTORY_CSI_DEMO").is_ok_and(|v| v == "1") { Box::new(DemoSource) } else { Box::new(AgentSource::find()) }
}

/// One line with a check box.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub name: String,
    pub kind: String,
    pub checked: bool,
    /// Why it cannot be ticked (e.g. "not run"); `None` = can be ticked.
    pub blocked: Option<String>,
    /// For combinations: their content.
    pub factors: Vec<(String, f64)>,
}

/// The dialog's stage.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    #[default]
    Closed,
    /// Waiting for CSiBridge (what is being done).
    Loading(String),
    /// Several CSiBridge are running: which one?
    Choose(Vec<Instance>),
    Error(String),
    Ready,
    /// Geometry and results are on their way (see [`crate::transfer`]).
    Sending,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dialog {
    pub phase: Phase,
    pub title: String,
    /// The CSiBridge process and the agent that reads it (`None` for demo data).
    pub pid: u32,
    pub agent: Option<PathBuf>,
    /// Program version and model file.
    pub program: String,
    pub model: String,
    pub shells: usize,
    pub frames: usize,
    pub cases: Vec<Item>,
    pub combos: Vec<Item>,
}

impl Dialog {
    /// The dialog for what instance `pid` of `source` reports. Nothing is ticked at first.
    pub fn from_source(source: &dyn Source, pid: u32) -> Self {
        let title = source.title();
        match source.read(pid) {
            Err(e) => Self { phase: Phase::Error(e), title, ..Default::default() },
            Ok(s) => Self {
                phase: Phase::Ready,
                title,
                pid,
                agent: source.agent(),
                program: s.program,
                model: s.model,
                shells: s.shells,
                frames: s.frames,
                cases: s
                    .cases
                    .into_iter()
                    .map(|(name, kind, status)| Item {
                        name,
                        kind,
                        checked: false,
                        blocked: (status != CaseStatus::Finished).then(|| status.label().to_string()),
                        factors: Vec::new(),
                    })
                    .collect(),
                combos: s.combos.into_iter().map(|(name, kind, factors)| Item { name, kind, checked: false, blocked: None, factors }).collect(),
            },
        }
    }

    /// Tick every item that can be ticked, or clear all.
    pub fn set_all(items: &mut [Item], on: bool) {
        for i in items {
            i.checked = on && i.blocked.is_none();
        }
    }

    pub fn ticked_cases(&self) -> usize {
        self.cases.iter().filter(|i| i.checked).count()
    }

    pub fn ticked_combos(&self) -> usize {
        self.combos.iter().filter(|i| i.checked).count()
    }

    /// Send is possible: something selected in CSiBridge and at least one case or combination ticked.
    pub fn can_send(&self) -> bool {
        self.phase == Phase::Ready && self.shells + self.frames > 0 && self.ticked_cases() + self.ticked_combos() > 0
    }

    /// Store the ticked cases and combinations in Factory's model and the selection in the status,
    /// then start the transfer of geometry and results (protocol v2) when an agent reads CSiBridge.
    pub fn send(&mut self) -> bool {
        if !self.can_send() {
            return false;
        }
        let cases: Vec<LoadCase> = self
            .cases
            .iter()
            .filter(|i| i.checked)
            .map(|i| LoadCase { name: i.name.clone(), kind: i.kind.clone(), description: String::new() })
            .collect();
        let combos: Vec<LoadCombination> = self
            .combos
            .iter()
            .filter(|i| i.checked)
            .map(|i| LoadCombination { name: i.name.clone(), kind: i.kind.clone(), factors: i.factors.clone() })
            .collect();
        model::write(|f| {
            f.model.load_cases = cases;
            f.model.load_combinations = combos;
            f.status.program = "CSiBridge";
            f.status.selected_shells = self.shells;
            f.status.selected_frames = self.frames;
        });
        if let Some(agent) = self.agent.clone() {
            let ticked = |items: &[Item]| items.iter().filter(|i| i.checked).map(|i| i.name.clone()).collect::<Vec<_>>();
            crate::transfer::start(agent, self.pid, ticked(&self.cases), ticked(&self.combos));
            self.phase = Phase::Sending;
        }
        true
    }
}

static DIALOG: LazyLock<RwLock<Dialog>> = LazyLock::new(|| RwLock::new(Dialog::default()));

/// Open the dialog (CONNECT › CSiBridge): find the running CSiBridge on a worker thread; one
/// instance is read at once, several are offered to choose from.
pub fn open() {
    model::write(|f| f.status.program = "CSiBridge");
    store(Dialog { phase: Phase::Loading("Looking for CSiBridge…".into()), title: source().title(), ..Default::default() });
    std::thread::spawn(|| {
        let source = source();
        let next = match source.instances() {
            Err(e) => Dialog { phase: Phase::Error(e), title: source.title(), ..Default::default() },
            Ok(list) if list.is_empty() => Dialog {
                phase: Phase::Error("CSiBridge is not running. Start it, open the analysed model and select shells/frames.".into()),
                title: source.title(),
                ..Default::default()
            },
            Ok(list) if list.len() == 1 => Dialog::from_source(source.as_ref(), list[0].pid),
            Ok(list) => Dialog { phase: Phase::Choose(list), title: source.title(), ..Default::default() },
        };
        finish_loading(next);
    });
}

/// Read the chosen instance (after [`Phase::Choose`]).
pub fn choose(pid: u32) {
    store(Dialog { phase: Phase::Loading("Reading CSiBridge…".into()), title: source().title(), ..Default::default() });
    std::thread::spawn(move || finish_loading(Dialog::from_source(source().as_ref(), pid)));
}

/// Store a worker's result unless the dialog was closed meanwhile.
fn finish_loading(next: Dialog) {
    let mut d = DIALOG.write().unwrap_or_else(PoisonError::into_inner);
    if matches!(d.phase, Phase::Loading(_)) {
        *d = next;
    }
}

/// A copy of the dialog (the UI edits it and stores it back with [`store`]).
pub fn dialog() -> Dialog {
    DIALOG.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Whether the dialog is open (cheap check for every frame).
pub fn is_open() -> bool {
    DIALOG.read().unwrap_or_else(PoisonError::into_inner).phase != Phase::Closed
}

pub fn store(dialog: Dialog) {
    *DIALOG.write().unwrap_or_else(PoisonError::into_inner) = dialog;
}

pub fn close() {
    store(Dialog::default());
}
