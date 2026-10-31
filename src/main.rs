//! fx: the views and effects of the desktop, each with a tick box.
//!
//! A view (from the `views` tool) dresses strip, herald, launch and tile
//! at once, and only one is on at a time. An effect is one trick, such
//! as frame's beam-in or astro's living wallpaper. Space or Enter switches the one under the cursor,
//! and it takes effect at once.
//!
//! Run from a key binding, with no terminal around it, fx opens its own
//! glass window; pressed again while open, it closes it.

use crust::{seq, style, Crust, Cursor, Input};
use std::fs;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

type Rgb = (u8, u8, u8);

/// The beam-in time fx writes when it turns the beam on, in ms.
const BEAM_MS: u32 = 600;

/// The colours of the window: launch's, from ~/.launchrc, so a view
/// that dresses launch dresses fx too.
#[derive(Clone, Copy)]
struct Look {
    accent: Rgb,
    text: Rgb,
    dim: Rgb,
    bar_bg: Rgb,
    bar_title: Rgb,
    bar_text: Rgb,
    foot_bg: Rgb,
    foot_text: Rgb,
    pick_fg: Rgb,
    pick_bg: Rgb,
    side: Option<Rgb>,
}

const PLAIN: Look = Look {
    accent: (247, 76, 0),
    text: (225, 225, 230),
    dim: (140, 140, 150),
    bar_bg: (38, 38, 38),
    bar_title: (247, 76, 0),
    bar_text: (140, 140, 150),
    foot_bg: (38, 38, 38),
    foot_text: (200, 200, 205),
    pick_fg: (225, 225, 230),
    pick_bg: (52, 48, 60),
    side: None,
};

impl Look {
    fn load(home: &Path) -> Look {
        let mut k = PLAIN;
        let text = fs::read_to_string(home.join(".launchrc")).unwrap_or_default();
        for line in text.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
            let Some((name, v)) = line.split_once('=') else { continue };
            let Some(c) = hex(v.trim()) else { continue };
            match name.trim() {
                "accent" => k.accent = c,
                "text" => k.text = c,
                "dim" => k.dim = c,
                "bar_bg" => k.bar_bg = c,
                "bar_title" => k.bar_title = c,
                "bar_text" => k.bar_text = c,
                "foot_bg" => k.foot_bg = c,
                "foot_text" => k.foot_text = c,
                "pick_fg" => k.pick_fg = c,
                "pick_bg" => k.pick_bg = c,
                "side" => k.side = Some(c),
                _ => {}
            }
        }
        k
    }
}

