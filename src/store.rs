//! Shared Hyprland layout store: `~/.config/hypr/monitors.json`.
//!
//! Same schema as bos-settings `MonitorRule` and
//! `iso/airootfs/etc/skel/.config/hypr/scripts/display/monitors.lua`.
//! Empty `output` is the wildcard default (matches any connector).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::monitor::{Mode, Monitor};

fn default_mode() -> String {
    "preferred".to_string()
}
fn default_position() -> String {
    "auto".to_string()
}
fn default_scale() -> String {
    "auto".to_string()
}

/// One `hl.monitor()` rule. Field names and defaults must stay in sync with
/// bos-settings `MonitorRule` and the ISO `monitors.lua` loader.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorRule {
    pub output: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_position")]
    pub position: String,
    #[serde(default = "default_scale")]
    pub scale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<String>,
}

impl Default for MonitorRule {
    fn default() -> Self {
        Self {
            output: String::new(),
            mode: default_mode(),
            position: default_position(),
            scale: default_scale(),
            mirror: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorsFile {
    #[serde(default)]
    pub monitors: Vec<MonitorRule>,
}

impl Default for MonitorsFile {
    fn default() -> Self {
        Self {
            monitors: vec![MonitorRule::default()],
        }
    }
}

/// `~/.config/hypr/monitors.json` — same path Hyprland and bos-settings use.
pub fn config_path() -> PathBuf {
    bread_utils::xdg::config_dir("hypr").join("monitors.json")
}

pub fn load() -> Result<Option<MonitorsFile>> {
    load_from(&config_path())
}

pub fn load_from(path: &Path) -> Result<Option<MonitorsFile>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let file: MonitorsFile = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    // Empty file ≡ missing: Lua falls back to the wildcard default rather
    // than applying zero rules (which can black-screen the session).
    if file.monitors.is_empty() {
        return Ok(None);
    }
    Ok(Some(file))
}

pub fn save(file: &MonitorsFile) -> Result<()> {
    save_to(&config_path(), file)
}

pub fn save_to(path: &Path, file: &MonitorsFile) -> Result<()> {
    let json = serde_json::to_string_pretty(file).context("failed to serialize monitors.json")?;
    bread_utils::atomic::write_atomic_backed_up(path, &json)
        .with_context(|| format!("failed to write {}", path.display()))
}

pub fn save_from_monitors(monitors: &[Monitor]) -> Result<()> {
    save(&from_monitors(monitors))
}

/// Persist the TUI layout as named `hl.monitor()` rules. Mirror slaves are
/// omitted (mirror is recorded on the source, matching `hl.monitor()`). If
/// nothing is writable, emit the wildcard default so the file is never empty.
pub fn from_monitors(monitors: &[Monitor]) -> MonitorsFile {
    let mut source_to_slave: HashMap<&str, &str> = HashMap::new();
    for m in monitors {
        if let Some(src) = &m.mirror_of {
            source_to_slave.insert(src.as_str(), m.name.as_str());
        }
    }

    let mut rules = Vec::new();
    for m in monitors {
        if m.disabled || m.mirror_of.is_some() {
            continue;
        }
        let refresh = (m.active_mode.refresh + 0.5) as u32;
        rules.push(MonitorRule {
            output: m.name.clone(),
            mode: format!(
                "{}x{}@{}",
                m.active_mode.width, m.active_mode.height, refresh
            ),
            position: format!("{}x{}", m.x, m.y),
            scale: format!("{:.2}", m.scale),
            mirror: source_to_slave
                .get(m.name.as_str())
                .map(|s| (*s).to_owned()),
        });
    }

    if rules.is_empty() {
        MonitorsFile::default()
    } else {
        MonitorsFile { monitors: rules }
    }
}

/// Overlay persisted rules onto live `hyprctl` monitors (matched by name;
/// empty `output` is the wildcard fallback). `preferred` / `auto` leave the
/// live value. A file with at least one named output is treated as a full
/// layout and replaces live mirrors; a wildcard-only file does not.
pub fn apply_to_monitors(file: &MonitorsFile, monitors: &mut [Monitor]) {
    let has_specific = file.monitors.iter().any(|r| !r.output.is_empty());
    if has_specific {
        for m in monitors.iter_mut() {
            m.mirror_of = None;
        }
        for rule in &file.monitors {
            let Some(slave_name) = rule.mirror.as_deref().filter(|s| !s.is_empty()) else {
                continue;
            };
            if rule.output.is_empty() {
                continue;
            }
            if let Some(slave) = monitors.iter_mut().find(|m| m.name == slave_name) {
                slave.mirror_of = Some(rule.output.clone());
            }
        }
    }

    for m in monitors.iter_mut() {
        if let Some(rule) = find_rule(&file.monitors, &m.name) {
            apply_rule_fields(m, rule);
        }
    }
}

fn find_rule<'a>(rules: &'a [MonitorRule], name: &str) -> Option<&'a MonitorRule> {
    rules
        .iter()
        .find(|r| r.output == name)
        .or_else(|| rules.iter().find(|r| r.output.is_empty()))
}

fn apply_rule_fields(m: &mut Monitor, rule: &MonitorRule) {
    if rule.mode != "preferred" {
        if let Some(mode) =
            Mode::parse(&format!("{}Hz", rule.mode)).or_else(|| Mode::parse(&rule.mode))
        {
            m.active_mode = mode;
        }
    }
    if rule.position != "auto" {
        if let Some((x, y)) = parse_position(&rule.position) {
            m.x = x;
            m.y = y;
        }
    }
    if rule.scale != "auto" {
        if let Ok(scale) = rule.scale.parse::<f64>() {
            if scale > 0.0 {
                m.scale = scale;
            }
        }
    }
}

