use memmap2::Mmap;
use std::fs::File;
use std::path::Path;
use regex::bytes::Regex;
use base64::Engine;
use rayon::prelude::*;

use crate::db::GCodeMetadata;

fn find_pattern(
    chunks: &[(usize, &[u8])],
    pattern: &str,
) -> Option<String> {
    let re = Regex::new(pattern).ok()?;
    chunks.par_iter().find_map_first(|&(_offset, slice)| {
        re.captures(slice).map(|caps| {
            let m = caps.get(1)
                .or_else(|| caps.get(2))
                .unwrap();
            String::from_utf8_lossy(m.as_bytes()).trim().to_string()
        })
    })
}

pub fn analyze_gcode(path: &Path) -> Result<GCodeMetadata, std::io::Error> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let file_len = mmap.len();

    // Define chunk size and overlap to handle patterns crossing boundary lines
    let chunk_size = 64 * 1024; // 64KB
    let overlap = 2048; // 2KB

    // Prepare chunks starting from the end of the file back to the beginning
    let mut chunks = Vec::new();
    let mut end = file_len;
    while end > 0 {
        let start = if end > chunk_size { end - chunk_size } else { 0 };
        let slice_end = std::cmp::min(end + overlap, file_len);
        let slice = &mmap[start..slice_end];
        chunks.push((start, slice));
        end = start;
    }

    // 1. Generator/Slicer Type check
    let slicer_type = find_pattern(
        &chunks,
        r"(?i);\s*(?:generator|GENERATOR)\s*=\s*([^\r\n]+)|;\s*GENERATOR:\s*([^\r\n]+)"
    );

    // 2. Layer height check
    let layer_height = find_pattern(
        &chunks,
        r"(?i);\s*(?:layer_height|Layer height)\s*[:=]\s*([0-9.]+)"
    ).and_then(|val| val.parse::<f64>().ok());

    // 3. Estimated print time check
    let mut estimated_time = find_pattern(
        &chunks,
        r"(?i);\s*(?:TIME|estimated_time)\s*[:=]\s*([0-9.]+)"
    ).and_then(|val| val.parse::<f64>().ok());

    if estimated_time.is_none() {
        let estimated_time_hms = chunks.par_iter().find_map_first(|&(_offset, slice)| {
            let re = Regex::new(r"(?i);\s*estimated printing time\s*[^=\r\n]*=\s*(?:(\d+)h\s*)?(?:(\d+)m\s*)?(?:(\d+)s)?").unwrap();
            re.captures(slice).map(|caps| {
                let h: f64 = caps.get(1).and_then(|m| String::from_utf8_lossy(m.as_bytes()).parse().ok()).unwrap_or(0.0);
                let m: f64 = caps.get(2).and_then(|m| String::from_utf8_lossy(m.as_bytes()).parse().ok()).unwrap_or(0.0);
                let s: f64 = caps.get(3).and_then(|m| String::from_utf8_lossy(m.as_bytes()).parse().ok()).unwrap_or(0.0);
                h * 3600.0 + m * 60.0 + s
            })
        });

        if let Some(secs) = estimated_time_hms {
            if secs > 0.0 {
                estimated_time = Some(secs);
            }
        }
    }

    // 4. Thumbnail check & extraction
    let mut thumbnail_path = None;

    let thumb_begin = chunks.par_iter().find_map_first(|&(offset, slice)| {
        let re = Regex::new(r";\s*thumbnail\s+begin\s+(\d+x\d+)\s+(\d+)").unwrap();
        re.captures(slice).map(|caps| {
            let full_match = caps.get(0).unwrap();
            let abs_pos = offset + full_match.start();
            let dim = String::from_utf8_lossy(caps.get(1).unwrap().as_bytes()).into_owned();
            let len = String::from_utf8_lossy(caps.get(2).unwrap().as_bytes()).parse::<usize>().unwrap_or(0);
            let offset_after_match = offset + full_match.end();
            (abs_pos, offset_after_match, dim, len)
        })
    });

    let thumb_end = chunks.par_iter().find_map_first(|&(offset, slice)| {
        let re = Regex::new(r";\s*thumbnail\s+end").unwrap();
        re.captures(slice).map(|caps| {
            let full_match = caps.get(0).unwrap();
            let abs_pos = offset + full_match.start();
            abs_pos
        })
    });

    if let (Some((_begin_pos, after_begin_pos, dim_str, _)), Some(end_pos)) = (thumb_begin, thumb_end) {
        if after_begin_pos < end_pos {
            let base64_block = &mmap[after_begin_pos..end_pos];
            let base64_clean: String = String::from_utf8_lossy(base64_block)
                .lines()
                .map(|l| l.trim_start_matches(';').trim())
                .collect();
            
            if let Ok(png_bytes) = base64::engine::general_purpose::STANDARD.decode(base64_clean) {
                let thumb_dir = Path::new("/opt/printer_data/thumbnails");
                let fallback_dir = dirs::home_dir()
                    .map(|h| h.join(".config/rmr/thumbnails"))
                    .unwrap_or_else(|| std::env::temp_dir().join("rmr/thumbnails"));

                let final_dir = if std::fs::create_dir_all(&thumb_dir).is_ok() {
                    thumb_dir
                } else {
                    let _ = std::fs::create_dir_all(&fallback_dir);
                    &fallback_dir
                };

                let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("thumb");
                let thumb_file_name = format!("{}_{}.png", file_stem, dim_str);
                let full_thumb_path = final_dir.join(&thumb_file_name);

                if std::fs::write(&full_thumb_path, png_bytes).is_ok() {
                    thumbnail_path = Some(full_thumb_path.to_string_lossy().to_string());
                }
            }
        }
    }

    Ok(GCodeMetadata {
        estimated_time,
        layer_height,
        slicer_type,
        thumbnail_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_gcode_analysis() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_print.gcode");

        // Write a mock G-code file with slicer comments
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "; G-code generated by PrusaSlicer").unwrap();
        writeln!(file, "; generator = PrusaSlicer 2.7.1").unwrap();
        writeln!(file, "; layer_height = 0.15").unwrap();
        writeln!(file, "; estimated printing time (normal mode) = 1h 45m 30s").unwrap();
        writeln!(file, "; thumbnail begin 16x16 100").unwrap();
        writeln!(file, "; iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAABmJLR0QA/wD/AP+gvaeTAAAAI0lEQVQ4y2NgGAWjYBSMAlrBqIGhAgxDCQAyDEXwBwADAAD//wM9ADF/268+AAAAAElFTkSuQmCC").unwrap();
        writeln!(file, "; thumbnail end").unwrap();
        writeln!(file, "G1 X10 Y10 Z0.15 E1.0").unwrap();
        writeln!(file, "M104 S0").unwrap();
        drop(file);

        let meta = analyze_gcode(&file_path).unwrap();
        
        assert_eq!(meta.slicer_type, Some("PrusaSlicer 2.7.1".to_string()));
        assert_eq!(meta.layer_height, Some(0.15));
        assert_eq!(meta.estimated_time, Some(3600.0 + 45.0 * 60.0 + 30.0));
        assert!(meta.thumbnail_path.is_some());
        
        // Check that png file exists
        let thumb_p = meta.thumbnail_path.unwrap();
        assert!(Path::new(&thumb_p).exists());

        // Cleanup
        let _ = std::fs::remove_file(&file_path);
        let _ = std::fs::remove_file(&thumb_p);
    }
}
