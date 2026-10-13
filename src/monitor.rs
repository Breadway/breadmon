use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::ffi::OsStr;
use tokio::process::Command;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMonitor {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub refresh_rate: f64,
    pub transform: u8,
    pub available_modes: Vec<String>,
    pub physical_width: u32,
    pub physical_height: u32,
    pub dpms_status: bool,
    pub vrr: bool,
    pub mirror_of: String,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub refresh: f64,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Self> {
        // Format: "1920x1200@60.00Hz"
        let s = s.trim_end_matches("Hz");
        let (res, refresh_str) = s.split_once('@')?;
        let (w_str, h_str) = res.split_once('x')?;
        Some(Mode {
            width: w_str.parse().ok()?,
            height: h_str.parse().ok()?,
            refresh: refresh_str.parse().ok()?,
        })
    }

    pub fn compact(&self) -> String {
        format!("{}x{}@{:.2}", self.width, self.height, self.refresh)
    }

    pub fn pixels(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}@{:.2}Hz", self.width, self.height, self.refresh)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    Normal = 0,
    R90 = 1,
    R180 = 2,
    R270 = 3,
    Flipped = 4,
    FlippedR90 = 5,
    FlippedR180 = 6,
    FlippedR270 = 7,
}

impl Transform {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::R90,
            2 => Self::R180,
            3 => Self::R270,
            4 => Self::Flipped,
            5 => Self::FlippedR90,
            6 => Self::FlippedR180,
            7 => Self::FlippedR270,
            _ => Self::Normal,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::R90 => "90°",
            Self::R180 => "180°",
            Self::R270 => "270°",
            Self::Flipped => "Flipped",
            Self::FlippedR90 => "Flipped 90°",
            Self::FlippedR180 => "Flipped 180°",
            Self::FlippedR270 => "Flipped 270°",
        }
    }

    pub fn all() -> &'static [Transform] {
        &[
            Self::Normal,
            Self::R90,
            Self::R180,
            Self::R270,
            Self::Flipped,
            Self::FlippedR90,
            Self::FlippedR180,
            Self::FlippedR270,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct Monitor {
    pub name: String,
    pub description: String,
    pub active_mode: Mode,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: Transform,
    pub vrr: bool,
    pub dpms: bool,
    pub disabled: bool,
    pub mirror_of: Option<String>,
    pub available_modes: Vec<Mode>,
    /// Physical dimensions in millimetres (0 if unknown)
    pub physical_width_mm: u32,
    pub physical_height_mm: u32,
}

impl Monitor {
    fn from_raw(raw: RawMonitor) -> Self {
        let mut modes: Vec<Mode> = raw
            .available_modes
            .iter()
            .filter_map(|s| Mode::parse(s))
            .collect();
        // Sort descending by pixels then refresh for consistent ordering
        modes.sort_by(|a, b| {
            b.pixels().cmp(&a.pixels()).then(
                b.refresh
                    .partial_cmp(&a.refresh)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        modes.dedup_by(|a, b| {
            a.width == b.width && a.height == b.height && (a.refresh - b.refresh).abs() < 0.01
        });

        let active_mode = Mode {
            width: raw.width,
            height: raw.height,
            refresh: raw.refresh_rate,
        };

        let mirror_of = if raw.mirror_of.is_empty() || raw.mirror_of == "none" {
            None
        } else {
            Some(raw.mirror_of)
        };

        Monitor {
            name: raw.name,
            description: raw.description,
            active_mode,
            x: raw.x,
            y: raw.y,
            scale: raw.scale,
            transform: Transform::from_u8(raw.transform),
            vrr: raw.vrr,
            dpms: raw.dpms_status,
            disabled: raw.disabled,
            mirror_of,
            available_modes: modes,
            physical_width_mm: raw.physical_width,
            physical_height_mm: raw.physical_height,
        }
    }

    /// Width in world coordinates (swapped for 90/270 transforms)
    pub fn world_width(&self) -> u32 {
        match self.transform {
            Transform::R90 | Transform::R270 | Transform::FlippedR90 | Transform::FlippedR270 => {
                self.active_mode.height
            }
            _ => self.active_mode.width,
        }
    }

    /// Height in world coordinates
    pub fn world_height(&self) -> u32 {
        match self.transform {
            Transform::R90 | Transform::R270 | Transform::FlippedR90 | Transform::FlippedR270 => {
                self.active_mode.width
            }
            _ => self.active_mode.height,
        }
    }

    pub fn right_edge(&self) -> i32 {
        self.x + self.world_width() as i32
    }

    pub fn bottom_edge(&self) -> i32 {
        self.y + self.world_height() as i32
    }

    /// Pixels-per-inch, or None if physical dimensions are unknown.
    pub fn ppi(&self) -> Option<f64> {
        if self.physical_width_mm == 0 || self.physical_height_mm == 0 {
            return None;
        }
        let diag_px =
            ((self.active_mode.width.pow(2) + self.active_mode.height.pow(2)) as f64).sqrt();
        let diag_mm =
            ((self.physical_width_mm.pow(2) + self.physical_height_mm.pow(2)) as f64).sqrt();
        Some(diag_px / (diag_mm / 25.4))
    }

    /// Suggested fractional scale based on PPI.
    pub fn suggested_scale(&self) -> Option<f64> {
        let ppi = self.ppi()?;
        Some(if ppi < 110.0 {
            1.0
        } else if ppi < 150.0 {
            1.25
        } else if ppi < 200.0 {
            1.5
        } else {
            2.0
        })
    }

    /// Unique resolutions available (deduped WxH pairs)
    pub fn unique_resolutions(&self) -> Vec<(u32, u32)> {
        let mut seen = Vec::new();
        for m in &self.available_modes {
            let pair = (m.width, m.height);
            if !seen.contains(&pair) {
                seen.push(pair);
            }
        }
        seen
    }

    /// Refresh rates available for a given WxH
    pub fn refreshes_for(&self, width: u32, height: u32) -> Vec<f64> {
        self.available_modes
            .iter()
            .filter(|m| m.width == width && m.height == height)
            .map(|m| m.refresh)
            .collect()
    }
}

pub async fn load_monitors() -> Result<Vec<Monitor>> {
    let output = Command::new("hyprctl")
        .args(["monitors", "all", "-j"])
        .output()
        .await
        .context("failed to run hyprctl monitors all -j")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let raw: Vec<RawMonitor> =
        serde_json::from_str(&stdout).context("failed to parse hyprctl monitors JSON")?;

    // Hyprland reports mirrorOf as a numeric ID string when using `monitors all`.
    // Resolve to monitor name so format_hypr_line emits the correct `mirror,<name>`.
    let id_to_name: std::collections::HashMap<String, String> = raw
        .iter()
        .map(|r| (r.id.to_string(), r.name.clone()))
        .collect();

    Ok(raw
        .into_iter()
        .map(|mut r| {
            if r.mirror_of != "none" && !r.mirror_of.is_empty() {
                if let Some(name) = id_to_name.get(&r.mirror_of) {
                    r.mirror_of = name.clone();
                }
            }
            Monitor::from_raw(r)
        })
        .collect())
}

#[cfg(test)]
pub fn format_hypr_line(m: &Monitor) -> String {
    if let Some(src) = &m.mirror_of {
        format!(
            "monitor={},{},auto,{:.2},mirror,{}",
            m.name,
            m.active_mode.compact(),
            m.scale,
            src
        )
    } else {
        format!(
            "monitor={},{},{}x{},{:.2},transform,{},vrr,{}",
            m.name,
            m.active_mode.compact(),
            m.x,
            m.y,
            m.scale,
            m.transform.as_u8(),
            if m.vrr { 1 } else { 0 }
        )
    }
}

pub async fn apply_monitors(monitors: &[Monitor]) -> Result<()> {
    apply_monitors_with(OsStr::new("hyprctl"), monitors).await
}

async fn apply_monitors_with(program: &OsStr, monitors: &[Monitor]) -> Result<()> {
    // breadmon stores mirror_of on the slave; hl.monitor() wants mirror= on the source.
    let mut mirror_slaves: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for m in monitors {
        if let Some(src) = &m.mirror_of {
            mirror_slaves.insert(src.as_str(), m.name.as_str());
        }
    }

    let mut eval_stmts: Vec<String> = Vec::new();
    // Enabled monitors (mirror slaves included) with the DPMS state they should have.
    let mut wanted: Vec<(&str, bool)> = Vec::new();

    for m in monitors {
        if m.disabled {
            continue;
        }
        wanted.push((m.name.as_str(), m.dpms));
        if m.mirror_of.is_some() {
            // Mirror slaves get no independent hl.monitor() call; only DPMS.
            continue;
        }

        let mode = format!(
            "{}x{}@{}",
            m.active_mode.width,
            m.active_mode.height,
            (m.active_mode.refresh + 0.5) as u32
        );
        let position = format!("{}x{}", m.x, m.y);
        let scale = format!("{:.2}", m.scale);

        let mut stmt = format!(
            "hl.monitor({{ output = \"{}\", mode = \"{}\", position = \"{}\", scale = \"{}\", transform = {}, vrr = {}",
            m.name, mode, position, scale, m.transform.as_u8(), m.vrr
        );

        if let Some(&slave) = mirror_slaves.get(m.name.as_str()) {
            stmt.push_str(&format!(", mirror = \"{}\"", slave));
        }

        stmt.push_str(" })");
        eval_stmts.push(stmt);
    }

    // Hyprland answers `ok` for a DPMS request naming a monitor that does not
    // exist and then applies it to every output, so look every name up first.
    // Only monitors whose live DPMS state differs get a dispatch.
    let mut dpms_changes: Vec<(&str, &str)> = Vec::new();
    if !wanted.is_empty() {
        let live = live_monitor_dpms(program).await?;
        for (name, want_on) in wanted {
            let Some(&(_, is_on)) = live.iter().find(|(n, _)| n == name) else {
                bail!("breadmon: cannot set DPMS for {name}: no monitor with that name exists");
            };
            if is_on != want_on {
                dpms_changes.push((name, if want_on { "on" } else { "off" }));
            }
        }
    }

    if !eval_stmts.is_empty() {
        let lua = eval_stmts.join("; ");
        hyprctl_eval(program, "hyprctl eval", &lua).await?;
    }

    for (name, action) in dpms_changes {
        let label = format!("hyprctl eval (dpms {action} {name})");
        hyprctl_eval(program, &label, &dpms_expr(action, name)).await?;
    }

    Ok(())
}

/// `(name, dpmsStatus)` for every monitor Hyprland knows, from
/// `<program> monitors all -j`. Fails unless the command exits 0 and its
/// output parses.
async fn live_monitor_dpms(program: &OsStr) -> Result<Vec<(String, bool)>> {
    #[derive(Deserialize)]
    struct Named {
        name: String,
        #[serde(rename = "dpmsStatus")]
        dpms_status: bool,
    }

    let output = Command::new(program)
        .args(["monitors", "all", "-j"])
        .output()
        .await
        .context("hyprctl monitors all -j could not be run")?;
    if !output.status.success() {
        bail!(
            "hyprctl monitors all -j failed: {}, stderr: {:?}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let named: Vec<Named> =
        serde_json::from_slice(&output.stdout).context("failed to parse hyprctl monitors JSON")?;
    Ok(named.into_iter().map(|m| (m.name, m.dpms_status)).collect())
}

/// Runs `<program> eval <lua>`. Succeeds only when the process exits 0 and
/// its trimmed stdout is exactly `ok`; otherwise the error carries the exit
/// status, stdout and stderr.
async fn hyprctl_eval(program: &OsStr, label: &str, lua: &str) -> Result<()> {
    let output = Command::new(program)
        .args(["eval", lua])
        .output()
        .await
        .with_context(|| format!("{label} could not be run"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() && stdout == "ok" {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if is_missing_eval_extension(&format!("{stdout}\n{stderr}")) {
        return Err(anyhow::anyhow!(
            "breadmon: `hyprctl eval` (and the `hl.monitor()` Lua call it uses \
             to apply monitor changes) is not available on this compositor. \
             Live apply needs Hyprland configured with hyprland.lua; it is not \
             available when Hyprland is configured with hyprland.conf. See the \
             breadmon README for details. ({label} {}, stdout: {stdout:?}, stderr: {stderr:?})",
            output.status
        ));
    }
    Err(anyhow::anyhow!(
        "{label} failed: {}, stdout: {stdout:?}, stderr: {stderr:?}",
        output.status
    ))
}

/// The Lua expression that sets DPMS `action` (`on` or `off`) for `monitor`.
/// Hyprland configured with `hyprland.lua` parses `hyprctl dispatch` arguments
/// as Lua, so the legacy `dispatch dpms off <name>` form fails there.
/// `hl.dsp.dpms` takes `action` and `monitor` fields.
fn dpms_expr(action: &str, monitor: &str) -> String {
    let mut quoted = String::with_capacity(monitor.len() + 2);
    quoted.push('\'');
    for c in monitor.chars() {
        match c {
            '\\' => quoted.push_str("\\\\"),
            '\'' => quoted.push_str("\\'"),
            c => quoted.push(c),
        }
    }
    quoted.push('\'');
    format!("hl.dispatch(hl.dsp.dpms({{ action = '{action}', monitor = {quoted} }}))")
}

/// Hyprland configured with `hyprland.conf` has no `eval` request (it comes
/// with the `hl.monitor()` Lua global used above, which needs `hyprland.lua`),
/// and responds with an "unknown request" style message rather than an error
/// about `hl`. Detect that case so we can point the user at the actual cause
/// instead of surfacing the raw hyprctl text.
fn is_missing_eval_extension(hyprctl_stdout: &str) -> bool {
    let s = hyprctl_stdout.to_lowercase();
    s.contains("unknown request") || s.contains("unknown command")
}

#[cfg(test)]
mod hyprctl_eval_tests {
    use super::hyprctl_eval;
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    // Fake hyprctl scripts are written and exec'd by parallel tests; serialise
    // them so no test forks while another script is still open for writing
    // (which would fail with ETXTBSY).
    static FAKE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn fake_hyprctl(name: &str, body: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("breadmon-hyprctl-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hyprctl");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, path)
    }

    async fn run(name: &str, body: &str) -> anyhow::Result<()> {
        let _guard = FAKE_LOCK.lock().await;
        let (dir, path) = fake_hyprctl(name, body);
        let result = hyprctl_eval(path.as_os_str(), "hyprctl eval", "hl.dispatch(x)").await;
        std::fs::remove_dir_all(&dir).unwrap();
        result
    }

    #[tokio::test]
    async fn ok_with_exit_zero_succeeds() {
        assert!(run("ok0", "printf 'ok\\n'; exit 0").await.is_ok());
    }

    #[tokio::test]
    async fn surrounding_whitespace_around_ok_is_trimmed() {
        assert!(run("ws", "printf '  ok \\n'; exit 0").await.is_ok());
    }

    #[tokio::test]
    async fn refusal_with_exit_zero_is_an_error_with_stdout_and_stderr() {
        let err = run("refuse0", "printf 'refused'; printf 'detail' >&2; exit 0")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("exit status: 0"), "{err}");
        assert!(err.contains("refused"), "{err}");
        assert!(err.contains("detail"), "{err}");
    }

    #[tokio::test]
    async fn ok_with_nonzero_exit_is_an_error_with_exit_status() {
        let err = run("ok7", "printf 'ok\\n'; printf 'boom' >&2; exit 7")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("exit status: 7"), "{err}");
        assert!(err.contains("boom"), "{err}");
    }

    #[tokio::test]
    async fn empty_output_with_exit_zero_is_an_error() {
        assert!(run("empty", "exit 0").await.is_err());
    }

    #[tokio::test]
    async fn spawn_failure_is_an_error() {
        let _guard = FAKE_LOCK.lock().await;
        let missing = OsString::from("/nonexistent/breadmon-no-hyprctl");
        assert!(hyprctl_eval(&missing, "hyprctl eval", "x").await.is_err());
    }
}

#[cfg(test)]
mod dpms_apply_tests {
    use super::{apply_monitors_with, Mode, Monitor, Transform};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    // Same serialisation as hyprctl_eval_tests: no script is exec'd while
    // another one is still open for writing (ETXTBSY).
    static FAKE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn monitor(name: &str, dpms: bool) -> Monitor {
        let mode = Mode {
            width: 1920,
            height: 1080,
            refresh: 60.0,
        };
        Monitor {
            name: name.into(),
            description: String::new(),
            active_mode: mode.clone(),
            x: 0,
            y: 0,
            scale: 1.0,
            transform: Transform::Normal,
            vrr: false,
            dpms,
            disabled: false,
            mirror_of: None,
            available_modes: vec![mode],
            physical_width_mm: 0,
            physical_height_mm: 0,
        }
    }

    /// Fake `hyprctl`: `monitors all -j` lists `eDP-1` and `HDMI-A-1`, both
    /// with DPMS on, and `eval` records each Lua argument on its own line in
    /// `log` and replies `ok`. Returns (dir, script path, log path).
    fn fake_hyprctl(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("breadmon-dpms-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("eval.log");
        let path = dir.join("hyprctl");
        let body = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = monitors ]; then\n\
               printf '%s' '[{{\"name\":\"eDP-1\",\"dpmsStatus\":true}},{{\"name\":\"HDMI-A-1\",\"dpmsStatus\":true}}]'\n\
               exit 0\n\
             fi\n\
             printf '%s\\n' \"$2\" >> '{}'\n\
             printf 'ok\\n'\n",
            log.display()
        );
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, path, log)
    }

    async fn run(name: &str, monitors: &[Monitor]) -> (anyhow::Result<()>, String) {
        let _guard = FAKE_LOCK.lock().await;
        let (dir, path, log) = fake_hyprctl(name);
        let result = apply_monitors_with(path.as_os_str(), monitors).await;
        let recorded = std::fs::read_to_string(&log).unwrap_or_default();
        std::fs::remove_dir_all(&dir).unwrap();
        (result, recorded)
    }

    #[tokio::test]
    async fn unknown_monitor_name_is_an_error_and_no_dpms_is_dispatched() {
        let monitors = [monitor("eDP-1", true), monitor("NOPE-9", false)];
        let (result, recorded) = run("unknown", &monitors).await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("NOPE-9"), "{err}");
        assert!(!recorded.contains("dpms"), "{recorded}");
        assert!(!recorded.contains("hl.monitor"), "{recorded}");
    }

    #[tokio::test]
    async fn known_monitor_off_dispatches_dpms_off_for_that_name_only() {
        let monitors = [monitor("eDP-1", true), monitor("HDMI-A-1", false)];
        let (result, recorded) = run("off", &monitors).await;
        assert!(result.is_ok(), "{result:?}");
        let dpms_lines: Vec<&str> = recorded.lines().filter(|l| l.contains("dpms")).collect();
        assert_eq!(
            dpms_lines,
            ["hl.dispatch(hl.dsp.dpms({ action = 'off', monitor = 'HDMI-A-1' }))"]
        );
    }

    #[tokio::test]
    async fn already_on_monitor_gets_no_dpms_dispatch() {
        let monitors = [monitor("eDP-1", true), monitor("HDMI-A-1", true)];
        let (result, recorded) = run("noop", &monitors).await;
        assert!(result.is_ok(), "{result:?}");
        assert!(!recorded.contains("dpms"), "{recorded}");
    }

    #[tokio::test]
    async fn known_monitor_restored_from_off_dispatches_dpms_on_for_that_name_only() {
        let _guard = FAKE_LOCK.lock().await;
        let dir =
            std::env::temp_dir().join(format!("breadmon-dpms-{}-restore", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("eval.log");
        let path = dir.join("hyprctl");
        let body = format!(
            "#!/bin/sh\n\
             if [ \"$1\" = monitors ]; then\n\
               printf '%s' '[{{\"name\":\"eDP-1\",\"dpmsStatus\":true}},{{\"name\":\"HDMI-A-1\",\"dpmsStatus\":false}}]'\n\
               exit 0\n\
             fi\n\
             printf '%s\\n' \"$2\" >> '{}'\n\
             printf 'ok\\n'\n",
            log.display()
        );
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let monitors = [monitor("eDP-1", true), monitor("HDMI-A-1", true)];
        let result = apply_monitors_with(path.as_os_str(), &monitors).await;
        let recorded = std::fs::read_to_string(&log).unwrap_or_default();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(result.is_ok(), "{result:?}");
        let dpms_lines: Vec<&str> = recorded.lines().filter(|l| l.contains("dpms")).collect();
        assert_eq!(
            dpms_lines,
            ["hl.dispatch(hl.dsp.dpms({ action = 'on', monitor = 'HDMI-A-1' }))"]
        );
    }

    #[tokio::test]
    async fn monitors_query_failure_stops_before_any_dpms() {
        let _guard = FAKE_LOCK.lock().await;
        let dir =
            std::env::temp_dir().join(format!("breadmon-dpms-{}-badquery", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("eval.log");
        let path = dir.join("hyprctl");
        let body = format!(
            "#!/bin/sh\nif [ \"$1\" = monitors ]; then echo '[]'; exit 3; fi\necho \"$2\" >> '{}'\necho ok\n",
            log.display()
        );
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let monitors = [monitor("eDP-1", false)];
        let result = apply_monitors_with(path.as_os_str(), &monitors).await;
        let recorded = std::fs::read_to_string(&log).unwrap_or_default();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(result.is_err());
        assert!(recorded.is_empty(), "{recorded}");
    }

    #[tokio::test]
    async fn monitors_query_nonzero_exit_with_valid_json_is_an_error() {
        let _guard = FAKE_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("breadmon-dpms-{}-exit3", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("eval.log");
        let path = dir.join("hyprctl");
        let body = format!(
            "#!/bin/sh\nif [ \"$1\" = monitors ]; then echo '[{{\"name\":\"eDP-1\",\"dpmsStatus\":true}}]'; exit 3; fi\necho \"$2\" >> '{}'\necho ok\n",
            log.display()
        );
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let monitors = [monitor("eDP-1", true)];
        let result = apply_monitors_with(path.as_os_str(), &monitors).await;
        let recorded = std::fs::read_to_string(&log).unwrap_or_default();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(result.is_err());
        assert!(recorded.is_empty(), "{recorded}");
    }
}

#[cfg(test)]
mod eval_extension_tests {
    use super::is_missing_eval_extension;

    #[test]
    fn detects_unknown_request() {
        assert!(is_missing_eval_extension("unknown request"));
        assert!(is_missing_eval_extension("Unknown Request"));
    }

    #[test]
    fn dpms_off_targets_the_named_monitor() {
        assert_eq!(
            super::dpms_expr("off", "DP-1"),
            "hl.dispatch(hl.dsp.dpms({ action = 'off', monitor = 'DP-1' }))"
        );
    }

    #[test]
    fn dpms_on_targets_the_named_monitor() {
        assert_eq!(
            super::dpms_expr("on", "DP-1"),
            "hl.dispatch(hl.dsp.dpms({ action = 'on', monitor = 'DP-1' }))"
        );
    }

    #[test]
    fn dpms_expr_escapes_quotes_in_the_name() {
        assert_eq!(
            super::dpms_expr("off", "a'b\\c"),
            "hl.dispatch(hl.dsp.dpms({ action = 'off', monitor = 'a\\'b\\\\c' }))"
        );
    }

    #[test]
    fn does_not_flag_lua_errors() {
        // A real Lua/apply-time error from a BOS build should still surface
        // as a normal hyprctl error, not the "requires BOS" message.
        assert!(!is_missing_eval_extension(
            "error: attempt to call a nil value"
        ));
        assert!(!is_missing_eval_extension("ok"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parse_normal() {
        let m = Mode::parse("1920x1200@60.00Hz").unwrap();
        assert_eq!(m.width, 1920);
        assert_eq!(m.height, 1200);
        assert!((m.refresh - 60.0).abs() < 0.01);
    }

    #[test]
    fn mode_parse_fractional() {
        let m = Mode::parse("3840x2160@59.94Hz").unwrap();
        assert_eq!(m.width, 3840);
        assert_eq!(m.height, 2160);
        assert!((m.refresh - 59.94).abs() < 0.01);
    }

    #[test]
    fn mode_compact_roundtrip() {
        let m = Mode {
            width: 1920,
            height: 1080,
            refresh: 60.0,
        };
        let s = m.compact();
        let m2 = Mode::parse(&format!("{}Hz", s)).unwrap();
        assert_eq!(m.width, m2.width);
        assert_eq!(m.height, m2.height);
    }

    #[test]
    fn format_hypr_line_normal() {
        let m = Monitor {
            name: "eDP-1".into(),
            description: String::new(),
            active_mode: Mode {
                width: 1920,
                height: 1200,
                refresh: 60.0,
            },
            x: 0,
            y: 0,
            scale: 1.0,
            transform: Transform::Normal,
            vrr: false,
            dpms: true,
            disabled: false,
            mirror_of: None,
            available_modes: vec![],
            physical_width_mm: 0,
            physical_height_mm: 0,
        };
        assert_eq!(
            format_hypr_line(&m),
            "monitor=eDP-1,1920x1200@60.00,0x0,1.00,transform,0,vrr,0"
        );
    }

    #[test]
    fn format_hypr_line_mirror() {
        let m = Monitor {
            name: "HDMI-A-1".into(),
            description: String::new(),
            active_mode: Mode {
                width: 1920,
                height: 1080,
                refresh: 60.0,
            },
            x: 1920,
            y: 0,
            scale: 1.0,
            transform: Transform::Normal,
            vrr: false,
            dpms: true,
            disabled: false,
            mirror_of: Some("eDP-1".into()),
            available_modes: vec![],
            physical_width_mm: 0,
            physical_height_mm: 0,
        };
        assert_eq!(
            format_hypr_line(&m),
            "monitor=HDMI-A-1,1920x1080@60.00,auto,1.00,mirror,eDP-1"
        );
    }
}
