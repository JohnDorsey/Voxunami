//! The component that says a ball of a joint is the ball of a rigid joint: a message like any other, so its codec is checked as the others' are.

use grid_bodies::JointRigid;
use protocol::{Message, check_message};

#[test]
fn the_component_round_trips_and_refuses_what_it_cannot_be() {
    assert_eq!(check_message::<JointRigid>(), Ok(()));
    assert_eq!(JointRigid::decode(&[0]), Ok(JointRigid(false)));
    assert_eq!(JointRigid::decode(&[1]), Ok(JointRigid(true)));
    assert!(JointRigid::decode(&[2]).is_err());
    assert!(JointRigid::decode(&[]).is_err());
    assert!(JointRigid::decode(&[1, 0]).is_err());
}
