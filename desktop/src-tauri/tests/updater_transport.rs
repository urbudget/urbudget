//! Exercise the real standalone updater against a loopback fault server. These tests download
//! four harmless bytes, never install a package, and never open UrBudget's data directory.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

// Public test vectors from tauri-plugin-updater 2.14's offline tests (MIT/Apache-2.0).
// The signature is for the literal bytes "test", version 2.0.0. No private key is needed.
const KEY: &str = "untrusted comment: minisign public key: A49EF4746FA6E6D7\nRWTX5qZvdPSepDGWg2bz8n6ArQnBMwgQK1geVosorcbLzcunD+KJ+cYe";
const SIGNATURE: &str = "untrusted comment: signature from tauri secret key\nRUTX5qZvdPSepPF2zsjvP/L/TQtBLRnWRvaq+4omK1UEugtQp/USrGiI1v762sDy3id3aor7bRC9ETyeot6lMHHrWoZ5hjUM1Qc=\ntrusted comment: timestamp:1791649051\tfile:payload\tversion:2.0.0\nDDq2CLOEl5cQmC2eJRHlnZPVViOZsu1CjpPKQoei4A/XBfkDqGO0B+QMrcrHgEQ5qV3mne75tdFWhJpqUXh9AA==";
const TIMEOUT: Duration = Duration::from_millis(150);

#[derive(Clone)]
enum Reply {
    Body(Vec<u8>),
    NoHeaders,
    PartialAndStall,
    Truncated,
    Unavailable,
}

struct Server {
    origin: String,
    stop: Arc<AtomicBool>,
    package_requests: Arc<AtomicUsize>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new(version: &str, replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let manifest = serde_json::to_vec(&json!({
            "version": version,
            "url": format!("{origin}/package"),
            "signature": STANDARD.encode(SIGNATURE),
            "notes": "Local transport test",
        }))
        .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let package_requests = Arc::new(AtomicUsize::new(0));
        let requests = package_requests.clone();
        let worker = thread::spawn(move || {
            let mut clients = Vec::new();
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let manifest = manifest.clone();
                        let replies = replies.clone();
                        let requests = requests.clone();
                        clients.push(thread::spawn(move || {
                            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                            stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                            let mut request = Vec::new();
                            let mut byte = [0];
                            while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                                if stream.read(&mut byte).unwrap_or(0) == 0 { return; }
                                request.push(byte[0]);
                            }
                            if request.starts_with(b"GET /update ") {
                                send(&mut stream, "200 OK", &manifest);
                                return;
                            }
                            assert!(request.starts_with(b"GET /package "));
                            let n = requests.fetch_add(1, Ordering::AcqRel);
                            match replies.get(n).unwrap_or_else(|| replies.last().unwrap()) {
                                Reply::Body(body) => send(&mut stream, "200 OK", body),
                                Reply::Unavailable => send(&mut stream, "503 Service Unavailable", b"retry later"),
                                Reply::NoHeaders => thread::sleep(Duration::from_millis(600)),
                                Reply::PartialAndStall | Reply::Truncated => {
                                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nte");
                                    let _ = stream.flush();
                                    if matches!(replies.get(n).unwrap_or_else(|| replies.last().unwrap()), Reply::PartialAndStall) {
                                        thread::sleep(Duration::from_millis(600));
                                    }
                                }
                            }
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("loopback server: {error}"),
                }
            }
            for client in clients {
                client.join().unwrap();
            }
        });
        Self {
            origin,
            stop,
            package_requests,
            worker: Some(worker),
        }
    }

