//! Console du serveur dédié : une ligne de saisie qui **reste en bas** quand
//! le journal défile (la ligne en cours n'est plus coupée par un message).
//!
//! En terminal interactif on passe en mode « brut » (touche par touche) :
//! chaque ligne de journal efface la ligne de saisie, s'écrit, puis la
//! redessine avec ce qui avait été tapé. Sans terminal (service, redirection),
//! on lit simplement des lignes.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};

use crossterm::cursor::MoveToColumn;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::queue;
use crossterm::style::Print;
use crossterm::terminal::{self, Clear, ClearType};

const PROMPT: &str = "> ";

static ACTIVE: AtomicBool = AtomicBool::new(false);
static BUFFER: Mutex<String> = Mutex::new(String::new());

/// Écrit une ligne de journal en gardant la saisie intacte.
pub fn print(line: &str) {
    if ACTIVE.load(Ordering::Relaxed) {
        let buf = BUFFER.lock().map(|b| b.clone()).unwrap_or_default();
        let mut out = std::io::stdout().lock();
        let _ = queue!(
            out,
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            Print(line),
            Print("\r\n"),
            Print(PROMPT),
            Print(buf)
        );
        let _ = out.flush();
    } else {
        println!("{line}");
    }
}

/// Rend le terminal à son état normal.
pub fn stop() {
    if ACTIVE.swap(false, Ordering::Relaxed) {
        let _ = terminal::disable_raw_mode();
        println!();
    }
}

fn redraw() {
    let buf = BUFFER.lock().map(|b| b.clone()).unwrap_or_default();
    let mut out = std::io::stdout().lock();
    let _ = queue!(out, MoveToColumn(0), Clear(ClearType::CurrentLine), Print(PROMPT), Print(buf));
    let _ = out.flush();
}

/// Démarre la lecture des commandes. Renvoie le canal des lignes saisies.
pub fn start() -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if interactive && terminal::enable_raw_mode().is_ok() {
        ACTIVE.store(true, Ordering::Relaxed);
        let _ = std::thread::Builder::new().name("boomground-console".into()).spawn(move || {
            let mut history: Vec<String> = Vec::new();
            let mut cursor: Option<usize> = None;
            redraw();
            loop {
                let Ok(Event::Key(key)) = event::read() else { continue };
                if key.kind == KeyEventKind::Release {
                    continue;
                }
                let mut buf = BUFFER.lock().unwrap();
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        drop(buf);
                        let _ = tx.send("stop".into());
                        continue;
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => buf.clear(),
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => buf.push(c),
                    KeyCode::Backspace => {
                        buf.pop();
                    }
                    KeyCode::Enter => {
                        let line = std::mem::take(&mut *buf);
                        drop(buf);
                        cursor = None;
                        if !line.trim().is_empty() {
                            history.push(line.clone());
                            // La commande saisie reste visible dans le journal.
                            print(&format!("{PROMPT}{line}"));
                            let _ = tx.send(line);
                        } else {
                            redraw();
                        }
                        continue;
                    }
                    KeyCode::Up if !history.is_empty() => {
                        let i = cursor.map_or(history.len() - 1, |c| c.saturating_sub(1));
                        cursor = Some(i);
                        *buf = history[i].clone();
                    }
                    KeyCode::Down => {
                        match cursor {
                            Some(c) if c + 1 < history.len() => {
                                cursor = Some(c + 1);
                                *buf = history[c + 1].clone();
                            }
                            _ => {
                                cursor = None;
                                buf.clear();
                            }
                        }
                    }
                    _ => {}
                }
                drop(buf);
                redraw();
            }
        });
    } else {
        let _ = std::thread::Builder::new().name("boomground-console".into()).spawn(move || {
            let mut line = String::new();
            loop {
                line.clear();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if tx.send(line.trim().to_string()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
    rx
}
