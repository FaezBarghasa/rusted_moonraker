use log::info;

pub struct DefectMonitor {
    pub threshold: f64,
}

impl DefectMonitor {
    pub fn new(threshold: f64) -> Self {
        Self { threshold }
    }

    pub fn compare_frames(&self, frame_a: &[u8], frame_b: &[u8]) -> f64 {
        info!("Comparing frames to detect printing defects...");
        if frame_a.len() != frame_b.len() || frame_a.is_empty() {
            return 0.0;
        }
        let diff: usize = frame_a.iter().zip(frame_b.iter())
            .map(|(a, b)| (*a as i32 - *b as i32).abs() as usize)
            .sum();
        
        let max_diff = frame_a.len() * 255;
        
        1.0 - (diff as f64 / max_diff as f64)
    }

    pub fn is_spaghetti(&self, similarity: f64) -> bool {
        similarity < self.threshold
    }
}