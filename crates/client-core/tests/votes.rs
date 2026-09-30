//! Who may vote on the clock (owner decision 12), as the client sees it.

mod common;

use common::*;
use protocol::ClientMsg;

#[test]
fn the_robots_name_is_the_games() {
    assert_eq!(client_core::app::ROBOT, game::ROBOT);
}

#[test]
fn you_vote_when_you_hold_an_area_or_nobody_does() {
    let t = Table::new("ann", Some("West"));
    assert!(t.app.game().unwrap().can_vote(), "a holder");
    let mut t = Table::new("sam", None);
    assert!(t.app.game().unwrap().can_vote(), "every area is the robot's");
    t.game.connect("bob");
    t.game.handle("bob", ClientMsg::Claim { area: s("East") });
    t.run(0.3);
    assert_eq!(t.view().holders["East"], "bob");
    assert!(!t.app.game().unwrap().can_vote(), "bob holds East: only holders vote");
    t.game.handle("bob", ClientMsg::Release);
    t.run(0.3);
    assert!(t.app.game().unwrap().can_vote(), "nobody holds an area again");
}
