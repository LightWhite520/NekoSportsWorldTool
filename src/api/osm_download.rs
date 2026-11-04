//! Public-map transport only. No application account or signed business headers.
//! Bounded fallback inspired by GAN1112233131's school-road downloader.
use std::{io::Read, time::Duration};

const PRIMARY: &str = "https://overpass-api.de/api/interpreter";
const BACKUP: &str = "https://api.openstreetmap.org/api/0.6/map";

/// south, west, north, east in WGS84. Explicit bounds avoid interpreting arbitrary QL.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds(pub [f64; 4]);

impl Bounds {
    fn query(self) -> Result<String, String> {
        let [s, w, n, e] = self.0;
        if self.0.iter().any(|v| !v.is_finite())
            || s < -85.
            || n > 85.
            || w < -180.
            || e > 180.
            || s >= n
            || w >= e
            || n - s > 0.16
            || e - w > 0.21
            || (n - s) * (e - w) > 0.03
        {
            return Err("地图备用下载范围无效或过大，请缩小范围".into());
        }
        Ok(format!("{w:.7},{s:.7},{e:.7},{n:.7}"))
    }

    pub(crate) fn around(lat: f64, lon: f64, radius: u32) -> Result<Self, String> {
        if !lat.is_finite()
            || !lon.is_finite()
            || lat.abs() > 85.
            || !(200..=2000).contains(&radius)
        {
            return Err("在线下载中心或半径无效".into());
        }
        let dy = radius as f64 / 110_000.;
        let dx = dy / (lat.abs() + dy).to_radians().cos();
        let bounds = Self([lat - dy, lon - dx, lat + dy, lon + dx]);
        bounds.query()?;
        Ok(bounds)
    }
}

pub(crate) fn request_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(status, response) => {
            let wait = response
                .header("Retry-After")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(|s| format!("请至少等待 {s} 秒后手动重试"))
                .unwrap_or_else(|| "请稍后手动重试".into());
            format!("路网服务请求失败（HTTP {status}）；{wait}")
        }
        ureq::Error::Transport(_) => "路网服务连接失败或超时，请检查网络后手动重试".into(),
    }
}

fn retryable(error: &ureq::Error) -> bool {
    match error {
        // Respect rate limits and authentication failures; no automatic 429 retry.
        ureq::Error::Status(status, _) => matches!(status, 408 | 500 | 502 | 503 | 504),
        ureq::Error::Transport(t) => matches!(
            t.kind(),
            ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Io
        ),
    }
}

pub(crate) fn download(
    query: &str,
    bounds: Bounds,
    max_bytes: u64,
    check: impl Fn() -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    download_from(PRIMARY, BACKUP, query, bounds, max_bytes, check)
}

fn download_from(
    primary: &str,
    backup: &str,
    query: &str,
    bounds: Bounds,
    max_bytes: u64,
    check: impl Fn() -> Result<(), String>,
) -> Result<Vec<u8>, String> {
    check()?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout(Duration::from_secs(55))
        .build();
    let request = |url: &str, post: bool| {
        let req = if post {
            agent.post(url)
        } else {
            agent.get(url)
        };
        let req = req.set(
            "User-Agent",
            concat!(
                "NekoSportsWorldTool/",
                env!("CARGO_PKG_VERSION"),
                " (public-map)"
            ),
        );
        if post {
            req.send_form(&[("data", query)])
        } else {
            req.call()
        }
    };
    let response = match request(primary, true) {
        Ok(response) => response,
        Err(error) if retryable(&error) => {
            check()?;
            let primary_error = request_error(error);
            let url = format!("{backup}?bbox={}", bounds.query()?);
            request(&url, false).map_err(|e| {
                format!(
                    "主地图源失败：{primary_error}；OSM 备用源失败：{}",
                    request_error(e)
                )
            })?
        }
        Err(error) => return Err(request_error(error)),
    };
    check()?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "读取地图响应失败，请稍后重试")?;
    check()?;
    if bytes.len() as u64 > max_bytes {
        return Err("地图响应超过大小限制，请缩小查询范围".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn server(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(8);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline, "missing request");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut req = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                req.extend_from_slice(&buf[..n]);
                if let Some(end) = req.windows(4).position(|v| v == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&req[..end]);
                    let len = head
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if req.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nRetry-After: 120\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            String::from_utf8(req).unwrap()
        });
        (url, handle)
    }

    #[test]
    fn service_failure_uses_one_public_get_with_correct_bbox() {
        let (primary, first) = server(503, "private-body");
        let (backup, second) = server(200, "<osm></osm>");
        let bytes = download_from(
            &primary,
            &backup,
            "[out:xml];",
            Bounds([39., 116., 39.01, 116.02]),
            1024,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(bytes, b"<osm></osm>");
        assert!(first.join().unwrap().starts_with("POST / "));
        let req = second.join().unwrap();
        assert!(req.starts_with("GET /?bbox=116.0000000,39.0000000,116.0200000,39.0100000 "));
        let req = req.to_lowercase();
        for header in ["authorization:", "token:", "headersign:", "cookie:"] {
            assert!(!req.contains(header));
        }
    }

    #[test]
    fn no_fallback_for_rate_limit_or_oversized_success() {
        for status in [400, 401, 403, 429] {
            let (primary, thread) = server(status, "PRIVATE");
            let err = download_from(
                &primary,
                "invalid backup must not be used",
                "query",
                Bounds([0.; 4]),
                100,
                || Ok(()),
            )
            .unwrap_err();
            assert!(err.contains(&status.to_string()));
            assert!(err.contains("120 秒"));
            assert!(!err.contains("PRIVATE"));
            thread.join().unwrap();
        }
        let (primary, thread) = server(200, "too much data");
        assert!(
            download_from(&primary, "invalid", "q", Bounds([0.; 4]), 3, || Ok(()))
                .unwrap_err()
                .contains("大小")
        );
        thread.join().unwrap();
    }

    #[test]
    fn cancellation_prevents_backup_and_bounds_are_limited() {
        let (primary, thread) = server(502, "");
        let checks = std::cell::Cell::new(0);
        let err = download_from(
            &primary,
            "invalid",
            "q",
            Bounds([39., 116., 39.01, 116.01]),
            100,
            || {
                checks.set(checks.get() + 1);
                if checks.get() > 1 {
                    Err("cancelled".into())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert_eq!(err, "cancelled");
        thread.join().unwrap();
        for b in [[f64::NAN, 0., 1., 1.], [0., 0., 2., 2.], [1., 1., 0., 0.]] {
            assert!(Bounds(b).query().is_err());
        }
        assert!(Bounds::around(39.9, 116.4, 2000).is_ok());
    }
}
