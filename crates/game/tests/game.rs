//! The game in process: claims, area checks, the robot, votes, grace,
//! notices, and views that clients rebuild from deltas.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use game::areas::AreaMap;
use game::game::{GRACE_S, MAX_TICKS_PER_ADVANCE};
use game::{Out, ROBOT};
use protocol::*;

fn s(x: &str) -> String {
    x.to_string()
}

fn claim(g: &mut game::Game, player: &str, area: &str) -> Vec<Out> {
    g.handle(player, ClientMsg::Claim { area: s(area) })
}

#[test]
fn connecting_sends_the_whole_layout_and_a_first_view() {
    let mut g = game();
    let out = g.connect("alice");
    assert_eq!(out.len(), 2, "{out:?}");
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!((l.you.as_str(), l.area.as_deref(), l.sections.len(), l.signals.len()), ("alice", None, 5, 5));
    assert!(l.signals.iter().all(|x| !x.operable));
    let ServerMsg::View(v) = &out[1].1 else { panic!("{out:?}") };
    assert_eq!(v.seq, 1);
    assert_eq!(v.holders, map(&[("East", s(ROBOT)), ("West", s(ROBOT))]));
    assert_eq!((v.speed, v.paused, v.score), (1, false, None));
}

#[test]
fn robot_is_a_reserved_name() {
    let mut g = game();
    let out = g.connect(ROBOT);
    assert_eq!(error_codes(&out, ROBOT), [codes::RESERVED_NAME]);
    assert!(g.handle(ROBOT, ClientMsg::Claim { area: s("West") }).is_empty());
    assert_eq!(g.holder("West"), None);
}

#[test]
fn reconnecting_while_connected_just_resyncs() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    let out = g.connect("alice");
    let ServerMsg::View(v) = &out[1].1 else { panic!("{out:?}") };
    assert_eq!(v.seq, 3);
    assert_eq!(g.holder("West"), Some("alice"));
}

#[test]
fn claiming_an_area_resyncs_with_its_layout_and_fringe() {
    let mut g = game();
    g.connect("alice");
    let out = claim(&mut g, "alice", "West");
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!(l.area.as_deref(), Some("West"));
    let sections: Vec<(&str, bool)> = l.sections.iter().map(|x| (x.name.as_str(), x.fringe)).collect();
    assert_eq!(sections, [("TW1", false), ("TW2", false), ("TP", true)]);
    let ServerMsg::View(v) = &out[1].1 else { panic!("{out:?}") };
    assert_eq!((v.seq, v.score), (2, Some(0)));
    assert_eq!(v.holders["West"], "alice");
    assert_eq!((g.holder("West"), g.area_of("alice")), (Some("alice"), Some("West")));
}

#[test]
fn a_held_area_cannot_be_claimed_and_unknown_areas_are_errors() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    g.connect("bob");
    let out = claim(&mut g, "bob", "West");
    assert_eq!(notices(&out, "bob"), vec![Notice::AreaTaken { area: s("West"), holder: s("alice") }]);
    assert_eq!(error_codes(&claim(&mut g, "bob", "North"), "bob"), [codes::UNKNOWN_AREA]);
    assert_eq!(g.holder("West"), Some("alice"));
    assert_eq!(g.area_of("bob"), None);
}

#[test]
fn claiming_another_area_moves_you() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    let out = claim(&mut g, "alice", "East");
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!(l.area.as_deref(), Some("East"));
    assert_eq!((g.holder("West"), g.holder("East")), (None, Some("alice")));
}

#[test]
fn releasing_gives_the_area_back_to_the_robot() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    let out = send(&mut g, "alice", ClientMsg::Release);
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!(l.area, None);
    assert_eq!(g.holder("West"), None);
    assert_eq!(error_codes(&send(&mut g, "alice", ClientMsg::Release), "alice"), [codes::NOT_HOLDING]);
}

