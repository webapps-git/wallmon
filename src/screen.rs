//! Taking over the terminal and giving it back.
//!
//! The keyboard is never read. Echo and line editing are switched off so typed keys do not
//! smear the screen, but signals stay on: Ctrl+C sends SIGINT and closing the terminal sends
//! SIGHUP, both handled as a normal exit.

use std::io::{self, Stdout, stdout};
use std::sync::OnceLock;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};

pub type Screen = Terminal<CrosstermBackend<Stdout>>;

/// Terminal settings from before we started, restored on exit and on panic.
static SAVED: OnceLock<libc::termios> = OnceLock::new();

pub fn enter() -> io::Result<Screen> {
    // SAFETY: tcgetattr/tcsetattr only read and write the termios struct of fd 0.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::isatty(libc::STDIN_FILENO) == 1 && libc::tcgetattr(libc::STDIN_FILENO, &mut t) == 0 {
            let _ = SAVED.set(t);
            t.c_lflag &= !(libc::ECHO | libc::ICANON);
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t);
        }
    }
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        hook(info);
    }));
    execute!(stdout(), EnterAlternateScreen, Hide)?;
    Terminal::new(CrosstermBackend::new(stdout()))
}

pub fn leave() {
    if let Some(t) = SAVED.get() {
        // SAFETY: restores the struct saved by `enter`.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, t);
        }
    }
    let _ = execute!(stdout(), LeaveAlternateScreen, Show);
}
