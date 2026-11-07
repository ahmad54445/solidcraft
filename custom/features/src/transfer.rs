//! Geometry and results from CSiBridge (protocol v2, docs/09-agent-protocol.md), received on a
//! worker thread. Rules (agents/csi-agent/README.md): the UI never waits; progress per step;
//! Cancel ends the agent; a watchdog reports a silent CSiBridge; results go straight to files.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::model::{self, Backend};

/// After this long without a frame the agent (CSiBridge) is reported as not responding.
pub const WATCHDOG: Duration = Duration::from_secs(30);

/// Geometry of the transferred elements (lengths in m).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub joints: Vec<[f64; 3]>,
    /// i, j, section index
    pub frames: Vec<[u32; 3]>,
    /// n1..n4 (n4 = u32::MAX for triangles), section index
    pub shells: Vec<[u32; 5]>,
    pub frame_names: Vec<String>,
    pub shell_names: Vec<String>,
    pub frame_sections: Vec<String>,
    pub shell_sections: Vec<String>,
    /// Thickness per shell section (m).
    pub shell_thickness: Vec<f64>,
    pub materials: Vec<String>,
}

/// One stored result block (a case or combination, frames or shells).
#[derive(Clone, Debug, PartialEq)]
pub struct ResultBlock {
    pub name: String,
    pub combo: bool,
    /// "frame_results" or "shell_results"
    pub family: String,
    pub rows: usize,
    /// Column names and types, as the agent sent them.
    pub columns: Vec<String>,
    /// Raw little-endian columns, one after another.
    pub file: PathBuf,
}

/// The transfer as the UI sees it.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub label: String,
    pub error: Option<String>,
    pub finished: bool,
    pub started: Option<Instant>,
    pub last_frame: Option<Instant>,
}

impl State {
    /// Running, and no frame for longer than [`WATCHDOG`].
    pub fn silent(&self) -> bool {
        self.running && self.last_frame.is_some_and(|t| t.elapsed() > WATCHDOG)
    }
}

static STATE: LazyLock<RwLock<State>> = LazyLock::new(|| RwLock::new(State::default()));
static CHILD: LazyLock<Mutex<Option<Child>>> = LazyLock::new(|| Mutex::new(None));
static GEOMETRY: LazyLock<RwLock<Arc<Geometry>>> = LazyLock::new(|| RwLock::new(Arc::new(Geometry::default())));
static RESULTS: LazyLock<RwLock<Vec<ResultBlock>>> = LazyLock::new(|| RwLock::new(Vec::new()));