#[test]
fn commands_outside_your_area_never_reach_the_sim() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "sam", None);
    let east = vec![
        set_route("C", ExitName::Signal(s("W2"))),
        PlayerCommand::CancelRoute { entrance: s("C") },
        PlayerCommand::SetAutoWorking { entrance: s("D"), on: true },
        PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse },
        PlayerCommand::Interpose { berth: s("BC"), headcode: s("1A01") },
        PlayerCommand::CancelBerth { berth: s("BE") },
    ];
    for cmd in east {
        for p in ["alice", "sam"] {
            let out = command(&mut g, p, cmd.clone());
            assert_eq!(notices(&out, p), vec![Notice::NotYourArea { area: s("East") }], "{p} {cmd:?}");
        }
    }
    g.advance(1.0);
    assert!(g.sim().log().is_empty(), "{:?}", g.sim().log());

    let west = vec![
        set_route("W1", ExitName::Signal(s("A"))),
        PlayerCommand::SetAutoWorking { entrance: s("W1"), on: true },
        PlayerCommand::CancelRoute { entrance: s("W1") },
        PlayerCommand::Interpose { berth: s("BA"), headcode: s("1A01") },
        PlayerCommand::CancelBerth { berth: s("BA") },
    ];
    for cmd in west {
        assert!(command(&mut g, "alice", cmd).is_empty());
    }
    let out = g.advance(0.1);
    assert_eq!(g.sim().log().len(), 5);
    assert!(notices(&out, "alice").is_empty(), "{out:?}");
    assert_eq!(g.stats().player_commands, 5);

    join(&mut g, "bob", Some("East"));
    assert!(command(&mut g, "bob", PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse }).is_empty());
    g.advance(0.1);
    assert_eq!(g.sim().log().len(), 6);
}

#[test]
fn unknown_names_and_non_points_are_rejected_before_the_sim() {
    let mut g = game();
    join(&mut g, "alice", Some("East"));
    let bad = PlayerCommand::CancelRoute { entrance: s("Z9") };
    let out = command(&mut g, "alice", bad.clone());
    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: bad, reason: Rejection::UnknownId }]);
    let joint = PlayerCommand::SwingPoints { points: s("J2"), to: PointsPos::Reverse };
    let out = command(&mut g, "alice", joint.clone());
    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: joint, reason: Rejection::NotPoints }]);
    g.advance(0.1);
    assert!(g.sim().log().is_empty());
}

#[test]
fn sim_rejections_go_back_to_the_sender() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "bob", Some("East"));
    let cancel = PlayerCommand::CancelRoute { entrance: s("W1") };
    assert!(command(&mut g, "alice", cancel.clone()).is_empty());
    let out = g.advance(0.1);
    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: cancel, reason: Rejection::RouteNotSet }]);
    assert!(notices(&out, "bob").is_empty());
    assert_eq!(g.stats().sim_rejections, 1);
}

#[test]
fn bad_headcodes_are_refused() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    for h in ["", "1A 01", "ABCDEFGHIJK"] {
        let out = command(&mut g, "alice", PlayerCommand::Interpose { berth: s("BA"), headcode: s(h) });
        assert_eq!(error_codes(&out, "alice"), [codes::BAD_HEADCODE], "{h:?}");
    }
    g.advance(0.1);
    assert!(g.sim().log().is_empty());
}

#[test]
fn the_robot_never_commands_a_claimed_area() {
    let mut g = game();
    join(&mut g, "alice", Some("East"));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
    run(&mut g, 20.0 * 60.0 / 8.0, 0.1);
    let map = AreaMap::new(g.sim().world());
    let east = g.sim().world().net.area("East").unwrap();
    assert!(g.stats().robot_commands > 0, "{:?}", g.stats());
    assert!(g.sim().log().iter().all(|(_, c)| map.subject(c) != Some(east)), "{:?}", g.sim().log());
}

#[test]
fn claiming_stops_the_robot_at_once_and_its_routes_stay_set() {
    let mut g = game();
    join(&mut g, "alice", None);
    run_to_tick(&mut g, 11);
    let before = g.sim().log().len();
    assert!(before >= 2, "the robot routes 1E01 at tick 10: {:?}", g.sim().log());
    let out = claim(&mut g, "alice", "West");
    let ServerMsg::View(v) = &out[1].1 else { panic!("{out:?}") };
    assert!(v.routes.contains_key("W1-A") && v.routes.contains_key("A-E"), "{:?}", v.routes);
    run_to_tick(&mut g, 15 * 600);
    let map = AreaMap::new(g.sim().world());
    let west = g.sim().world().net.area("West").unwrap();
    assert!(g.sim().log()[before..].iter().all(|(_, c)| map.subject(c) != Some(west)), "{:?}", g.sim().log());
}

