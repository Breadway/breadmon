use anyhow::{Context, Result};
use serde::Deserialize;
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
            b.pixels()
                .cmp(&a.pixels())
                .then(b.refresh.partial_cmp(&a.refresh).unwrap_or(std::cmp::Ordering::Equal))
        });
        modes.dedup_by(|a, b| a.width == b.width && a.height == b.height && (a.refresh - b.refresh).abs() < 0.01);

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
        let diag_px = ((self.active_mode.width.pow(2) + self.active_mode.height.pow(2)) as f64).sqrt();
        let diag_mm = ((self.physical_width_mm.pow(2) + self.physical_height_mm.pow(2)) as f64).sqrt();
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
    let id_to_name: std::collections::HashMap<String, String> =
        raw.iter().map(|r| (r.id.to_string(), r.name.clone())).collect();

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
    // breadmon stores mirror_of on the slave; hl.monitor() wants mirror= on the source.
    let mut mirror_slaves: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for m in monitors {
        if let Some(src) = &m.mirror_of {
            mirror_slaves.insert(src.as_str(), m.name.as_str());
        }
    }

    let mut eval_stmts: Vec<String> = Vec::new();
    let mut dpms_off: Vec<String> = Vec::new();

    for m in monitors {
        if m.disabled {
            continue;
        }
        if m.mirror_of.is_some() {
            // Mirror slaves get no independent hl.monitor() call; only track DPMS.
            if !m.dpms {
                dpms_off.push(m.name.clone());
            }
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

        if !m.dpms {
            dpms_off.push(m.name.clone());
        }
    }

    if !eval_stmts.is_empty() {
        let lua = eval_stmts.join("; ");
        let output = Command::new("hyprctl")
            .args(["eval", &lua])
            .output()
            .await
            .context("hyprctl eval failed")?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if stdout != "ok" {
            if is_missing_eval_extension(&stdout) {
                return Err(anyhow::anyhow!(
                    "breadmon: `hyprctl eval` (and the `hl.monitor()` Lua extension it uses \
                     to apply monitor changes) is not available on this compositor. \
                     This feature requires BOS (Bread OS)'s patched Hyprland build — it \
                     does not exist on vanilla/upstream Hyprland. See the breadmon README \
                     for details. (raw hyprctl response: {stdout:?})"
                ));
            }
            return Err(anyhow::anyhow!("hyprctl: {}", stdout));
        }
    }

    for name in dpms_off {
        Command::new("hyprctl")
            .args(["dispatch", "dpms", "off", &name])
            .output()
            .await
            .context("hyprctl dispatch dpms off failed")?;
    }

    Ok(())
}

/// Vanilla/upstream Hyprland doesn't recognize the `eval` request at all
/// (BOS's Hyprland fork adds it, along with the `hl.monitor()` Lua global
/// used above), and responds with an "unknown request" style message rather
/// than an error about `hl`. Detect that case so we can point the user at
/// the actual cause instead of surfacing the raw hyprctl text.
fn is_missing_eval_extension(hyprctl_stdout: &str) -> bool {
    let s = hyprctl_stdout.to_lowercase();
    s.contains("unknown request") || s.contains("unknown command")
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
        let m = Mode { width: 1920, height: 1080, refresh: 60.0 };
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
            active_mode: Mode { width: 1920, height: 1200, refresh: 60.0 },
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
            active_mode: Mode { width: 1920, height: 1080, refresh: 60.0 },
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