fn parse_position(s: &str) -> Option<(i32, i32)> {
    let (x, y) = s.split_once('x')?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::Transform;

    fn make_monitor(name: &str, w: u32, h: u32, x: i32, y: i32) -> Monitor {
        Monitor {
            name: name.into(),
            description: String::new(),
            active_mode: Mode {
                width: w,
                height: h,
                refresh: 60.0,
            },
            x,
            y,
            scale: 1.0,
            transform: Transform::Normal,
            vrr: false,
            dpms: true,
            disabled: false,
            mirror_of: None,
            available_modes: vec![],
            physical_width_mm: 0,
            physical_height_mm: 0,
        }
    }

    #[test]
    fn iso_default_parses() {
        let json = r#"{
  "monitors": [
    { "output": "", "mode": "preferred", "position": "auto", "scale": "auto" }
  ]
}"#;
        let file: MonitorsFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.monitors.len(), 1);
        assert_eq!(file.monitors[0], MonitorRule::default());
    }

    #[test]
    fn pretty_roundtrip_omits_absent_mirror() {
        let file = MonitorsFile::default();
        let json = serde_json::to_string_pretty(&file).unwrap();
        assert!(json.contains("\"output\": \"\""));
        assert!(json.contains("\"mode\": \"preferred\""));
        assert!(!json.contains("mirror"));
        let back: MonitorsFile = serde_json::from_str(&json).unwrap();
        assert_eq!(file, back);
    }

    #[test]
    fn from_monitors_writes_named_rules_and_source_mirror() {
        let mut hdmi = make_monitor("HDMI-A-1", 1920, 1080, 1920, 0);
        hdmi.mirror_of = Some("eDP-1".into());
        let file = from_monitors(&[make_monitor("eDP-1", 1920, 1200, 0, 0), hdmi]);
        assert_eq!(file.monitors.len(), 1);
        let rule = &file.monitors[0];
        assert_eq!(rule.output, "eDP-1");
        assert_eq!(rule.mode, "1920x1200@60");
        assert_eq!(rule.position, "0x0");
        assert_eq!(rule.scale, "1.00");
        assert_eq!(rule.mirror.as_deref(), Some("HDMI-A-1"));
    }

    #[test]
    fn from_monitors_empty_or_all_slaves_emits_wildcard() {
        let mut only_slave = make_monitor("HDMI-A-1", 1920, 1080, 0, 0);
        only_slave.mirror_of = Some("missing".into());
        assert_eq!(from_monitors(&[]), MonitorsFile::default());
        assert_eq!(from_monitors(&[only_slave]), MonitorsFile::default());
    }

    #[test]
    fn wildcard_overlay_leaves_live_geometry_and_mirrors() {
        let file = MonitorsFile::default();
        let mut monitors = vec![make_monitor("eDP-1", 1920, 1200, 10, 20)];
        monitors[0].scale = 1.5;
        monitors[0].mirror_of = Some("HDMI-A-1".into());
        apply_to_monitors(&file, &mut monitors);
        assert_eq!(monitors[0].x, 10);
        assert_eq!(monitors[0].y, 20);
        assert!((monitors[0].scale - 1.5).abs() < f64::EPSILON);
        assert_eq!(monitors[0].mirror_of.as_deref(), Some("HDMI-A-1"));
    }

    #[test]
    fn specific_overlay_applies_fields_and_replaces_mirrors() {
        let file = MonitorsFile {
            monitors: vec![
                MonitorRule {
                    output: "eDP-1".into(),
                    mode: "1920x1200@60".into(),
                    position: "0x0".into(),
                    scale: "1.25".into(),
                    mirror: Some("HDMI-A-1".into()),
                },
                MonitorRule {
                    output: "DP-1".into(),
                    mode: "2560x1440@144".into(),
                    position: "-2560x0".into(),
                    scale: "1".into(),
                    mirror: None,
                },
            ],
        };
        let mut monitors = vec![
            make_monitor("eDP-1", 1600, 900, 100, 100),
            make_monitor("HDMI-A-1", 1920, 1080, 200, 0),
            make_monitor("DP-1", 1920, 1080, 300, 0),
        ];
        monitors[1].mirror_of = Some("DP-1".into());
        apply_to_monitors(&file, &mut monitors);

        assert_eq!(monitors[0].active_mode.width, 1920);
        assert_eq!(monitors[0].active_mode.height, 1200);
        assert!((monitors[0].active_mode.refresh - 60.0).abs() < 0.01);
        assert_eq!(monitors[0].x, 0);
        assert_eq!(monitors[0].y, 0);
        assert!((monitors[0].scale - 1.25).abs() < f64::EPSILON);

        assert_eq!(monitors[1].mirror_of.as_deref(), Some("eDP-1"));

        assert_eq!(monitors[2].active_mode.width, 2560);
        assert_eq!(monitors[2].x, -2560);
        assert!(monitors[2].mirror_of.is_none());
    }

    #[test]
    fn load_from_missing_or_empty_is_none() {
        let dir = std::env::temp_dir().join(format!(
            "breadmon-store-test-{}-{}",
            std::process::id(),
            "empty"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("nope.json");
        assert!(load_from(&missing).unwrap().is_none());

        let empty = dir.join("empty.json");
        std::fs::write(&empty, "{ \"monitors\": [] }\n").unwrap();
        assert!(load_from(&empty).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_to_roundtrips() {
        let dir = std::env::temp_dir().join(format!(
            "breadmon-store-test-{}-{}",
            std::process::id(),
            "save"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("monitors.json");
        let file = from_monitors(&[make_monitor("eDP-1", 1920, 1200, 0, 0)]);
        save_to(&path, &file).unwrap();
        let loaded = load_from(&path).unwrap().unwrap();
        assert_eq!(loaded, file);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
