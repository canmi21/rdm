//! A small HTTP/1.1 server for the tests: one body, served with whatever misbehaviour a test
//! asks for -- no ranges, a wrong status, a redirect, a connection dropped part way, bytes
//! doled out slowly -- and a log of every request so a test can say what the engine did. On
//! std threads rather than the runtime under test, so a hang in one cannot hide in the other.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use reqwest::Url;

#[derive(Clone, Debug)]
pub struct Options {
	/// Answer Range requests with 206; off, every request gets the whole body and 200.
	pub ranges: bool,
	/// Send Content-Length; off, the body is chunked and its size unknown up front.
	pub length: bool,
	pub etag: Option<String>,
	pub last_modified: Option<String>,
	pub disposition: Option<String>,
	pub content_type: Option<String>,
	/// Answer this status with an empty body instead of serving.
	pub status: Option<u16>,
	/// A request for this path is answered 302 to `/target`.
	pub redirect_from: Option<String>,
	/// Close the connection after this many body bytes, on every request until `fail_times`
	/// have been cut short.
	pub fail_after: Option<usize>,
	pub fail_times: usize,
	/// Sleep this long per 4 KiB of body, to make transfers slow enough to interrupt.
	pub delay_per_chunk: Duration,
	/// Refuse a Range whose start is not zero with 200 and the whole body, as a server that
	/// advertises ranges and does not honour them would.
	pub ignore_ranges: bool,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			ranges: true,
			length: true,
			etag: None,
			last_modified: None,
			disposition: None,
			content_type: None,
			status: None,
			redirect_from: None,
			fail_after: None,
			fail_times: usize::MAX,
			delay_per_chunk: Duration::ZERO,
			ignore_ranges: false,
		}
	}
}

/// One request as the server saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
	pub method: String,
	pub path: String,
	/// `bytes=a-b` as (a, Some(b)), `bytes=a-` as (a, None).
	pub range: Option<(u64, Option<u64>)>,
	pub if_range: Option<String>,
}

/// The server's state, shared with the threads that serve. Cloning it never stops anything;
/// only the owning [`TestServer`] does, on drop -- a lesson learnt when the first served
/// connection's clone stopped the listener and the second connection was reset.
#[derive(Clone)]
struct Inner {
	addr: SocketAddr,
	body: Arc<Mutex<Vec<u8>>>,
	options: Arc<Mutex<Options>>,
	seen: Arc<Mutex<Vec<Seen>>>,
	failures: Arc<AtomicUsize>,
	open: Arc<AtomicUsize>,
	peak: Arc<AtomicUsize>,
	stop: Arc<AtomicBool>,
}

pub struct TestServer {
	inner: Inner,
}

impl TestServer {
	pub fn start(body: Vec<u8>, options: Options) -> TestServer {
		let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
		let addr = listener.local_addr().unwrap();
		listener.set_nonblocking(true).unwrap();
		let inner = Inner {
			addr,
			body: Arc::new(Mutex::new(body)),
			options: Arc::new(Mutex::new(options)),
			seen: Arc::new(Mutex::new(Vec::new())),
			failures: Arc::new(AtomicUsize::new(0)),
			open: Arc::new(AtomicUsize::new(0)),
			peak: Arc::new(AtomicUsize::new(0)),
			stop: Arc::new(AtomicBool::new(false)),
		};
		let shared = inner.clone();
		thread::spawn(move || {
			while !shared.stop.load(Ordering::Relaxed) {
				match listener.accept() {
					Ok((stream, _)) => {
						let shared = shared.clone();
						thread::spawn(move || shared.serve(stream));
					}
					Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
						thread::sleep(Duration::from_millis(5));
					}
					Err(_) => break,
				}
			}
		});
		TestServer { inner }
	}

	pub fn url(&self, path: &str) -> Url {
		Url::parse(&format!("http://{}{}", self.inner.addr, path)).unwrap()
	}

	pub fn requests(&self) -> Vec<Seen> {
		self.inner.seen.lock().unwrap().clone()
	}

	/// The most connections open at one moment so far.
	pub fn peak_connections(&self) -> usize {
		self.inner.peak.load(Ordering::Relaxed)
	}

	pub fn set_options(&self, f: impl FnOnce(&mut Options)) {
		f(&mut self.inner.options.lock().unwrap());
	}

	pub fn set_body(&self, body: Vec<u8>) {
		*self.inner.body.lock().unwrap() = body;
	}

	pub fn body(&self) -> Vec<u8> {
		self.inner.body.lock().unwrap().clone()
	}
}

impl Drop for TestServer {
	fn drop(&mut self) {
		self.inner.stop.store(true, Ordering::Relaxed);
	}
}

impl Inner {
	fn serve(&self, mut stream: TcpStream) {
		let open = self.open.fetch_add(1, Ordering::Relaxed) + 1;
		self.peak.fetch_max(open, Ordering::Relaxed);
		let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
		// One request per connection is enough for these tests; the client is told so. The
		// socket is closed politely -- our side shut, theirs read to its end -- because a close
		// with bytes still unread in the receive buffer is answered with a reset, and the client
		// then reports the reset instead of the response it already had.
		let _ = self.serve_one(&mut stream);
		let _ = stream.shutdown(std::net::Shutdown::Write);
		let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
		let mut sink = [0u8; 1024];
		while matches!(stream.read(&mut sink), Ok(n) if n > 0) {}
		self.open.fetch_sub(1, Ordering::Relaxed);
	}

