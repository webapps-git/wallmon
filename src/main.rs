use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

mod screen;

use wallmon::collectors::demo::{self, Demo, Scenario};
use wallmon::collectors::{self, Shared};
use wallmon::config::{self, Config};
use wallmon::state::State;
use wallmon::ui;

const USAGE: &str = "\
Usage: wallmon [--config PATH] [--demo | --demo=failure] [--basic]

  --config PATH    configuration file (default: /etc/wallmon/config.toml)
  --demo           made-up data, for trying wallmon out without a server
  --demo=failure   made-up data with an ongoing failure
  --basic          16 colours and no braille, as on the plain Linux console
                   (chosen by itself when TERM is linux)
  -h, --help       show this help
  -V, --version    show the version

The keyboard is ignored. Ctrl+C quits.";

struct Args {
    config: Option<PathBuf>,
    demo: Option<Scenario>,
    basic: bool,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut args = Args { config: None, demo: None, basic: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("wallmon {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--demo" => args.demo = Some(Scenario::Normal),
            "--demo=failure" => args.demo = Some(Scenario::Failure),
            "--basic" => args.basic = true,
            "--config" => args.config = Some(it.next().ok_or("--config needs a path")?.into()),
            _ => match a.strip_prefix("--config=") {
                Some(p) => args.config = Some(p.into()),
                None => return Err(format!("unknown option: {a}")),
            },
        }
    }
    Ok(Some(args))
}

fn load_config(args: &Args) -> Result<Config, String> {
    match (&args.config, args.demo) {
        (Some(path), _) => Config::load(path),
        (None, Some(_)) => Ok(Config::demo()),
        (None, None) => {
            let path = Path::new(config::DEFAULT_PATH);
            if path.exists() { Config::load(path) } else { Ok(Config::default()) }
        }
    }
}

/// The plain Linux console has 16 colours and fonts without braille. kmscon and terminal
/// emulators set another TERM, or announce full colour in COLORTERM.
fn look(args: &Args) -> ui::Look {
    let var = |k| std::env::var(k).unwrap_or_default();
    if args.basic || (var("TERM") == "linux" && var("COLORTERM").is_empty()) { ui::Look::Basic } else { ui::Look::Full }
}

fn system_hostname() -> String {
    ["/proc/sys/kernel/hostname", "/etc/hostname"]
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "localhost".into())
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wallmon: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let config = match load_config(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("wallmon: {e}");
            return ExitCode::FAILURE;
        }
    };

    let hostname = config.general.name.clone().unwrap_or_else(system_hostname);
    let state: Shared = Arc::new(Mutex::new(State::new(hostname)));
    let demo = args.demo.map(|s| Arc::new(Demo::new(s, &config)));
    if let Some(d) = &demo {
        d.prefill(&mut state.lock().unwrap_or_else(|e| e.into_inner()), Instant::now());
    }

    // Sources run on a small runtime in the background; the screen is drawn on this thread.
    let quit = Arc::new(AtomicBool::new(false));
    let resized = Arc::new(AtomicBool::new(false));
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("wallmon: {e}");
            return ExitCode::FAILURE;
        }
    };
    {
        let (state, config, quit, resized) = (state.clone(), config.clone(), quit.clone(), resized.clone());
        thread::spawn(move || {
            runtime.block_on(async move {
                match demo {
                    Some(d) => demo::spawn_all(&state, d),
                    None => collectors::spawn_local(&state, &config),
                }
                watch_resize(resized);
                wait_for_signal().await;
                quit.store(true, Ordering::Relaxed);
            })
        });
    }

    let look = look(&args);
    let result = screen::enter().and_then(|mut s| run(&mut s, &state, &config, look, &quit, &resized));
    screen::leave();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wallmon: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Ctrl+C, `kill` and a closed terminal all end the program.
async fn wait_for_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut term), Ok(mut int), Ok(mut hup)) =
        (signal(SignalKind::terminate()), signal(SignalKind::interrupt()), signal(SignalKind::hangup()))
    else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
        _ = hup.recv() => {}
    }
}

/// Asks for an immediate redraw when the terminal changes size.
fn watch_resize(flag: Arc<AtomicBool>) {
    use tokio::signal::unix::{SignalKind, signal};
    if let Ok(mut winch) = signal(SignalKind::window_change()) {
        tokio::spawn(async move {
            while winch.recv().await.is_some() {
                flag.store(true, Ordering::Relaxed);
            }
        });
    }
}

/// Draws a frame at every full second, or at once after a resize.
fn run(screen: &mut screen::Screen, state: &Shared, config: &Config, look: ui::Look, quit: &AtomicBool, resized: &AtomicBool) -> std::io::Result<()> {
    loop {
        {
            let s = state.lock().unwrap_or_else(|e| e.into_inner());
            screen.draw(|f| ui::draw(f, &s, config, Instant::now(), chrono::Local::now(), look))?;
        }
        loop {
            if quit.load(Ordering::Relaxed) {
                return Ok(());
            }
            if resized.swap(false, Ordering::Relaxed) {
                screen.autoresize()?;
                break;
            }
            let left = 1000 - u64::from(chrono::Local::now().timestamp_subsec_millis().min(999));
            thread::sleep(Duration::from_millis(left.min(100)));
            if left <= 100 {
                break;
            }
        }
    }
}