#[test]
fn a_train_crossing_into_your_area_is_handed_over() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    command(&mut g, "alice", set_route("W1", ExitName::Signal(s("A"))));
    command(&mut g, "alice", set_route("A", ExitName::Node(s("E"))));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
    let out = run(&mut g, 16.0 * 60.0 / 8.0, 0.1);
    let handover = Notice::Handover { headcode: s("2W03"), from_area: s("East") };
    assert!(notices(&out, "alice").contains(&handover), "{:?}", notices(&out, "alice"));
}

#[test]
fn the_clock_follows_votes() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    g.advance(1.0);
    assert_eq!(g.sim().tick(), 10);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } });
    g.advance(1.0);
    assert_eq!(g.sim().tick(), 50);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    g.advance(10.0);
    assert_eq!(g.sim().tick(), 50);
    assert!(g.view_of("alice").unwrap().paused);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Resume });
    g.advance(0.1);
    assert_eq!(g.sim().tick(), 54);
    let out = send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 3 } });
    assert_eq!(error_codes(&out, "alice"), [codes::BAD_SPEED]);
    join(&mut g, "sam", None);
    let out = send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    assert_eq!(error_codes(&out, "sam"), [codes::NOT_A_HOLDER]);
}

#[test]
fn advance_survives_bad_real_time() {
    let mut g = game();
    for dt in [f64::NAN, -1.0, f64::NEG_INFINITY, f64::INFINITY, 0.0, -0.0] {
        assert!(g.advance(dt).is_empty(), "{dt}");
        assert_eq!(g.sim().tick(), 0, "{dt}");
    }
    g.advance(1e9);
    assert_eq!(g.sim().tick(), MAX_TICKS_PER_ADVANCE);
    g.advance(0.1);
    assert_eq!(g.sim().tick(), MAX_TICKS_PER_ADVANCE + 1);
}

#[test]
fn grace_keeps_the_area_then_releases_it() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "bob", None);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    g.disconnect("alice");
    assert!(g.handle("alice", ClientMsg::Release).is_empty(), "a disconnected player is not heard");
    g.advance(GRACE_S - 1.0);
    assert_eq!(g.holder("West"), Some("alice"));
    assert_eq!(notices(&claim(&mut g, "bob", "West"), "bob"), vec![Notice::AreaTaken { area: s("West"), holder: s("alice") }]);
    let out = g.connect("alice");
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!(l.area.as_deref(), Some("West"));
    g.disconnect("alice");
    g.advance(GRACE_S - 1.0);
    assert_eq!(g.holder("West"), Some("alice"), "reconnecting restarted the grace period");
    g.advance(1.0);
    assert_eq!(g.holder("West"), None);
    assert_eq!(g.view_of("bob").unwrap().holders["West"], ROBOT);
    claim(&mut g, "bob", "West");
    assert_eq!(g.holder("West"), Some("bob"));
}

/// A front that repeats `disconnect` must not keep the area held forever.
#[test]
fn a_repeated_disconnect_does_not_restart_the_grace_period() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    g.disconnect("alice");
    g.advance(100.0);
    g.disconnect("alice");
    g.advance(30.0);
    assert_eq!(g.holder("West"), None, "the grace period ran from the first disconnect");
    assert_eq!(g.area_of("alice"), None);
}

#[test]
fn a_spectator_who_leaves_is_forgotten() {
    let mut g = game();
    join(&mut g, "sam", None);
    g.disconnect("sam");
    assert_eq!(g.view_of("sam"), None);
    assert!(g.flush().is_empty());
}

#[test]
fn grace_expiry_completes_a_vote() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "bob", Some("East"));
    g.disconnect("bob");
    g.advance(100.0);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    assert!(!g.clock().paused, "bob still holds East and has not agreed");
    g.advance(10.0);
    assert!(!g.clock().paused && g.clock().vote.is_some());
    g.advance(10.0);
    assert_eq!(g.holder("East"), None);
    assert!(g.clock().paused, "bob's grace ran out, leaving alice as the only holder");
}

/// Owner decision 12: with every area robot-run, the spectators vote.
#[test]
fn a_lone_spectator_runs_the_clock_of_a_robot_only_game() {
    let mut g = game();
    join(&mut g, "sam", None);
    assert_eq!(g.voters(), BTreeSet::from([s("sam")]));
    assert!(send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause }).is_empty());
    assert!(g.clock().paused, "a lone voter's proposal applies at once");
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } });
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Resume });
    assert_eq!((g.clock().paused, g.clock().speed), (false, 4));
}