    fn updater(&self) -> Updater {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            json!({
                "pubkey": STANDARD.encode(KEY),
                "requireSignedVersion": true,
                // Only the mock runtime can reach this loopback server; production config is unchanged.
                "dangerousInsecureTransportProtocol": true,
            }),
        );
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        app.updater_builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .endpoints(vec![format!("{}/update", self.origin).parse().unwrap()])
            .unwrap()
            .build()
            .unwrap()
    }

    async fn update(&self) -> Update {
        self.updater()
            .check()
            .await
            .unwrap()
            .expect("newer test version")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn send(stream: &mut TcpStream, status: &str, body: &[u8]) {
    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n", body.len());
    let _ = stream.write_all(body);
}

#[test]
fn downloads_and_verifies_a_version_bound_package_without_installing() {
    tauri::async_runtime::block_on(async {
        let server = Server::new("2.0.0", vec![Reply::Body(b"test".to_vec())]);
        let mut update = server.update().await;
        update.timeout = Some(Duration::from_secs(2));
        let mut received = 0;
        let bytes = update
            .download(
                |chunk, total| {
                    received += chunk;
                    assert_eq!(total, Some(4));
                },
                || {},
            )
            .await
            .unwrap();
        assert_eq!(bytes, b"test");
        assert_eq!(received, 4);
        assert_eq!(server.package_requests.load(Ordering::Acquire), 1);
    });
}

#[test]
fn timeout_covers_both_missing_headers_and_a_body_that_stops_mid_download() {
    tauri::async_runtime::block_on(async {
        for reply in [Reply::NoHeaders, Reply::PartialAndStall] {
            let server = Server::new("2.0.0", vec![reply]);
            let mut update = server.update().await;
            update.timeout = Some(TIMEOUT);
            let started = Instant::now();
            let error = update.download(|_, _| {}, || {}).await.unwrap_err();
            assert!(
                matches!(error, tauri_plugin_updater::Error::Reqwest(ref error) if error.is_timeout()),
                "{error}"
            );
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    });
}

#[test]
fn a_failed_download_can_be_retried_with_the_same_update() {
    tauri::async_runtime::block_on(async {
        let server = Server::new(
            "2.0.0",
            vec![Reply::PartialAndStall, Reply::Body(b"test".to_vec())],
        );
        let mut update = server.update().await;
        update.timeout = Some(TIMEOUT);
        assert!(update.download(|_, _| {}, || {}).await.is_err());
        update.timeout = Some(Duration::from_secs(2));
        assert_eq!(update.download(|_, _| {}, || {}).await.unwrap(), b"test");
        assert_eq!(server.package_requests.load(Ordering::Acquire), 2);
    });
}

#[test]
fn incomplete_http_error_and_tampered_packages_are_never_accepted() {
    tauri::async_runtime::block_on(async {
        for reply in [
            Reply::Truncated,
            Reply::Unavailable,
            Reply::Body(b"evil".to_vec()),
        ] {
            let server = Server::new("2.0.0", vec![reply]);
            let mut update = server.update().await;
            update.timeout = Some(Duration::from_secs(2));
            assert!(update.download(|_, _| {}, || {}).await.is_err());
        }
    });
}

#[test]
fn valid_signature_cannot_relabel_an_older_package_as_a_newer_version() {
    tauri::async_runtime::block_on(async {
        let server = Server::new("3.0.0", vec![Reply::Body(b"test".to_vec())]);
        let mut update = server.update().await;
        update.timeout = Some(Duration::from_secs(2));
        let error = update.download(|_, _| {}, || {}).await.unwrap_err();
        assert!(
            matches!(
                error,
                tauri_plugin_updater::Error::SignedVersionMismatch { .. }
            ),
            "{error}"
        );
    });
}

#[test]
fn same_or_older_versions_are_not_downloaded() {
    tauri::async_runtime::block_on(async {
        // The mock runtime is 0.1.0.
        for version in ["0.1.0", "0.0.9"] {
            let server = Server::new(version, vec![Reply::Body(b"test".to_vec())]);
            assert!(server.updater().check().await.unwrap().is_none());
            assert_eq!(server.package_requests.load(Ordering::Acquire), 0);
        }
    });
}