/// `#rrggbb` as a colour.
fn hex(v: &str) -> Option<Rgb> {
    let n = u32::from_str_radix(v.strip_prefix('#')?.get(..6)?, 16).ok()?;
    Some(((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

enum Kind {
    View(String),
    Beam,
    Wall,
    RedAlert,
}

struct Item {
    name: String,
    about: String,
    on: bool,
    kind: Kind,
}

struct Fx {
    items: Vec<Item>,
    sel: usize,
    look: Look,
    home: PathBuf,
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("-v" | "--version") => {
            println!("fx {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("-h" | "--help") => {
            println!("fx — the views and effects of the desktop, each with a tick box");
            println!();
            println!("  fx          the list (opens its own window from a key binding)");
            println!();
            println!("  arrows      move");
            println!("  Space Enter switch the one under the cursor");
            println!("  Esc         close");
            println!();
            println!("  Views come from the views tool; one is on at a time.");
            println!("  Beam-in is frame's `beam` key in ~/.framerc ({BEAM_MS} ms when on).");
            println!("  Living wallpaper points frame's `background` at astro's picture,");
            println!("  and starts `astro --wall`, which draws it again every hour.");
            println!("  Red alert is the file ~/.drain/red-alert: `drain --tally` sees the");
            println!("  battery fall to 10 % and 5 %, and herald shows a pulsing red alert.");
            return;
        }
        _ => {}
    }
    // SAFETY: isatty only looks at the descriptor.
    let on_tty = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if !on_tty {
        toggle_window();
        return;
    }

    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into());
    let mut items = views();
    if frame_pid().is_some() {
        let rc = fs::read_to_string(home.join(".framerc")).unwrap_or_default();
        items.push(Item {
            name: "Beam-in".into(),
            about: "a new window arrives in sparkles, like a transporter".into(),
            on: beam_ms(&rc) > 0,
            kind: Kind::Beam,
        });
        if on_path("astro") {
            items.push(Item {
                name: "Living wallpaper".into(),
                about: "the sky, the Moon and the Yr forecast, drawn again every hour".into(),
                on: frame_key(&rc, "background").as_deref() == Some(wall_file(&home).to_str().unwrap_or("")),
                kind: Kind::Wall,
            });
        }
    }
    if on_path("drain") {
        items.push(Item {
            name: "Red alert".into(),
            about: "a pulsing red alert when the battery falls to 10 % and 5 %".into(),
            on: home.join(".drain/red-alert").exists(),
            kind: Kind::RedAlert,
        });
    }
    let mut fx = Fx { items, sel: 0, look: Look::load(&home), home };

    let pid_file = runtime_dir().join("fx.pid");
    let _ = fs::write(&pid_file, std::process::id().to_string());
    Crust::init();
    Crust::set_app_identity("fx");
    Cursor::hide();
    fx.run();
    Crust::cleanup();
    let _ = fs::remove_file(&pid_file);
}

impl Fx {
    /// Keys until Esc. Nothing runs between keys, except one wait for a
    /// view to write its colours, so the window can wear them.
    fn run(&mut self) {
        let mut relook = false;
        loop {
            self.draw();
            let wait = if relook { 400 } else { 600_000 };
            let Some(key) = Input::getchr_ms(wait) else {
                if relook {
                    self.look = Look::load(&self.home);
                    relook = false;
                }
                continue;
            };
            match key.as_str() {
                "ESC" | "q" => return,
                "DOWN" | "TAB" => {
                    if self.sel + 1 < self.items.len() {
                        self.sel += 1;
                    }
                }
                "UP" | "S-TAB" => self.sel = self.sel.saturating_sub(1),
                " " | "ENTER" => relook = self.switch(),
                _ => {}
            }
        }
    }

    /// Switch the item under the cursor. True when it was a view.
    fn switch(&mut self) -> bool {
        let Some(item) = self.items.get(self.sel) else { return false };
        let on = !item.on;
        match &item.kind {
            Kind::View(name) => {
                detached(&["views", if on { name } else { "off" }]);
                for it in self.items.iter_mut().filter(|it| matches!(it.kind, Kind::View(_))) {
                    it.on = false;
                }
                self.items[self.sel].on = on;
                true
            }
            Kind::Beam => {
                let path = self.home.join(".framerc");
                let rc = fs::read_to_string(&path).unwrap_or_default();
                if fs::write(&path, set_key(&rc, "beam", &(if on { BEAM_MS } else { 0 }).to_string())).is_ok() {
                    if let Some(pid) = frame_pid() {
                        // SAFETY: a plain signal; frame re-reads ~/.framerc on it.
                        unsafe { libc::kill(pid, libc::SIGUSR2) };
                    }
                    self.items[self.sel].on = on;
                }
                false
            }
            Kind::Wall => {
                // On: frame's background is astro's picture, and astro draws
                // it, tells frame, and draws it again each hour. Off: back to
                // chasm-bg's picture, and astro, woken, sees that and ends.
                let path = self.home.join(".framerc");
                let rc = fs::read_to_string(&path).unwrap_or_default();
                let file = if on { wall_file(&self.home) } else { self.home.join(".framebg") };
                if fs::write(&path, set_key(&rc, "background", &file.to_string_lossy())).is_err() {
                    return false;
                }
                if on {
                    detached(&["astro", "--wall"]);
                } else {
                    if let Some(pid) = frame_pid() {
                        // SAFETY: a plain signal; frame re-reads ~/.framerc on it.
                        unsafe { libc::kill(pid, libc::SIGUSR2) };
                    }
                    let drawer = runtime_dir().join("astro-wall.pid");
                    if let Some(pid) = fs::read_to_string(drawer).ok().and_then(|s| s.trim().parse::<i32>().ok()) {
                        if fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "astro") {
                            // SAFETY: a plain signal to a process we just checked is astro.
                            unsafe { libc::kill(pid, libc::SIGUSR1) };
                        }
                    }
                }
                self.items[self.sel].on = on;
                false
            }
            Kind::RedAlert => {
                // drain reads the file only when the battery reaches a mark.
                // Starting the tally is free when it already runs: the
                // second one finds the first and ends.
                let flag = self.home.join(".drain/red-alert");
                let done = if on {
                    let _ = fs::create_dir_all(self.home.join(".drain"));
                    detached(&["drain", "--tally"]);
                    fs::write(&flag, "").is_ok()
                } else {
                    fs::remove_file(&flag).is_ok()
                };
                if done {
                    self.items[self.sel].on = on;
                }
                false
            }
        }
    }

    fn draw(&self) {
        let (cols, rows) = Crust::terminal_size();
        let (w, rows) = (cols as usize, rows as usize);
        let look = self.look;
        let mut out = String::new();
        for y in 1..=rows {
            out.push_str(&format!("{}{}", Cursor::at(1, y as u16), seq::ERASE_EOL));
        }
        if let Some(c) = look.side {
            for y in 2..rows {
                let edge = if y == 2 { "  " } else { " " };
                out.push_str(&format!("{}{}", Cursor::at(1, y as u16), style::rgb(edge, None, Some(c), "")));
            }
        }
        let tail = "   views and effects";
        out.push_str(&format!(
            "{}{}{}{}",
            Cursor::at(1, 1),
            style::rgb(" ", None, Some(look.bar_bg), ""),
            style::rgb("fx", Some(look.bar_title), Some(look.bar_bg), "b"),
            style::rgb(&format!("{tail}{}", " ".repeat(w.saturating_sub(3 + tail.len()))), Some(look.bar_text), Some(look.bar_bg), "")
        ));

        let nw = self.items.iter().map(|i| crust::display_width(&i.name)).max().unwrap_or(0);
        let mut y = 3;
        let mut heading = "";
        if self.items.is_empty() {
            let say = "nothing to switch: no views tool on the PATH, and frame is not running";
            out.push_str(&format!("{}{}", Cursor::at(5, y as u16), style::rgb(say, Some(look.dim), None, "i")));
        }
        for (i, item) in self.items.iter().enumerate() {
            let group = if matches!(item.kind, Kind::View(_)) { "Views" } else { "Effects" };
            if group != heading {
                if !heading.is_empty() {
                    y += 1;
                }
                heading = group;
                out.push_str(&format!("{}{}", Cursor::at(4, y as u16), style::rgb(group, Some(look.accent), None, "b")));
                y += 1;
            }
            let tick = if item.on { "[✓]" } else { "[ ]" };
            let name = format!("{:<nw$}", item.name);
            let row = if i == self.sel {
                style::rgb(&format!(" {tick} {name} "), Some(look.pick_fg), Some(look.pick_bg), "b")
            } else {
                format!(" {} {} ", style::rgb(tick, Some(look.accent), None, ""), style::rgb(&name, Some(look.text), None, ""))
            };
            out.push_str(&format!("{}{} {}", Cursor::at(5, y as u16), row, style::rgb(&item.about, Some(look.dim), None, "")));
            y += 1;
        }

        let (lead, fw) = match look.side {
            Some(c) => (format!("{} ", style::rgb(" ", None, Some(c), "")), w.saturating_sub(2)),
            None => (String::new(), w),
        };
        let foot = " arrows move · Space switch · Esc close";
        let version = format!("v{} ", env!("CARGO_PKG_VERSION"));
        let pad = fw.saturating_sub(crust::display_width(foot) + version.len());
        out.push_str(&format!(
            "{}{lead}{}",
            Cursor::at(1, rows as u16),
            style::rgb(&format!("{foot}{}{version}", " ".repeat(pad)), Some(look.foot_text), Some(look.foot_bg), "")
        ));
        print!("{out}");
        std::io::stdout().flush().ok();
    }
}

/// The views from the views tool: `* name  Title: about` per line, the
/// one in use marked with a star.
fn views() -> Vec<Item> {
    let Ok(out) = Command::new("views").stdin(Stdio::null()).stderr(Stdio::null()).output() else { return Vec::new() };
    parse_views(&String::from_utf8_lossy(&out.stdout))
}

fn parse_views(text: &str) -> Vec<Item> {
    text.lines()
        .filter_map(|l| {
            let rest = l.get(2..)?.trim();
            let (id, about) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if id.is_empty() {
                return None;
            }
            // "LCARS: the screens of Star Trek." names the view LCARS.
            let about = about.trim();
            let (name, about) = match about.split_once(": ") {
                Some((n, a)) if n.eq_ignore_ascii_case(id) => (n.to_string(), a.to_string()),
                _ => (id.to_string(), about.to_string()),
            };
            Some(Item { name, about, on: l.starts_with('*'), kind: Kind::View(id.to_string()) })
        })
        .collect()
}

/// The value of `key` in ~/.framerc; the last line counts, as in frame.
fn frame_key(rc: &str, key: &str) -> Option<String> {
    rc.lines()
        .filter_map(|l| {
            let (k, v) = l.split('#').next()?.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
        .last()
}

/// frame's `beam` in ms, 0 when off or missing.
fn beam_ms(rc: &str) -> u32 {
    frame_key(rc, "beam").and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Where `astro --wall` keeps its picture.
fn wall_file(home: &Path) -> PathBuf {
    home.join(".astro/wall.bgrx")
}

/// Whether a program by this name is on the PATH.
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// ~/.framerc with every `key` line set to `value`, comments kept; a line
/// is added when there is none.
fn set_key(rc: &str, key: &str, value: &str) -> String {
    let mut out = String::new();
    let mut found = false;
    for line in rc.lines() {
        let k = line.split('#').next().unwrap_or("").split_once('=').map(|(k, _)| k.trim());
        if k == Some(key) {
            found = true;
            let value = format!("{key} = {value}");
            match line.find('#') {
                // The comment keeps its column, a space after a long value.
                Some(i) => out.push_str(&format!("{value:<w$}{}", &line[i..], w = 27.max(value.len() + 1))),
                None => out.push_str(&value),
            }
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !found {
        out.push_str(&format!("{key} = {value}\n"));
    }
    out
}

/// The frame serving this DISPLAY: the one whose first argument is its
/// number (`frame 8 --display`).
fn frame_pid() -> Option<i32> {
    let display = std::env::var("DISPLAY").ok()?;
    let num = display.rsplit(':').next()?.split('.').next()?.to_string();
    fs::read_dir("/proc").ok()?.flatten().find_map(|e| {
        let pid: i32 = e.file_name().to_str()?.parse().ok()?;
        if fs::read_to_string(e.path().join("comm")).ok()?.trim() != "frame" {
            return None;
        }
        let cmd = fs::read(e.path().join("cmdline")).ok()?;
        (cmd.split(|&b| b == 0).nth(1)? == num.as_bytes()).then_some(pid)
    })
}

/// Start a program on its own, and reap it when it ends.
fn detached(args: &[&str]) {
    let mut c = Command::new(args[0]);
    c.args(&args[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe, as pre_exec requires.
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    if let Ok(mut child) = c.spawn() {
        std::thread::spawn(move || child.wait());
    }
}

/// From a key binding: close the open fx window if there is one, or open
/// a new one.
fn toggle_window() {
    let pid_file = runtime_dir().join("fx.pid");
    if let Some(pid) = fs::read_to_string(&pid_file).ok().and_then(|s| s.trim().parse::<i32>().ok()) {
        if fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "fx") {
            // SAFETY: a plain signal to a process we just checked is fx.
            unsafe { libc::kill(pid, libc::SIGTERM) };
            let _ = fs::remove_file(&pid_file);
            return;
        }
    }
    let me = std::env::current_exe().unwrap_or_else(|_| "fx".into());
    let err = Command::new("glass").args(["--class", "Fx", "--"]).arg(me).exec();
    eprintln!("fx: cannot start glass: {err}");
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| "/tmp".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_are_named_from_their_first_line() {
        let v = parse_views("* lcars      LCARS: the screens of Star Trek.\n  plain      a quiet look\n");
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].name.as_str(), v[0].about.as_str(), v[0].on), ("LCARS", "the screens of Star Trek.", true));
        assert_eq!((v[1].name.as_str(), v[1].about.as_str(), v[1].on), ("plain", "a quiet look", false));
    }

    #[test]
    fn beam_reads_the_last_line() {
        assert_eq!(beam_ms("keymap = no\n"), 0);
        assert_eq!(beam_ms("beam = 300\nbeam = 600   # ms\n"), 600);
        assert_eq!(beam_ms("# beam = 600\n"), 0);
    }

    #[test]
    fn set_key_keeps_comments_and_adds_a_missing_line() {
        assert_eq!(set_key("keymap = no\nbeam = 600  # ms (0 = off)\n", "beam", "0"), "keymap = no\nbeam = 0                   # ms (0 = off)\n");
        assert_eq!(set_key("keymap = no\n", "beam", "600"), "keymap = no\nbeam = 600\n");
        let rc = "background = /h/.framebg\nbeam = 0\n";
        let on = set_key(rc, "background", "/h/.astro/wall.bgrx");
        assert_eq!(frame_key(&on, "background").as_deref(), Some("/h/.astro/wall.bgrx"));
        let long = set_key("background = /h/.framebg  # the picture\n", "background", "/home/someone/.astro/wall.bgrx");
        assert_eq!(long, "background = /home/someone/.astro/wall.bgrx # the picture\n", "a space before the comment");
        assert_eq!(beam_ms(&on), 0, "the other keys stay");
    }
}