#[test]
fn spectators_of_a_robot_only_game_must_all_agree() {
    let mut g = game();
    join(&mut g, "sam", None);
    join(&mut g, "tom", None);
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    assert!(!g.clock().paused && g.clock().vote.is_some(), "tom has not agreed");
    send(&mut g, "tom", ClientMsg::Vote { proposal: Proposal::Pause });
    assert!(g.clock().paused);
}

#[test]
fn a_spectator_who_leaves_can_complete_a_spectators_vote() {
    let mut g = game();
    join(&mut g, "sam", None);
    join(&mut g, "tom", None);
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    g.disconnect("tom");
    assert!(g.clock().paused, "sam is the only voter left, and agreed");
}

#[test]
fn a_claim_stops_the_spectators_votes_counting() {
    let mut g = game();
    join(&mut g, "sam", None);
    join(&mut g, "tom", None);
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    join(&mut g, "alice", Some("West"));
    assert_eq!(g.voters(), BTreeSet::from([s("alice")]));
    assert!(!g.clock().paused && g.clock().vote.is_some(), "re-settled: alice has not agreed");
    let out = send(&mut g, "tom", ClientMsg::Vote { proposal: Proposal::Pause });
    assert_eq!(error_codes(&out, "tom"), [codes::NOT_A_HOLDER]);
    assert!(!g.clock().paused);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    assert!(g.clock().paused);
    send(&mut g, "alice", ClientMsg::Release);
    assert_eq!(g.voters(), BTreeSet::from([s("alice"), s("sam"), s("tom")]), "nobody holds an area again");
}

#[test]
fn a_spectator_who_agreed_and_then_claims_completes_the_vote() {
    let mut g = game();
    join(&mut g, "sam", None);
    join(&mut g, "tom", None);
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
    claim(&mut g, "sam", "East");
    assert_eq!(g.clock().speed, 8, "sam is now the only voter, and agreed");
}

#[test]
fn a_holder_in_grace_still_counts_and_spectators_still_do_not() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "sam", None);
    g.disconnect("alice");
    assert_eq!(g.voters(), BTreeSet::from([s("alice")]), "alice holds West through her grace period");
    let out = send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    assert_eq!(error_codes(&out, "sam"), [codes::NOT_A_HOLDER]);
    g.advance(GRACE_S);
    assert_eq!(g.holder("West"), None);
    assert_eq!(g.voters(), BTreeSet::from([s("sam")]), "the grace ran out: now sam decides");
    send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    assert!(g.clock().paused);
}

#[test]
fn resync_restarts_the_delta_base() {
    let mut g = game();
    let mut c = Client::default();
    c.take(&g.connect("alice"), "alice");
    g.advance(1.0);
    let out = g.flush();
    assert!(matches!(&out[..], [(_, ServerMsg::Delta(d))] if d.seq == 2), "{out:?}");
    c.take(&out, "alice");
    let out = send(&mut g, "alice", ClientMsg::Resync);
    assert!(matches!(&out[..], [(_, ServerMsg::Layout(_)), (_, ServerMsg::View(v))] if v.seq == 3), "{out:?}");
    c.take(&out, "alice");
    g.advance(1.0);
    let out = g.flush();
    assert!(matches!(&out[..], [(_, ServerMsg::Delta(d))] if d.seq == 4), "{out:?}");
    c.take(&out, "alice");
    assert_eq!(c.view, g.view_of("alice"));
}

#[test]
fn flush_sends_nothing_when_nothing_changed() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    assert_eq!(g.flush().len(), 1, "the pause itself");
    g.advance(5.0);
    assert!(g.flush().is_empty());
}

fn deliver(clients: &mut BTreeMap<&'static str, Client>, out: &[Out]) {
    for (p, c) in clients.iter_mut() {
        c.take(out, p);
    }
}

