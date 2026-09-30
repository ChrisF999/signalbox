//! A bot playing over the network: one `Conn`, a `Bot` keeping the layout
//! and view, and `Greedy` deciding what to send.

use std::time::Duration;

use protocol::{ClientFrame, ClientMsg, LobbyMsg, LobbyReply, ServerFrame, ServerMsg, View};
use tokio::time::{Instant, MissedTickBehavior, interval, timeout_at};

use crate::Bot;
use crate::net::{Conn, NetError, dev_login};
use crate::strategy::Greedy;

/// `Greedy::decide` runs at most twice per real second (C2 decision 12); this
/// also keeps the send rate far under the front's per-socket limit.
pub const MIN_DECIDE_EVERY: Duration = Duration::from_millis(500);

pub struct NetPlayer {
    pub name: String,
    pub conn: Conn,
    pub bot: Bot,
    pub greedy: Greedy,
    /// The game this socket is in, from the last `joined`.
    pub game: Option<String>,
    /// Lobby replies other than `joined`, oldest first.
    pub lobby: Vec<LobbyReply>,
    /// Commands the strategy sent.
    pub commands_sent: usize,
    /// When `decide` last ran for real (see `MIN_DECIDE_EVERY`).
    last_decided: Option<Instant>,
}

impl NetPlayer {
    /// Log in on a dev-auth front and open a socket.
    pub async fn login(base: &str, name: &str) -> Result<NetPlayer, NetError> {
        let cookie = dev_login(base, name).await?;
        let conn = Conn::connect(base, Some(&cookie)).await?;
        Ok(NetPlayer::new(name, conn))
    }

    pub fn new(name: &str, conn: Conn) -> NetPlayer {
        NetPlayer {
            name: name.to_string(),
            conn,
            bot: Bot::new(),
            greedy: Greedy::new(),
            game: None,
            lobby: Vec::new(),
            commands_sent: 0,
            last_decided: None,
        }
    }

    pub async fn lobby_msg(&mut self, msg: LobbyMsg) -> Result<(), NetError> {
        self.conn.send(&ClientFrame::Lobby(msg)).await
    }

    pub async fn game_msg(&mut self, msg: ClientMsg) -> Result<(), NetError> {
        self.conn.send(&ClientFrame::Game(msg)).await
    }

    /// Take one frame: game messages go to the `Bot` (a resync it asks for
    /// is sent at once); `joined` records the game; other lobby replies are
    /// kept in `lobby`.
    pub async fn take(&mut self, f: ServerFrame) -> Result<(), NetError> {
        match f {
            ServerFrame::Game(msg) => {
                if let Some(reply) = self.bot.receive(msg) {
                    self.game_msg(reply).await?;
                }
            }
            ServerFrame::Lobby(LobbyReply::Joined { game, .. }) => self.game = Some(game),
            ServerFrame::Lobby(other) => self.lobby.push(other),
        }
        Ok(())
    }

    /// Take frames until one matches `stop` (it is taken too); returns a
    /// copy of it. Fails if the socket closes or `limit` passes first.
    pub async fn until(&mut self, limit: Duration, stop: impl Fn(&ServerFrame) -> bool) -> Result<ServerFrame, NetError> {
        let deadline = Instant::now() + limit;
        loop {
            let f = match timeout_at(deadline, self.conn.recv()).await {
                Ok(r) => r?.ok_or_else(|| NetError::Http(format!("{}: the server closed the socket", self.name)))?,
                Err(_) => return Err(NetError::Timeout(format!("{} waited {limit:?}", self.name))),
            };
            let hit = stop(&f);
            let copy = hit.then(|| f.clone());
            self.take(f).await?;
            if let Some(f) = copy {
                return Ok(f);
            }
        }
    }

    /// Take frames until the bot's view satisfies `ok`. Fails if the
    /// socket closes or `limit` passes first.
    pub async fn until_view(&mut self, limit: Duration, ok: impl Fn(&View) -> bool) -> Result<(), NetError> {
        let deadline = Instant::now() + limit;
        while !self.bot.view().is_some_and(&ok) {
            let f = match timeout_at(deadline, self.conn.recv()).await {
                Ok(r) => r?.ok_or_else(|| NetError::Http(format!("{}: the server closed the socket", self.name)))?,
                Err(_) => return Err(NetError::Timeout(format!("{} waited {limit:?} for its view", self.name))),
            };
            self.take(f).await?;
        }
        Ok(())
    }

    /// Send what the strategy decides for the current view. Within
    /// `MIN_DECIDE_EVERY` of the last decision it does nothing and returns 0,
    /// so the strategy runs at most twice a real second for every caller.
    pub async fn decide(&mut self) -> Result<usize, NetError> {
        if self.last_decided.is_some_and(|t| t.elapsed() < MIN_DECIDE_EVERY) {
            return Ok(0);
        }
        self.last_decided = Some(Instant::now());
        let (Some(layout), Some(view)) = (self.bot.layout(), self.bot.view()) else { return Ok(0) };
        let cmds = self.greedy.decide(layout, view);
        let n = cmds.len();
        for cmd in cmds {
            self.game_msg(ClientMsg::Command { cmd }).await?;
        }
        self.commands_sent += n;
        Ok(n)
    }

    /// Play until the view's sim time reaches `until_s`: take every frame
    /// and decide every `every` of real time (never more than twice a second). Fails after `limit`.
    pub async fn play_until(&mut self, until_s: f64, every: Duration, limit: Duration) -> Result<(), NetError> {
        let deadline = Instant::now() + limit;
        let mut tick = interval(every.max(MIN_DECIDE_EVERY));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        while !self.bot.view().is_some_and(|v| v.sim_time >= until_s) {
            tokio::select! {
                r = timeout_at(deadline, self.conn.recv()) => {
                    let f = r
                        .map_err(|_| NetError::Timeout(format!("{} did not reach {until_s} s within {limit:?}", self.name)))??
                        .ok_or_else(|| NetError::Http(format!("{}: the server closed the socket", self.name)))?;
                    self.take(f).await?;
                }
                _ = tick.tick() => {
                    self.decide().await?;
                }
            }
        }
        Ok(())
    }

    /// Ask for a full view and wait for it; returns the view the bot had
    /// built from deltas just before, and the fresh one.
    pub async fn resync_and_compare(&mut self, limit: Duration) -> Result<(Option<View>, Option<View>), NetError> {
        let built = self.bot.view().cloned();
        self.game_msg(ClientMsg::Resync).await?;
        self.until(limit, |f| matches!(f, ServerFrame::Game(ServerMsg::View(_)))).await?;
        Ok((built, self.bot.view().cloned()))
    }
}
