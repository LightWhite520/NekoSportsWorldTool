//! Terminal preview and an explicit post-generation decision, including EOF cancellation.
use crate::track::review::{format_pace, Point, RunReview};
use std::io::{BufRead, Write};

fn approved(text: &str) -> bool {
    matches!(text.trim(), "submit" | "提交")
}

pub(super) fn confirm(track: &crate::track::model::Track) -> Result<bool, String> {
    let review = RunReview::from_track(track)?;
    println!(
        "\n提交前预览：{:.2} 公里 / {}:{:02} / {} 步 / 爬升 {:.2} 米",
        review.distance_m / 1000.0,
        review.duration_s / 60,
        review.duration_s % 60,
        review.steps,
        review.ascent_m
    );
    println!("路线图（S 起点，E 终点，B 表示起终点重合）：");
    let mut canvas = vec![vec![' '; 60]; 14];
    let lat0 = review.route[0].0;
    let lon0 = review.route[0].1;
    let xy: Vec<_> = review
        .route
        .iter()
        .map(|&(lat, lon)| ((lon - lon0) * lat0.to_radians().cos(), lat - lat0))
        .collect();
    let min_x = xy.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x = xy.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let min_y = xy.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_y = xy.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let scale = (59.0 / (max_x - min_x).max(1e-8)).min(26.0 / (max_y - min_y).max(1e-8));
    let cell = |&(x, y): &(f64, f64)| {
        let column = ((x - min_x) * scale).round().clamp(0.0, 59.0) as usize;
        let row = ((max_y - y) * scale / 2.0).round().clamp(0.0, 13.0) as usize;
        (row, column)
    };
    for point in &xy {
        let (row, column) = cell(point);
        canvas[row][column] = '*';
    }
    // Mark endpoints last so later laps cannot overwrite the start marker.
    let start = cell(&xy[0]);
    let end = cell(xy.last().unwrap());
    canvas[start.0][start.1] = 'S';
    canvas[end.0][end.1] = if start == end { 'B' } else { 'E' };
    for row in canvas {
        println!("{}", row.iter().collect::<String>());
    }
    trace("配速", &review.pace, review.duration_s, "秒/公里");
    println!("  平均 {} /公里", format_pace(review.avg_pace_s));
    trace("步频", &review.cadence, review.duration_s, "步/分钟");
    trace("步幅", &review.stride, review.duration_s, "厘米/步");
    trace("海拔", &review.altitude, review.duration_s, "米");
    println!("输入 submit 确认提交这份数据；其他输入或回车取消：");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    Ok(approved(&answer))
}

fn trace(title: &str, points: &[Point], duration: i64, unit: &str) {
    let values: Vec<_> = points
        .iter()
        .filter_map(|p| p.value)
        .filter(|v| v.is_finite())
        .collect();
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if values.is_empty() {
        println!("{title}：暂无数据");
        return;
    }
    let glyphs: Vec<_> = "▁▂▃▄▅▆▇█".chars().collect();
    let mut line = String::new();
    for i in 0..60 {
        let time = i as f64 / 59.0 * duration as f64;
        let index = points
            .partition_point(|p| p.time_s < time)
            .min(points.len().saturating_sub(1));
        line.push(match points.get(index).and_then(|p| p.value) {
            Some(value) => {
                glyphs[((value - min) / (max - min).max(1e-8) * 7.0)
                    .round()
                    .clamp(0.0, 7.0) as usize]
            }
            None => ' ',
        });
    }
    println!("{title}：{min:.2}–{max:.2} {unit}\n{line}");
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_an_explicit_post_preview_answer_submits() {
        for text in ["", "\n", "yes", "y", "取消"] {
            assert!(!super::approved(text));
        }
        assert!(super::approved("submit\n"));
        assert!(super::approved("提交\n"));
    }
}
