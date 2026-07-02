use crate::monitor::Monitor;

#[derive(Debug, Clone)]
pub struct LayoutState {
    pub selected: usize,
    pub snap_threshold: i32,
    pub zoom: f32,
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            selected: 0,
            snap_threshold: 10,
            zoom: 1.0,
        }
    }
}

impl LayoutState {
    pub fn clamp_selected(&mut self, count: usize) {
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }

    pub fn next(&mut self, count: usize) {
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    pub fn prev(&mut self, count: usize) {
        if count > 0 {
            self.selected = self.selected.checked_sub(1).unwrap_or(count - 1);
        }
    }
}

/// Snap a monitor being moved to the edges of its neighbours.
/// Returns the adjusted (x, y) after snapping.
pub fn snap_position(
    moving_idx: usize,
    new_x: i32,
    new_y: i32,
    monitors: &[Monitor],
    threshold: i32,
) -> (i32, i32) {
    let mw = monitors[moving_idx].world_width() as i32;
    let mh = monitors[moving_idx].world_height() as i32;

    // Candidate snaps: (snapped_x, snapped_y, priority)
    // We collect the closest snap in each axis independently
    let mut best_x_dist = threshold + 1;
    let mut best_y_dist = threshold + 1;
    let mut snap_x = new_x;
    let mut snap_y = new_y;

    for (i, other) in monitors.iter().enumerate() {
        if i == moving_idx {
            continue;
        }
        let ox = other.x;
        let oy = other.y;
        let ow = other.world_width() as i32;
        let oh = other.world_height() as i32;

        // X-axis edge pairs:
        // moving left vs other left
        let pairs_x = [
            (new_x, ox),           // left aligned
            (new_x, ox + ow),      // moving left snaps to other right
            (new_x + mw, ox),      // moving right snaps to other left
            (new_x + mw, ox + ow), // right aligned
        ];
        for (ma, oa) in pairs_x {
            let dist = (ma - oa).abs();
            if dist < best_x_dist {
                best_x_dist = dist;
                snap_x = new_x + (oa - ma);
            }
        }

        // Y-axis edge pairs
        let pairs_y = [
            (new_y, oy),
            (new_y, oy + oh),
            (new_y + mh, oy),
            (new_y + mh, oy + oh),
        ];
        for (ma, oa) in pairs_y {
            let dist = (ma - oa).abs();
            if dist < best_y_dist {
                best_y_dist = dist;
                snap_y = new_y + (oa - ma);
            }
        }
    }

    (snap_x, snap_y)
}

/// Move the selected monitor by (dx, dy) pixels, then snap.
pub fn move_selected(state: &LayoutState, monitors: &mut Vec<Monitor>, dx: i32, dy: i32) {
    let idx = state.selected;
    if idx >= monitors.len() {
        return;
    }
    let new_x = monitors[idx].x + dx;
    let new_y = monitors[idx].y + dy;
    let (sx, sy) = snap_position(idx, new_x, new_y, monitors, state.snap_threshold);
    monitors[idx].x = sx;
    monitors[idx].y = sy;
}

/// Place monitors in a left-to-right row with no gaps.
pub fn auto_arrange(monitors: &mut Vec<Monitor>) {
    let mut cursor = 0i32;
    for m in monitors.iter_mut() {
        m.x = cursor;
        m.y = 0;
        cursor += m.world_width() as i32;
    }
}

/// Compute canvas scale: returns pixels-per-cell such that all monitors fit in (area_w, area_h) cells.
/// The 0.5 factor corrects for terminal cells being ~2:1 tall.
pub fn canvas_scale(monitors: &[Monitor], area_w: u16, area_h: u16, zoom: f32) -> f32 {
    if monitors.is_empty() || area_w == 0 || area_h == 0 {
        return 1.0;
    }
    let (min_x, min_y, max_x, max_y) = bounding_box(monitors);
    let total_w = (max_x - min_x).max(1) as f32;
    let total_h = (max_y - min_y).max(1) as f32;

    let sx = area_w as f32 / total_w;
    let sy = area_h as f32 / total_h * 0.5; // cell-aspect correction
    sx.min(sy) * zoom
}

/// (min_x, min_y, max_x, max_y) in world coordinates
pub fn bounding_box(monitors: &[Monitor]) -> (i32, i32, i32, i32) {
    let min_x = monitors.iter().map(|m| m.x).min().unwrap_or(0);
    let min_y = monitors.iter().map(|m| m.y).min().unwrap_or(0);
    let max_x = monitors.iter().map(|m| m.right_edge()).max().unwrap_or(1);
    let max_y = monitors.iter().map(|m| m.bottom_edge()).max().unwrap_or(1);
    (min_x, min_y, max_x, max_y)
}

/// Map a canvas cell position back to world coordinates (inverse of world_to_canvas)
pub fn canvas_to_world(
    col: u16,
    row: u16,
    scale: f32,
    origin_x: i32,
    origin_y: i32,
    pad_x: u16,
    pad_y: u16,
) -> (i32, i32) {
    let wx = if col >= pad_x {
        ((col - pad_x) as f32 / scale) as i32 + origin_x
    } else {
        origin_x - (((pad_x - col) as f32 / scale) as i32)
    };
    let wy = if row >= pad_y {
        ((row - pad_y) as f32 / (scale * 0.5)) as i32 + origin_y
    } else {
        origin_y - (((pad_y - row) as f32 / (scale * 0.5)) as i32)
    };
    (wx, wy)
}

/// Map a world coordinate to a canvas cell position
pub fn world_to_canvas(
    wx: i32,
    wy: i32,
    scale: f32,
    origin_x: i32,
    origin_y: i32,
    pad_x: u16,
    pad_y: u16,
) -> (u16, u16) {
    let cx = ((wx - origin_x) as f32 * scale) as u16 + pad_x;
    let cy = ((wy - origin_y) as f32 * scale * 0.5) as u16 + pad_y;
    (cx, cy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::{Mode, Transform};

    fn mon(name: &str, w: u32, h: u32, x: i32, y: i32) -> Monitor {
        Monitor {
            name: name.into(),
            description: String::new(),
            active_mode: Mode { width: w, height: h, refresh: 60.0 },
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
    fn snap_to_adjacent_right_edge() {
        let monitors = vec![
            mon("A", 1920, 1200, 0, 0),
            mon("B", 1920, 1080, 1915, 0), // almost touching, 5px gap
        ];
        // Moving B (index 1) at x=1915, A's right edge is 1920
        // snap should pull B to x=1920
        let (sx, _sy) = snap_position(1, 1915, 0, &monitors, 10);
        assert_eq!(sx, 1920);
    }

    #[test]
    fn no_snap_beyond_threshold() {
        let monitors = vec![
            mon("A", 1920, 1200, 0, 0),
            mon("B", 1920, 1080, 1950, 0), // 30px gap, beyond threshold
        ];
        let (sx, _sy) = snap_position(1, 1950, 0, &monitors, 10);
        assert_eq!(sx, 1950);
    }

    #[test]
    fn auto_arrange_no_gaps() {
        let mut monitors = vec![
            mon("A", 1920, 1200, 100, 50),
            mon("B", 1280, 1024, 200, 100),
        ];
        auto_arrange(&mut monitors);
        assert_eq!(monitors[0].x, 0);
        assert_eq!(monitors[0].y, 0);
        assert_eq!(monitors[1].x, 1920);
        assert_eq!(monitors[1].y, 0);
    }
}
