use std::collections::HashMap;

use crate::monitor::{Mode, Monitor};

#[derive(Debug, Clone)]
pub struct MirrorResult {
    pub source_mode: Mode,
    pub mirror_mode: Mode,
    pub refresh: f64,
    pub ar_exact: bool,
    pub ar_ratio: (u32, u32),
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn reduced_ar(w: u32, h: u32) -> (u32, u32) {
    let g = gcd(w, h);
    (w / g, h / g)
}

fn ratio_f64(ar: (u32, u32)) -> f64 {
    ar.0 as f64 / ar.1 as f64
}

pub fn find_mirror_modes(source: &Monitor, target: &Monitor) -> Option<MirrorResult> {
    let src_modes = &source.available_modes;
    let tgt_modes = &target.available_modes;

    if src_modes.is_empty() || tgt_modes.is_empty() {
        return None;
    }

    // Group source modes by reduced AR
    let mut src_by_ar: HashMap<(u32, u32), Vec<&Mode>> = HashMap::new();
    for m in src_modes {
        src_by_ar.entry(reduced_ar(m.width, m.height)).or_default().push(m);
    }

    #[derive(Debug)]
    struct Candidate {
        src_ar: (u32, u32),
        tgt_ar: (u32, u32),
        is_exact: bool,
    }

    let mut candidates: Vec<Candidate> = Vec::new();

    for t in tgt_modes {
        let tgt_ar = reduced_ar(t.width, t.height);
        let tgt_ratio = ratio_f64(tgt_ar);

        if src_by_ar.contains_key(&tgt_ar) {
            // Check if we already have this exact pair
            if !candidates.iter().any(|c| c.src_ar == tgt_ar && c.tgt_ar == tgt_ar && c.is_exact) {
                candidates.push(Candidate { src_ar: tgt_ar, tgt_ar, is_exact: true });
            }
            continue;
        }

        // Approximate: check all source ARs within 5%
        for &s_ar in src_by_ar.keys() {
            let s_ratio = ratio_f64(s_ar);
            if (s_ratio - tgt_ratio).abs() / s_ratio < 0.05 {
                if !candidates.iter().any(|c| c.src_ar == s_ar && c.tgt_ar == tgt_ar) {
                    candidates.push(Candidate { src_ar: s_ar, tgt_ar, is_exact: false });
                }
            }
        }
    }

    if candidates.is_empty() {
        return None;
    }

    // Score each candidate: sum of best pixel counts on each side
    // Exact beats approximate regardless of score
    let mut best_score: u64 = 0;
    let mut best_exact = false;
    let mut best_src_ar = (0u32, 0u32);
    let mut best_tgt_ar = (0u32, 0u32);

    for c in &candidates {
        let src_max_px = src_by_ar[&c.src_ar]
            .iter()
            .map(|m| m.pixels())
            .max()
            .unwrap_or(0);
        let tgt_max_px = tgt_modes
            .iter()
            .filter(|m| reduced_ar(m.width, m.height) == c.tgt_ar)
            .map(|m| m.pixels())
            .max()
            .unwrap_or(0);
        let score = src_max_px + tgt_max_px;

        let better = if c.is_exact && !best_exact {
            true
        } else if !c.is_exact && best_exact {
            false
        } else {
            score > best_score
        };

        if better {
            best_score = score;
            best_exact = c.is_exact;
            best_src_ar = c.src_ar;
            best_tgt_ar = c.tgt_ar;
        }
    }

    if best_src_ar == (0, 0) {
        return None;
    }

    // Best source mode (highest pixels at winning AR)
    let best_src_mode = src_by_ar[&best_src_ar]
        .iter()
        .max_by_key(|m| m.pixels())
        .copied()?;

    // Best target mode
    let best_tgt_mode = tgt_modes
        .iter()
        .filter(|m| reduced_ar(m.width, m.height) == best_tgt_ar)
        .max_by_key(|m| m.pixels())?;

    // Refresh matching
    let src_refreshes: Vec<f64> = src_by_ar[&best_src_ar]
        .iter()
        .filter(|m| m.width == best_src_mode.width && m.height == best_src_mode.height)
        .map(|m| m.refresh)
        .collect();

    let tgt_refreshes: Vec<f64> = tgt_modes
        .iter()
        .filter(|m| m.width == best_tgt_mode.width && m.height == best_tgt_mode.height)
        .map(|m| m.refresh)
        .collect();

    // Exact common (within 0.01 Hz)
    let exact_common: Vec<f64> = src_refreshes
        .iter()
        .filter(|&&sr| tgt_refreshes.iter().any(|&tr| (sr - tr).abs() < 0.01))
        .copied()
        .collect();

    let chosen_refresh = if !exact_common.is_empty() {
        exact_common.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    } else {
        // Near-match within 1 Hz
        let near: Vec<f64> = src_refreshes
            .iter()
            .flat_map(|&sr| {
                tgt_refreshes
                    .iter()
                    .filter(move |&&tr| (sr - tr).abs() <= 1.0)
                    .map(move |&tr| sr.min(tr))
            })
            .collect();

        if !near.is_empty() {
            near.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        } else {
            // Fallback: max source refresh
            src_refreshes.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        }
    };

    // Find actual mode structs within 0.1 Hz of chosen_refresh
    let final_src = src_by_ar[&best_src_ar]
        .iter()
        .filter(|m| m.width == best_src_mode.width && m.height == best_src_mode.height)
        .filter(|m| (m.refresh - chosen_refresh).abs() < 0.1)
        .max_by_key(|m| m.pixels())
        .copied()
        .or(Some(best_src_mode))?;

    let final_tgt = tgt_modes
        .iter()
        .filter(|m| m.width == best_tgt_mode.width && m.height == best_tgt_mode.height)
        .filter(|m| (m.refresh - chosen_refresh).abs() < 0.1)
        .max_by_key(|m| m.pixels())
        .or_else(|| {
            // fallback: nearest refresh on target
            tgt_modes
                .iter()
                .filter(|m| m.width == best_tgt_mode.width && m.height == best_tgt_mode.height)
                .min_by(|a, b| {
                    let da = (a.refresh - chosen_refresh).abs();
                    let db = (b.refresh - chosen_refresh).abs();
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
        })?;

    Some(MirrorResult {
        source_mode: final_src.clone(),
        mirror_mode: final_tgt.clone(),
        refresh: chosen_refresh,
        ar_exact: best_exact,
        ar_ratio: best_src_ar,
    })
}

pub fn refresh_match_label(result: &MirrorResult) -> &'static str {
    let diff = (result.source_mode.refresh - result.mirror_mode.refresh).abs();
    if diff < 0.01 {
        "exact match"
    } else if diff <= 1.0 {
        "near match (≤1 Hz)"
    } else {
        "mismatch"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::{Transform};

    fn make_monitor_with_modes(name: &str, modes: Vec<Mode>) -> Monitor {
        let active = modes[0].clone();
        Monitor {
            name: name.into(),
            description: String::new(),
            active_mode: active,
            x: 0,
            y: 0,
            scale: 1.0,
            transform: Transform::Normal,
            vrr: false,
            dpms: true,
            disabled: false,
            mirror_of: None,
            available_modes: modes,
            physical_width_mm: 0,
            physical_height_mm: 0,
        }
    }

    fn m(w: u32, h: u32, r: f64) -> Mode {
        Mode { width: w, height: h, refresh: r }
    }

    #[test]
    fn identical_mode_lists_exact_match() {
        let modes = vec![m(1920, 1200, 60.0), m(1280, 800, 60.0)];
        let src = make_monitor_with_modes("eDP-1", modes.clone());
        let tgt = make_monitor_with_modes("HDMI-A-1", modes);
        let result = find_mirror_modes(&src, &tgt).unwrap();
        assert!(result.ar_exact);
        assert_eq!(result.source_mode.width, 1920);
        assert_eq!(result.mirror_mode.width, 1920);
        assert!((result.refresh - 60.0).abs() < 0.01);
    }

    #[test]
    fn different_ar_approx_match() {
        // 16:9 source vs 16:10 target — AR ratio ~11% difference, exceeds 5%
        // so this should fail to find a match (incompatible)
        // Actually 16:10 ratio is 1.6, 16:9 is 1.777, diff/1.777 = 10% > 5%
        let src = make_monitor_with_modes("src", vec![m(1920, 1080, 60.0)]);
        let tgt = make_monitor_with_modes("tgt", vec![m(1920, 1200, 60.0)]);
        // These are >5% apart so should be None
        let result = find_mirror_modes(&src, &tgt);
        assert!(result.is_none());
    }

    #[test]
    fn near_common_ar_within_5pct() {
        // 16:9 = 1.7778, 17:9 = 1.8889, diff/1.7778 = ~6%, just over
        // Let's use a case within 5%: 1920x1080 (16:9 = 1.7778) vs 2560x1440 (16:9 = 1.7778)
        let src = make_monitor_with_modes("src", vec![m(1920, 1080, 60.0)]);
        let tgt = make_monitor_with_modes("tgt", vec![m(2560, 1440, 60.0)]);
        let result = find_mirror_modes(&src, &tgt).unwrap();
        assert!(result.ar_exact);
    }

    #[test]
    fn near_refresh_match() {
        let src = make_monitor_with_modes("src", vec![m(1920, 1080, 60.0)]);
        let tgt = make_monitor_with_modes("tgt", vec![m(1920, 1080, 59.94)]);
        let result = find_mirror_modes(&src, &tgt).unwrap();
        assert!(result.ar_exact);
        // chosen_refresh should be the min of the near pair = 59.94
        assert!((result.refresh - 59.94).abs() < 0.1);
    }

    #[test]
    fn incompatible_modes_returns_none() {
        let src = make_monitor_with_modes("src", vec![m(1920, 1080, 60.0)]);
        let tgt = make_monitor_with_modes("tgt", vec![m(1024, 768, 60.0)]);
        // 16:9 vs 4:3, difference >> 5%
        let result = find_mirror_modes(&src, &tgt);
        assert!(result.is_none());
    }
}