	fn serve_one(&self, stream: &mut TcpStream) -> std::io::Result<()> {
		let mut reader = BufReader::new(stream.try_clone()?);
		let mut line = String::new();
		reader.read_line(&mut line)?;
		let mut words = line.split_whitespace();
		let method = words.next().unwrap_or("").to_owned();
		let path = words.next().unwrap_or("/").to_owned();
		let mut range = None;
		let mut if_range = None;
		loop {
			let mut header = String::new();
			reader.read_line(&mut header)?;
			let header = header.trim_end();
			if header.is_empty() {
				break;
			}
			let Some((name, value)) = header.split_once(':') else { continue };
			let value = value.trim();
			match name.to_ascii_lowercase().as_str() {
				"range" => range = parse_range(value),
				"if-range" => if_range = Some(value.to_owned()),
				_ => {}
			}
		}
		self.seen.lock().unwrap().push(Seen {
			method: method.clone(),
			path: path.clone(),
			range,
			if_range: if_range.clone(),
		});
		let options = self.options.lock().unwrap().clone();
		let body = self.body.lock().unwrap().clone();

		if let Some(status) = options.status {
			return write_head(stream, status, &[("Content-Length", "0".into())], true);
		}
		if options.redirect_from.as_deref() == Some(path.as_str()) {
			return write_head(
				stream,
				302,
				&[("Location", "/target".into()), ("Content-Length", "0".into())],
				true,
			);
		}
		let mut headers: Vec<(&str, String)> = Vec::new();
		if options.ranges {
			headers.push(("Accept-Ranges", "bytes".into()));
		}
		if let Some(etag) = &options.etag {
			headers.push(("ETag", etag.clone()));
		}
		if let Some(modified) = &options.last_modified {
			headers.push(("Last-Modified", modified.clone()));
		}
		if let Some(disposition) = &options.disposition {
			headers.push(("Content-Disposition", disposition.clone()));
		}
		if let Some(content_type) = &options.content_type {
			headers.push(("Content-Type", content_type.clone()));
		}
		// A validator that does not match means the file changed: the whole new body, 200.
		let stale = matches!((&if_range, &options.etag), (Some(sent), Some(now)) if sent != now);
		let honoured = options.ranges
			&& !stale
			&& range.is_some_and(|(start, _)| start == 0 || !options.ignore_ranges);
		let (status, slice) = match (honoured, range) {
			(true, Some((start, end))) => {
				let total = body.len() as u64;
				if start >= total {
					headers.push(("Content-Range", format!("bytes */{total}")));
					return write_head(stream, 416, &headers, true);
				}
				let end = end.map_or(total - 1, |e| e.min(total - 1));
				headers.push(("Content-Range", format!("bytes {start}-{end}/{total}")));
				(206, body[start as usize..=end as usize].to_vec())
			}
			_ => (200, body),
		};
		let chunked = !options.length && status == 200;
		if chunked {
			headers.push(("Transfer-Encoding", "chunked".into()));
		} else {
			headers.push(("Content-Length", slice.len().to_string()));
		}
		write_head(stream, status, &headers, false)?;
		if method == "HEAD" {
			return Ok(());
		}
		let cut =
			options.fail_after.filter(|_| self.failures.load(Ordering::Relaxed) < options.fail_times);
		let mut sent = 0usize;
		for chunk in slice.chunks(4096) {
			let chunk = match cut {
				Some(limit) if sent + chunk.len() > limit => &chunk[..limit.saturating_sub(sent)],
				_ => chunk,
			};
			if chunked {
				write!(stream, "{:x}\r\n", chunk.len())?;
				stream.write_all(chunk)?;
				stream.write_all(b"\r\n")?;
			} else {
				stream.write_all(chunk)?;
			}
			sent += chunk.len();
			if cut.is_some_and(|limit| sent >= limit) {
				// Dropped mid-body, as a flaky link would; the client sees an unexpected EOF.
				self.failures.fetch_add(1, Ordering::Relaxed);
				let _ = stream.shutdown(std::net::Shutdown::Both);
				return Ok(());
			}
			if !options.delay_per_chunk.is_zero() {
				thread::sleep(options.delay_per_chunk);
			}
		}
		if chunked {
			stream.write_all(b"0\r\n\r\n")?;
		}
		stream.flush()?;
		Ok(())
	}
}

fn write_head(
	stream: &mut TcpStream,
	status: u16,
	headers: &[(&str, String)],
	done: bool,
) -> std::io::Result<()> {
	let reason = match status {
		200 => "OK",
		206 => "Partial Content",
		302 => "Found",
		403 => "Forbidden",
		404 => "Not Found",
		416 => "Range Not Satisfiable",
		503 => "Service Unavailable",
		_ => "Status",
	};
	write!(stream, "HTTP/1.1 {status} {reason}\r\nConnection: close\r\n")?;
	for (name, value) in headers {
		write!(stream, "{name}: {value}\r\n")?;
	}
	write!(stream, "\r\n")?;
	if done {
		stream.flush()?;
	}
	Ok(())
}

fn parse_range(value: &str) -> Option<(u64, Option<u64>)> {
	let spec = value.strip_prefix("bytes=")?;
	let (start, end) = spec.split_once('-')?;
	let start = start.trim().parse().ok()?;
	let end = end.trim();
	Some((start, if end.is_empty() { None } else { Some(end.parse().ok()?) }))
}
