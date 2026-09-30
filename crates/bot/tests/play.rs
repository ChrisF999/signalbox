//! `NetPlayer::decide` keeps the strategy to twice a real second for every
//! caller (C2 decision 12), against a bare WebSocket server that counts what
//! it is sent.

use std::collections::BTreeMap;
use std::time::Duration;

use bot::net::Conn;
use bot::play::{MIN_DECIDE_EVERY, NetPlayer};
use futures_util::StreamExt;
use protocol::*;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

fn s(x: &str) -> String {
    x.to_string()
}

#[tokio::test]
async fn decide_runs_at_most_twice_a_second_whoever_calls_it() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        while let Some(Ok(m)) = ws.next().await {
            if m.is_text() {
                tx.send(()).unwrap();
            }
        }
    });

    let names = ["S1", "S2", "S3"];
    let layout = Layout {
        title: s("t"),
        you: s("ann"),
        area: Some(s("A")),
        areas: vec![s("A")],
        sections: vec![],
        segments: vec![],
        signals: vec![],
        points: vec![],
        berths: names
            .iter()
            .enumerate()
            .map(|(i, n)| BerthInfo { name: format!("B{i}"), signal: Some(s(n)), boundary: None, area: s("A"), operable: true })
            .collect(),
        platforms: vec![],
        routes: names
            .iter()
            .map(|n| RouteInfo { name: format!("{n}-X"), entrance: s(n), exit: ExitName::Signal(s("X")), automatic: false, operable: true })
            .collect(),
        geometry: None,
    };
    let view = View {
        seq: 1,
        sim_time: 0.0,
        speed: 1,
        paused: false,
        vote: None,
        holders: BTreeMap::new(),
        score: Some(0),
        signals: names.iter().map(|n| (s(n), Aspect::Red)).collect(),
        routes: BTreeMap::new(),
        points: BTreeMap::new(),
        sections: BTreeMap::new(),
        berths: (0..3).map(|i| (format!("B{i}"), format!("1A0{i}"))).collect(),
        trains: BTreeMap::new(),
    };

    let mut p = NetPlayer::new("ann", Conn::connect(&base, None).await.unwrap());
    p.bot.receive(ServerMsg::Layout(layout));
    p.bot.receive(ServerMsg::View(view));

    assert_eq!(p.decide().await.unwrap(), 2, "two commands at most");
    assert_eq!(p.decide().await.unwrap(), 0, "a second call at once does nothing, though S3 is still waiting");
    tokio::time::sleep(MIN_DECIDE_EVERY + Duration::from_millis(50)).await;
    assert_eq!(p.decide().await.unwrap(), 1, "after the interval it decides again");
    assert_eq!(p.commands_sent, 3);
    for _ in 0..3 {
        tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap().unwrap();
    }
    assert!(rx.try_recv().is_err(), "nothing else was sent");
}
