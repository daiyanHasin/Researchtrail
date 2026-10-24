//! ResearchTrail: a local research notebook that connects the papers you read to
//! the code you write, and remembers every version of both.
//!
//!   researchtrail [PROJECT_FOLDER] [--port N] [--no-open]
//!
//! Runs a small web server on 127.0.0.1 and opens it in your browser. Everything
//! is stored in PROJECT_FOLDER/.trail. Nothing leaves your computer except reading
//! the paper links you paste and looking up their metadata.

mod catalog;
mod code;
mod compare;
mod demo;
mod extract;
mod history;
mod pack;
mod readers;
mod search;
mod server;
mod store;
mod util;

use server::{App, Shared};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

fn open_browser(url: &str) {
    use std::process::Command;
    let _ = if cfg!(windows) { Command::new("cmd").args(["/C", "start", "", url]).spawn() }
        else if cfg!(target_os = "macos") { Command::new("open").arg(url).spawn() }
        else { Command::new("xdg-open").arg(url).spawn() };
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut folder: Option<PathBuf> = None;
    let mut port: u16 = 4400;
    let mut open = true;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()).unwrap_or(port),
            "--no-open" => open = false,
            "-h" | "--help" => {
                println!("ResearchTrail {}\n\nUsage: researchtrail [PROJECT_FOLDER] [--port N] [--no-open]\n\nOpens a local page at http://127.0.0.1:{} where you add papers by link,\nsee every version of your code, and compare what each paper did with what you did.", env!("CARGO_PKG_VERSION"), port);
                return;
            }
            _ => folder = Some(PathBuf::from(a)),
        }
    }

    let mut app = App { state: None, recent: Vec::new(), home: home(), jobs_seq: 0 };
    app.load_recent();
    if let Some(f) = folder {
        match app.open(&f) {
            Ok(()) => println!("Opened {}", f.display()),
            Err(e) => eprintln!("Couldn't open {}: {}. Choose a folder in the browser instead.", f.display(), e),
        }
    }
    let shared: Shared = Arc::new(Mutex::new(app));

    let mut server = None;
    for p in port..port + 20 {
        if let Ok(s) = tiny_http::Server::http(("127.0.0.1", p)) { server = Some((s, p)); break; }
    }
    let Some((server, port)) = server else { eprintln!("No free port between {} and {}.", port, port + 19); std::process::exit(1) };
    let server = Arc::new(server);
    let url = format!("http://127.0.0.1:{}", port);

    // Background watcher: snapshots saves and refreshes the analysis.
    {
        let shared = shared.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(2000));
            if let Ok(mut a) = shared.lock() { if let Some(st) = a.state.as_mut() { st.scan_code(true); } }
        });
    }

    let mut workers = Vec::new();
    for _ in 0..6 {
        let server = server.clone();
        let shared = shared.clone();
        workers.push(std::thread::spawn(move || {
            for req in server.incoming_requests() { server::handle(shared.clone(), req); }
        }));
    }
    println!("\n  ResearchTrail is running at {}\n  Keep this window open while you work. Press Ctrl+C to stop.\n", url);
    if open { open_browser(&url); }
    for w in workers { let _ = w.join(); }
}
