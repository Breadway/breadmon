use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::monitor::{Mode, Monitor, Transform};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMonitor {
    pub name: String,
    pub mode: String,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: u8,
    pub vrr: bool,
    pub dpms: bool,
    pub mirror_of: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub name: String,
    pub created: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub profile: ProfileMeta,
    pub monitors: Vec<ProfileMonitor>,
}

pub fn profiles_dir() -> PathBuf {
    // Was `dirs::config_dir().unwrap_or_else(|| PathBuf::from("~/.config"))`
    // — same literal-tilde-fallback bug found (and fixed) in breadclip-core
    // and breadpad-shared during tonight's ecosystem-utils pass: PathBuf/
    // std::fs never expand `~`, so on a box where `dirs` can't resolve a
    // home directory this would silently resolve to a directory literally
    // named `~` under the current working directory instead of the user's
    // actual home.
    bread_utils::xdg::config_dir("breadmon/profiles")
}

pub fn save(profile: &Profile) -> Result<()> {
    let dir = profiles_dir();
    std::fs::create_dir_all(&dir).context("failed to create profiles dir")?;
    let path = dir.join(format!("{}.toml", profile.profile.name));
    let content = toml::to_string_pretty(profile).context("failed to serialize profile")?;
    std::fs::write(&path, content).context("failed to write profile")?;
    Ok(())
}

pub fn load(name: &str) -> Result<Profile> {
    let path = profiles_dir().join(format!("{}.toml", name));
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read profile '{}'", name))?;
    toml::from_str(&content).context("failed to parse profile TOML")
}

pub fn list() -> Result<Vec<String>> {
    let dir = profiles_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir).context("failed to read profiles dir")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_owned());
            }
        }
    }
    names.sort();
    Ok(names)
}

pub fn delete(name: &str) -> Result<()> {
    let path = profiles_dir().join(format!("{}.toml", name));
    std::fs::remove_file(&path).with_context(|| format!("failed to delete profile '{}'", name))
}

pub fn from_monitors(name: &str, monitors: &[Monitor]) -> Profile {
    let profile_monitors = monitors
        .iter()
        .map(|m| ProfileMonitor {
            name: m.name.clone(),
            mode: m.active_mode.compact(),
            x: m.x,
            y: m.y,
            scale: m.scale,
            transform: m.transform.as_u8(),
            vrr: m.vrr,
            dpms: m.dpms,
            mirror_of: m.mirror_of.clone().unwrap_or_default(),
        })
        .collect();

    Profile {
        profile: ProfileMeta {
            name: name.to_owned(),
            created: chrono_now(),
        },
        monitors: profile_monitors,
    }
}

/// Apply a profile's settings onto a list of live monitors (matched by name).
/// Monitors not in the profile are left unchanged.
pub fn apply_to_monitors(profile: &Profile, monitors: &mut [Monitor]) {
    for pm in &profile.monitors {
        if let Some(m) = monitors.iter_mut().find(|m| m.name == pm.name) {
            if let Some(mode) = Mode::parse(&format!("{}Hz", pm.mode)) {
                m.active_mode = mode;
            }
            m.x = pm.x;
            m.y = pm.y;
            m.scale = pm.scale;
            m.transform = Transform::from_u8(pm.transform);
            m.vrr = pm.vrr;
            m.dpms = pm.dpms;
            m.mirror_of = if pm.mirror_of.is_empty() {
                None
            } else {
                Some(pm.mirror_of.clone())
            };
        }
    }
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without pulling in chrono
    // Uses date command; falls back to a placeholder if unavailable
    std::process::Command::new("date")
        .arg("+%Y-%m-%dT%H:%M:%SZ")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::{Mode, Transform};

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
    fn profile_roundtrip() {
        let monitors = vec![
            make_monitor("eDP-1", 1920, 1200, 0, 0),
            make_monitor("HDMI-A-1", 1920, 1080, 1920, 0),
        ];
        let profile = from_monitors("test", &monitors);
        let serialized = toml::to_string_pretty(&profile).unwrap();
        let deserialized: Profile = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.profile.name, "test");
        assert_eq!(deserialized.monitors.len(), 2);
        assert_eq!(deserialized.monitors[0].name, "eDP-1");
        assert_eq!(deserialized.monitors[0].mode, "1920x1200@60.00");
        assert_eq!(deserialized.monitors[1].x, 1920);
    }
}
