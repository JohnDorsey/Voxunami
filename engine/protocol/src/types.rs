use std::fmt;

/// A server tick: the unit of game time. Tick 0 is a world before its first step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tick(u64);

impl Tick {
    pub const ZERO: Self = Self(0);

    pub const fn new(count: u64) -> Self {
        Self(count)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    /// The tick after this one. Panics on overflow, which at any real tick rate takes far longer
    /// than the universe has existed.
    pub fn next(self) -> Self {
        Self(self.0.checked_add(1).expect("tick count overflowed"))
    }
}

impl fmt::Display for Tick {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "tick {}", self.0)
    }
}

/// Identifies one connection to a server for as long as the server runs. Ids are handed out in the
/// order clients join and are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(u32);

impl ClientId {
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "client {}", self.0)
    }
}

/// Identifies one entity of a world for as long as the world lives. Ids come from a counter that is
/// saved with the world and are never reused, so a recording, a reload and a second machine agree on
/// which entity an id means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(u64);

impl EntityId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "entity {}", self.0)
    }
}

/// A player's identity: the public key of the keypair their install holds (see the `identity` crate).
/// A server knows a player by it, so it is the same on every connection and in every world.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerKey([u8; 32]);

impl PlayerKey {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for PlayerKey {
    /// The key as 64 hexadecimal digits.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for PlayerKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "PlayerKey({self})")
    }
}