/// Spec §12: area, fringe and spectator views rebuilt from deltas equal the
/// server's full view at every flush, through claims and releases.
#[test]
fn every_client_rebuilds_the_servers_view_from_deltas() {
    let mut g = game();
    let mut clients: BTreeMap<&'static str, Client> = BTreeMap::new();
    for p in ["alice", "bob", "carol"] {
        clients.insert(p, Client::default());
    }
    for (p, a) in [("alice", Some("West")), ("bob", Some("East")), ("carol", None)] {
        let out = join(&mut g, p, a);
        deliver(&mut clients, &out);
    }
    for p in ["alice", "bob"] {
        let out = send(&mut g, p, ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
        deliver(&mut clients, &out);
    }
    assert_eq!(g.clock().speed, 8);
    let mut trains_seen: BTreeMap<&'static str, usize> = BTreeMap::new();
    // 0.125 s at 8x is one robot period (10 ticks): 2400 periods = 40 sim minutes.
    for period in 0..2400u64 {
        if period == 1200 {
            let out = send(&mut g, "bob", ClientMsg::Release);
            deliver(&mut clients, &out);
        }
        if period == 1500 {
            let out = claim(&mut g, "bob", "East");
            deliver(&mut clients, &out);
        }
        for p in ["alice", "bob"] {
            let out = play_as_robot(&mut g, p);
            deliver(&mut clients, &out);
        }
        let out = g.advance(0.125);
        deliver(&mut clients, &out);
        if period % 2 == 1 {
            let out = g.flush();
            deliver(&mut clients, &out);
            for (p, c) in &clients {
                assert_eq!(c.view, g.view_of(p), "{p} at tick {}", g.sim().tick());
                assert_eq!(c.layout, g.layout_of(p), "{p}");
                *trains_seen.entry(p).or_default() += c.view.as_ref().map_or(0, |v| v.trains.len());
            }
        }
    }
    assert!(trains_seen.values().all(|&n| n > 0), "every view listed trains: {trains_seen:?}");
    let st = g.stats();
    assert!(st.player_commands > 0, "{st:?}");
    assert_eq!((st.spads, st.collisions, st.invariant_violations), (0, 0, 0), "{st:?}");
}

// ---- what tutorials use (tutorial spec §3) ----

#[test]
fn advance_with_sees_every_tick_and_stops_once_paused() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    g.set_speed(8);
    let mut ticks = Vec::new();
    g.advance_with(1.0, |g, _, _| {
        ticks.push(g.sim().tick());
        if ticks.len() == 3 {
            g.set_paused(true);
        }
        vec![]
    });
    assert_eq!(ticks, [1, 2, 3], "80 ticks were due; the pause stopped them after 3");
    assert!(g.clock().paused);
    assert!(!g.set_speed(3), "only 1, 2, 4 or 8");
    assert_eq!(g.clock().speed, 8);
}

#[test]
fn a_snapshot_puts_the_sim_and_the_clock_back() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    let snap = g.snapshot();
    let t0 = g.sim().now_s();
    command(&mut g, "alice", set_route("W1", ExitName::Signal(s("A"))));
    g.advance(30.0);
    g.flush();
    g.set_speed(4);
    assert!(g.sim().interlocking().active_route_from(g.sim().world(), g.sim().world().net.signal("W1").unwrap()).is_some());
    g.restore(&snap).unwrap();
    assert_eq!(g.sim().now_s(), t0);
    assert_eq!(g.clock().speed, 1);
    assert!(g.sim().interlocking().active_route_from(g.sim().world(), g.sim().world().net.signal("W1").unwrap()).is_none());
    assert_eq!(g.area_of("alice"), Some("West"), "players and holders stay");
    let out = g.flush();
    assert!(out.iter().any(|(p, m)| p == "alice" && matches!(m, ServerMsg::Delta(_))), "the change goes out as a delta");
}

#[test]
fn a_demonstration_acts_in_any_area_and_tells_nobody() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    g.demonstrate(&set_route("C", ExitName::Signal(s("W2")))).unwrap();
    g.demonstrate(&PlayerCommand::CancelRoute { entrance: s("W1") }).unwrap();
    assert_eq!(g.demonstrate(&set_route("Nope", ExitName::Signal(s("W2")))), Err(Rejection::UnknownId));
    let out = g.advance(0.1);
    assert!(notices(&out, "alice").is_empty(), "the refused cancel is told to nobody");
    let w = g.sim().world();
    assert!(g.sim().interlocking().active_route_from(w, w.net.signal("C").unwrap()).is_some(), "East's route, while alice holds West");
    assert_eq!(g.stats().sim_rejections, 1);
}
