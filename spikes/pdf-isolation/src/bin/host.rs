//! Spike host. Two modes, one output line (tab-separated key=value):
//!
//!   spike-host inproc   <pdf> <libpdfium>
//!   spike-host isolated <pdf> <libpdfium> <helper> [--fd] [--sandbox <profile.sb>]
//!
//! `isolated` spawns the helper, feeds the PDF over a pipe (default; models
//! XPC data copy) or by handing the helper the file as stdin (`--fd`; models
//! passing a file descriptor), reads the framed reply, deserialises the
//! Document, and reports typed outcomes. A helper crash (signal / nonzero exit
//! / timeout) becomes `outcome=HelperCrashed{...}`, never a host crash.
//! Helper peak RSS comes from wait4's rusage; host peak RSS from getrusage.

use gist_model::{Document, ParseLimits};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn self_rss() -> i64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    ru.ru_maxrss as i64 // bytes on macOS
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: spike-host inproc|isolated <pdf> <lib> [helper] [--fd] [--sandbox p.sb]");
        std::process::exit(2);
    }
    let mode = a[1].as_str();
    let pdf = &a[2];
    let lib = &a[3];
    let t0 = Instant::now();
    match mode {
        "inproc" => {
            gist_parse_pdf::set_library_path(lib.clone());
            let bytes = std::fs::read(pdf).expect("read");
            let r = gist_parse_pdf::parse_pdf(&bytes, "spike", &ParseLimits::default());
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let (outcome, json) = match &r {
                Ok(d) => ("Ok".to_string(), serde_json::to_vec(d).map(|v| v.len()).unwrap_or(0)),
                Err(e) => (format!("Err({e})"), 0),
            };
            println!(
                "mode=inproc\twall_ms={ms:.1}\thost_rss={}\thelper_rss=0\tjson_bytes={json}\tin_bytes={}\toutcome={outcome}",
                self_rss(),
                bytes.len()
            );
        }
        "isolated" => isolated(&a, t0),
        _ => std::process::exit(2),
    }
}

fn isolated(a: &[String], t0: Instant) {
    let pdf = &a[2];
    let lib = &a[3];
    let helper = &a[4];
    let use_fd = a.iter().any(|x| x == "--fd");
    let sandbox = a.iter().position(|x| x == "--sandbox").map(|i| a[i + 1].clone());
    let file = std::fs::File::open(pdf).expect("open");
    let in_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);

    let mut cmd = if let Some(p) = &sandbox {
        let mut c = Command::new("/usr/bin/sandbox-exec");
        c.args(["-f", p, helper, lib]);
        c
    } else {
        let mut c = Command::new(helper);
        c.arg(lib);
        c
    };
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    if use_fd {
        cmd.stdin(Stdio::from(file.try_clone().expect("dup")));
    } else {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn().expect("spawn helper");
    let pid = child.id() as i32;

    // Watchdog: a hung helper is killed and reported as a typed timeout.
    let timed_out = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let (t, d) = (timed_out.clone(), done.clone());
        std::thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(20) {
                if d.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            t.store(true, std::sync::atomic::Ordering::SeqCst);
            unsafe { libc::kill(pid, libc::SIGKILL) };
        });
    }

    let writer = if use_fd {
        None
    } else {
        let mut stdin = child.stdin.take().unwrap();
        let mut f = file;
        Some(std::thread::spawn(move || {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).ok();
            let _ = stdin.write_all(&buf); // EPIPE if helper died: ignored
        }))
    };
    let mut reply = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut reply).ok();
    if let Some(w) = writer {
        let _ = w.join();
    }
    let _ = child.stdin.as_ref().map(|s| s.as_raw_fd());

    let mut status: libc::c_int = 0;
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::wait4(pid, &mut status, 0, &mut ru) };
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let helper_rss = ru.ru_maxrss as i64;

    let outcome = if timed_out.load(std::sync::atomic::Ordering::SeqCst) {
        "HelperCrashed{timeout}".to_string()
    } else if libc::WIFSIGNALED(status) {
        format!("HelperCrashed{{signal={}}}", libc::WTERMSIG(status))
    } else if libc::WEXITSTATUS(status) != 0 || reply.is_empty() {
        format!("HelperCrashed{{exit={}}}", libc::WEXITSTATUS(status))
    } else if reply[0] == 0 {
        match serde_json::from_slice::<Document>(&reply[1..]) {
            Ok(_) => "Ok".to_string(),
            Err(e) => format!("HelperProtocolError({e})"),
        }
    } else {
        format!("Err({})", String::from_utf8_lossy(&reply[1..]).replace('\t', ": "))
    };
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!(
        "mode=isolated{}{}\twall_ms={ms:.1}\thost_rss={}\thelper_rss={helper_rss}\tjson_bytes={}\tin_bytes={in_bytes}\toutcome={outcome}",
        if use_fd { "+fd" } else { "+pipe" },
        if sandbox.is_some() { "+sandbox" } else { "" },
        self_rss(),
        reply.len().saturating_sub(1)
    );
}
