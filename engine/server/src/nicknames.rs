//! Which nickname a joining player gets: the one they asked for if it is theirs to have, else the same with a number.

use protocol::Nickname;

/// The nickname for a player who asked for `asked_for`, among the players whose nicknames are `in_use`: what they asked for
/// made valid ([`Nickname::sanitized`]), and if another player has that name (the letters compared without regard to case)
/// the first of `_2`, `_3`, ... after it that nobody has.
pub(crate) fn unique_nickname<'a>(
    asked_for: &str,
    in_use: impl Iterator<Item = &'a Nickname> + Clone,
) -> Nickname {
    let base = Nickname::sanitized(asked_for);
    let taken = |name: &Nickname| in_use.clone().any(|other| other.same_as(name));
    if !taken(&base) {
        return base;
    }
    (2u32..)
        .map(|number| base.with_number(number))
        .find(|candidate| !taken(candidate))
        .expect("there are more numbers than players")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(texts: &[&str]) -> Vec<Nickname> {
        texts
            .iter()
            .map(|text| Nickname::parse(text).unwrap())
            .collect()
    }

    #[test]
    fn a_free_name_is_kept_and_a_taken_one_is_numbered_from_two() {
        let in_use = names(&["Alex", "Alex_2", "Bo"]);
        assert_eq!(unique_nickname("Cy", in_use.iter()).as_str(), "Cy");
        assert_eq!(unique_nickname("Alex", in_use.iter()).as_str(), "Alex_3");
        assert_eq!(unique_nickname("aLEX", in_use.iter()).as_str(), "aLEX_3");
        assert_eq!(unique_nickname("bo", in_use.iter()).as_str(), "bo_2");
        assert_eq!(unique_nickname("Alex", [].iter()).as_str(), "Alex");
    }

    #[test]
    fn the_name_asked_for_is_made_valid_before_it_is_compared() {
        let in_use = names(&["a_b"]);
        assert_eq!(unique_nickname("a b", in_use.iter()).as_str(), "a_b_2");
        assert_eq!(unique_nickname("{}", in_use.iter()).as_str(), "__");
    }

    #[test]
    fn a_name_of_the_longest_length_gives_way_to_its_number_and_is_still_new() {
        let long = "x".repeat(32);
        let in_use = names(&[long.as_str()]);
        let made = unique_nickname(&long, in_use.iter());
        assert_eq!(made.as_str(), format!("{}_2", "x".repeat(30)));
    }

    #[test]
    fn however_many_have_the_name_each_gets_one_of_their_own() {
        let mut in_use: Vec<Nickname> = Vec::new();
        for _ in 0..30 {
            let made = unique_nickname("Alex", in_use.iter());
            assert!(in_use.iter().all(|other| !other.same_as(&made)));
            in_use.push(made);
        }
    }
}
