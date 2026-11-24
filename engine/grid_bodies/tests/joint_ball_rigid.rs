//! The component that says a ball of a joint is the ball of a rigid joint: a message like any other, so its codec is checked as the others' are.

use grid_bodies::JointBallRigid;
use protocol::{Message, check_message};

#[test]
fn the_component_round_trips_and_refuses_what_it_cannot_be() {
    assert_eq!(check_message::<JointBallRigid>(), Ok(()));
    assert_eq!(JointBallRigid::decode(&[0]), Ok(JointBallRigid(false)));
    assert_eq!(JointBallRigid::decode(&[1]), Ok(JointBallRigid(true)));
    assert!(JointBallRigid::decode(&[2]).is_err());
    assert!(JointBallRigid::decode(&[]).is_err());
    assert!(JointBallRigid::decode(&[1, 0]).is_err());
}
