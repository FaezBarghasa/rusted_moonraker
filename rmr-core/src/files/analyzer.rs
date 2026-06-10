use rayon::prelude::*;
use std::fs::File;
use std::io::{BufRead, BufReader};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GcodeMetadata {
    pub layer_count: usize,
    pub estimated_time: u64,
    pub max_z: f64,
}

pub fn analyze_gcode(filepath: &str) -> Result<GcodeMetadata, std::io::Error> {
    let file = File::open(filepath)?;
    let reader = BufReader::new(file);

    let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();

    let layer_count = lines.par_iter()
        .filter(|line| line.starts_with(";LAYER:"))
        .count();

    let max_z = lines.par_iter()
        .filter_map(|line| {
            if line.starts_with("G1 ") && line.contains(" Z") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                for part in parts {
                    if part.starts_with('Z') {
                        return part[1..].parse::<f64>().ok();
                    }
                }
            }
            None
        })
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(0.0);

    let estimated_time = (lines.len() as u64) / 50;

    Ok(GcodeMetadata {
        layer_count,
        estimated_time,
        max_z,
    })
}