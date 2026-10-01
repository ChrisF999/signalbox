//! The app's side of the controls (spec D1 §3.2): clicks, Esc, menus,
//! interposing and hover text, on top of `select`.

use protocol::ExitName;

use crate::app::App;
use crate::select::{self, Click, MenuItem};
use crate::simplifier;

/// Something on the diagram under the pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Signal(String),
    Points(String),
    Berth(String),
    /// A buffer stop or boundary node a route can end at.
    Exit(String),
    Section(String),
    /// The ○A auto-working button beside this signal.
    Auto(String),
}

impl App {
    /// A left click. Signals and exits select an entrance or set a route;
    /// anything else is a dead click and clears the entrance (as a click on
    /// nothing does: the screen sends that as `escape`).
    pub fn click(&mut self, target: &Target) {
        let exit = match target {
            Target::Signal(s) => ExitName::Signal(s.clone()),
            Target::Exit(n) => ExitName::Node(n.clone()),
            Target::Auto(s) => return self.toggle_auto(s),
            Target::Points(_) | Target::Berth(_) | Target::Section(_) => return self.escape(),
        };
        let Some(g) = self.game.as_mut() else { return };
        let Some(l) = g.bot.layout() else { return };
        match select::click(l, g.selected.as_deref(), &exit) {
            Click::Select(s) => g.selected = Some(s),
            Click::Clear => g.selected = None,
            Click::Send(cmd) => {
                g.selected = None;
                self.command(cmd);
            }
            Click::Ignore => {}
        }
    }

    /// The ○A button: the signal menu's auto-working command, if it offers
    /// one; the selection is left as it is.
    fn toggle_auto(&mut self, signal: &str) {
        let cmd = self.game.as_ref().and_then(|g| select::auto_toggle(g.bot.layout()?, g.bot.view()?, signal));
        if let Some(cmd) = cmd {
            self.command(cmd);
        }
    }

    /// Esc: forget the chosen entrance.
    pub fn escape(&mut self) {
        if let Some(g) = self.game.as_mut() {
            g.selected = None;
        }
    }

    /// The exits to light up for the chosen entrance.
    pub fn valid_exits(&self) -> Vec<ExitName> {
        let Some(g) = self.game.as_ref() else { return vec![] };
        match (g.bot.layout(), g.selected.as_deref()) {
            (Some(l), Some(e)) => select::exits_from(l, e),
            _ => vec![],
        }
    }

    /// The right-click menu for `target` (empty: no menu).
    pub fn menu(&self, target: &Target) -> Vec<MenuItem> {
        let Some(g) = self.game.as_ref() else { return vec![] };
        let (Some(l), Some(v)) = (g.bot.layout(), g.bot.view()) else { return vec![] };
        match target {
            Target::Signal(s) => select::signal_menu(l, v, s),
            Target::Points(p) => select::points_menu(l, v, p),
            Target::Berth(b) => select::berth_menu(l, v, b),
            Target::Exit(_) | Target::Section(_) | Target::Auto(_) => vec![],
        }
    }

    /// Whether the berth menu should offer a headcode box.
    pub fn can_interpose(&self, berth: &str) -> bool {
        self.game.as_ref().and_then(|g| g.bot.layout()).is_some_and(|l| select::operable_berth(l, berth))
    }

    /// Interpose a typed headcode; blank input sends nothing.
    pub fn interpose(&mut self, berth: &str, typed: &str) {
        if !self.can_interpose(berth) {
            return;
        }
        if let Some(cmd) = select::interpose(berth, typed) {
            self.command(cmd);
        }
    }

    /// The headcode shown at `target`, if it is a berth holding one: what a
    /// click opens the enquiry for (realism spec §3), as `simplifier::resolve`
    /// reads it. Never routes.
    pub fn headcode_at(&self, target: &Target) -> Option<String> {
        let Target::Berth(b) = target else { return None };
        let bot = &self.game.as_ref()?.bot;
        let (l, v) = (bot.layout()?, bot.view()?);
        Some(simplifier::resolve(l, Some(v), v.berths.get(b)?))
    }

    /// Hover text.
    pub fn describe(&self, target: &Target) -> String {
        let Some(g) = self.game.as_ref() else { return String::new() };
        let (Some(l), Some(v)) = (g.bot.layout(), g.bot.view()) else { return String::new() };
        match target {
            Target::Signal(s) => select::describe_signal(l, v, s),
            Target::Points(p) => select::describe_points(l, v, p),
            Target::Berth(b) => select::describe_berth(l, v, b),
            Target::Exit(n) => format!("Exit {n}"),
            Target::Section(s) => select::describe_section(l, v, s),
            Target::Auto(s) => select::describe_auto(l, v, s),
        }
    }
}
