use std::f32::consts::TAU;

const ORBIT: [char; 8] = ['⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
const ORBIT_RATE: f32 = 12.0;
const DENSITY: [char; 9] = ['⠀', '⡀', '⣀', '⣄', '⣤', '⣦', '⣶', '⣷', '⣿'];
const COMET_PERIOD: f32 = 1.8;
const TAIL_SHARE: f32 = 0.45;
const MIN_TAIL: f32 = 3.0;

pub fn orbit(t: f32) -> char {
    ORBIT[(t * ORBIT_RATE) as usize % ORBIT.len()]
}

pub fn comet(t: f32, width: usize) -> Vec<(char, f32)> {
    if width == 0 {
        return Vec::new();
    }
    let travel = (width - 1) as f32;
    let phase = (t / COMET_PERIOD).fract();
    let eased = (1.0 - (phase * TAU).cos()) / 2.0;
    let head = travel * eased;
    let heading_right = phase < 0.5;
    let tail = (width as f32 * TAIL_SHARE).max(MIN_TAIL);

    (0..width)
        .map(|i| {
            let behind = if heading_right { head - i as f32 } else { i as f32 - head };
            let intensity = if behind < -0.5 { 0.0 } else { (1.0 - behind.max(0.0) / tail).clamp(0.0, 1.0) };
            let level = (intensity * (DENSITY.len() - 1) as f32).round() as usize;
            (DENSITY[level.min(DENSITY.len() - 1)], intensity)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_cycles_through_frames() {
        let frames: std::collections::HashSet<char> = (0..8).map(|i| orbit(i as f32 / ORBIT_RATE)).collect();
        assert_eq!(frames.len(), ORBIT.len());
    }

    #[test]
    fn comet_has_bright_head_and_fading_tail() {
        let cells = comet(COMET_PERIOD * 0.25, 20);
        let head = cells.iter().map(|(_, i)| *i).fold(0.0f32, f32::max);
        assert!((head - 1.0).abs() < 0.1);
        let lit = cells.iter().filter(|(_, i)| *i > 0.0).count();
        assert!(lit > 3 && lit < 20);
    }

    #[test]
    fn comet_is_deterministic_and_handles_zero_width() {
        assert_eq!(comet(0.7, 12), comet(0.7, 12));
        assert!(comet(0.7, 0).is_empty());
    }
}