pub fn state() -> State {
    STATE.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn update(f: impl FnOnce(&mut State)) {
    f(&mut STATE.write().unwrap_or_else(PoisonError::into_inner));
}

/// The last transferred geometry (shared, never copied).
pub fn geometry() -> Arc<Geometry> {
    GEOMETRY.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The stored result blocks.
pub fn results() -> Vec<ResultBlock> {
    RESULTS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Start the transfer from CSiBridge process `pid` with the ticked cases and combinations.
pub fn start(agent: PathBuf, pid: u32, cases: Vec<String>, combos: Vec<String>) {
    cancel();
    let now = Instant::now();
    update(|s| *s = State { running: true, total: 1, label: "Starting csi-agent".into(), started: Some(now), last_frame: Some(now), ..Default::default() });
    model::write(|f| f.status.backend = Backend::Busy("CSiBridge transfer".into()));
    std::thread::spawn(move || {
        let outcome = run(agent, pid, cases, combos);
        let cancelled = !state().running;
        if let Err(e) = &outcome
            && !cancelled
        {
            update(|s| {
                s.error = Some(e.clone());
                s.running = false;
            });
            model::write(|f| f.status.backend = Backend::Error("CSiBridge transfer failed".into()));
        }
        *CHILD.lock().unwrap_or_else(PoisonError::into_inner) = None;
    });
}

/// End the transfer now: the agent process is ended, partial data is dropped. CSiBridge is not touched.
pub fn cancel() {
    if let Some(mut child) = CHILD.lock().unwrap_or_else(PoisonError::into_inner).take() {
        let _ = child.kill();
    }
    let was_running = state().running;
    update(|s| s.running = false);
    if was_running {
        model::write(|f| f.status.backend = Backend::Ready);
    }
}

/// Forget a finished or failed transfer (the dialog closed).
pub fn reset() {
    if !state().running {
        update(|s| *s = State::default());
    }
}

fn run(agent: PathBuf, pid: u32, cases: Vec<String>, combos: Vec<String>) -> Result<(), String> {
    let mut command = Command::new(&agent);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|e| format!("Could not start csi-agent: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("csi-agent has no input")?;
    let stdout = child.stdout.take().ok_or("csi-agent has no output")?;
    *CHILD.lock().unwrap_or_else(PoisonError::into_inner) = Some(child);
    let request = json!({"v": 2, "id": 1, "cmd": "transfer", "pid": pid, "cases": cases, "combos": combos});
    writeln!(stdin, "{request}").map_err(|e| format!("csi-agent: {e}"))?;
    drop(stdin);

    let dir = results_dir()?;
    let mut reader = BufReader::with_capacity(1 << 20, stdout);
    let mut geometry = Geometry::default();
    let mut blocks = Vec::new();
    loop {
        let (header, payload) = read_frame(&mut reader)?;
        update(|s| s.last_frame = Some(Instant::now()));
        if !state().running {
            return Err("cancelled".into());
        }
        let count = header["count"].as_u64().unwrap_or(0) as usize;
        match header["type"].as_str().unwrap_or("") {
            "progress" => {
                let (done, total) = (header["done"].as_u64().unwrap_or(0) as usize, header["total"].as_u64().unwrap_or(1) as usize);
                let label = header["label"].as_str().unwrap_or("").to_string();
                model::write(|f| f.status.backend = Backend::Busy(format!("{label} ({done}/{total})")));
                update(|s| {
                    s.done = done;
                    s.total = total;
                    s.label = label;
                });
            }
            "names" => {
                let names = |k: &str| header[k].as_array().map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
                geometry.frame_names = names("frames");
                geometry.shell_names = names("shells");
                geometry.frame_sections = names("frame_sections");
                geometry.shell_sections = names("shell_sections");
                geometry.materials = names("materials");
            }
            "joints" => {
                let c = columns_f64(&payload, count, 3)?;
                geometry.joints = (0..count).map(|k| [c[0][k], c[1][k], c[2][k]]).collect();
            }
            "frames" => {
                let c = columns_u32(&payload, count, 3)?;
                geometry.frames = (0..count).map(|k| [c[0][k], c[1][k], c[2][k]]).collect();
            }
            "shells" => {
                let c = columns_u32(&payload, count, 5)?;
                geometry.shells = (0..count).map(|k| [c[0][k], c[1][k], c[2][k], c[3][k], c[4][k]]).collect();
            }
            "shell_thickness" => geometry.shell_thickness = columns_f64(&payload, count, 1)?.remove(0),
            family @ ("frame_results" | "shell_results") => {
                // Straight to disk: the payload is already the stored column layout.
                let file = dir.join(format!("{:05}_{family}.bin", blocks.len()));
                let mut w = BufWriter::new(File::create(&file).map_err(|e| format!("result store: {e}"))?);
                w.write_all(&payload).map_err(|e| format!("result store: {e}"))?;
                w.flush().map_err(|e| format!("result store: {e}"))?;
                blocks.push(ResultBlock {
                    name: header["name"].as_str().unwrap_or("").to_string(),
                    combo: header["combo"].as_bool().unwrap_or(false),
                    family: family.to_string(),
                    rows: count,
                    columns: header["columns"].as_array().map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default(),
                    file,
                });
            }
            "error" => return Err(header["message"].as_str().unwrap_or("csi-agent failed").to_string()),
            "done" => break,
            _ => {}
        }
    }
    publish(geometry, blocks);
    Ok(())
}

/// Make the finished transfer visible: geometry, result index, browser counts, status.
fn publish(geometry: Geometry, blocks: Vec<ResultBlock>) {
    let first = blocks.first().map(|b| b.name.clone());
    let unique_thickness = {
        let mut t: Vec<u64> = geometry.shell_thickness.iter().map(|v| v.to_bits()).collect();
        t.sort_unstable();
        t.dedup();
        t.len()
    };
    model::write(|f| {
        let m = &mut f.model;
        m.nodes = geometry.joints.len();
        m.elements = geometry.frames.len() + geometry.shells.len();
        m.materials = geometry.materials.len();
        m.sections = geometry.frame_sections.len() + geometry.shell_sections.len();
        m.thicknesses = unique_thickness;
        f.status.selected_frames = geometry.frames.len();
        f.status.selected_shells = geometry.shells.len();
        f.status.current_load = first;
        f.status.connection = model::Connection::Connected;
        f.status.backend = Backend::Ready;
    });
    *GEOMETRY.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(geometry);
    *RESULTS.write().unwrap_or_else(PoisonError::into_inner) = blocks;
    update(|s| {
        s.running = false;
        s.finished = true;
        s.done = s.total;
    });
}

/// One frame: u32 header length, JSON header, u64 payload length, payload.
pub fn read_frame(r: &mut impl Read) -> Result<(Value, Vec<u8>), String> {
    let mut b4 = [0u8; 4];
    r.read_exact(&mut b4).map_err(|_| "csi-agent ended without finishing".to_string())?;
    let mut head = vec![0u8; u32::from_le_bytes(b4) as usize];
    r.read_exact(&mut head).map_err(|e| format!("csi-agent: {e}"))?;
    let header: Value = serde_json::from_slice(&head).map_err(|e| format!("csi-agent header: {e}"))?;
    if header["v"] != json!(2) {
        return Err(format!("csi-agent speaks protocol {} (Factory expects 2)", header["v"]));
    }
    let mut b8 = [0u8; 8];
    r.read_exact(&mut b8).map_err(|e| format!("csi-agent: {e}"))?;
    let mut payload = vec![0u8; u64::from_le_bytes(b8) as usize];
    r.read_exact(&mut payload).map_err(|e| format!("csi-agent: {e}"))?;
    Ok((header, payload))
}

/// `n` columns of `count` little-endian f64.
pub fn columns_f64(payload: &[u8], count: usize, n: usize) -> Result<Vec<Vec<f64>>, String> {
    if payload.len() != count * n * 8 {
        return Err(format!("csi-agent: {} bytes for {n} f64 columns of {count}", payload.len()));
    }
    if count == 0 {
        return Ok(vec![Vec::new(); n]);
    }
    Ok(payload.chunks_exact(count * 8).map(|c| c.chunks_exact(8).map(|b| f64::from_le_bytes(b.try_into().unwrap_or_default())).collect()).collect())
}

/// `n` columns of `count` little-endian u32.
pub fn columns_u32(payload: &[u8], count: usize, n: usize) -> Result<Vec<Vec<u32>>, String> {
    if payload.len() != count * n * 4 {
        return Err(format!("csi-agent: {} bytes for {n} u32 columns of {count}", payload.len()));
    }
    if count == 0 {
        return Ok(vec![Vec::new(); n]);
    }
    Ok(payload.chunks_exact(count * 4).map(|c| c.chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap_or_default())).collect()).collect())
}

/// A new folder for this transfer's results: %LOCALAPPDATA%\Factory\results\<time>.
fn results_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let dir = base.join("Factory").join("results").join(stamp.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("result store {}: {e}", dir.display()))?;
    Ok(dir)
}
