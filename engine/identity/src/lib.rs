//! A player's identity: a keypair per install, and the proof of holding it that a server asks for when
//! a player connects.
//!
//! There are no accounts. An install makes an Ed25519 keypair the first time it needs one and keeps it
//! in a file ([`Identity::load_or_create`]); the public key is who the player is ([`PlayerKey`]). To
//! connect, the client sends its public key, the server sends bytes it has not used before (a
//! challenge, from a [`ChallengeSource`]), and the client signs them ([`Identity::prove`]); the server
//! checks the signature ([`verify`]). What is signed names the game and the key as well as the
//! challenge, so a proof is good for one login to one game and for nothing else.
//!
//! This proves who is connecting; it hides nothing: nothing on the wire is encrypted.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use namespaced_id::NamespacedId;
use protocol::{ClientMessage, PROTOCOL_VERSION, PlayerKey, ServerMessage};
use rand_core::{OsRng, RngCore};

/// What is signed starts with this, so that the key is never used to sign something that looks like
/// something else.
const LOGIN_DOMAIN: &[u8] = b"voxunami login v1";

/// The bytes that a login signs: the domain, the game, the server's challenge and the player's key.
pub fn login_message(game: &NamespacedId, challenge: &[u8; 32], key: &PlayerKey) -> Vec<u8> {
    let game = game.as_str().as_bytes();
    let mut message = Vec::with_capacity(LOGIN_DOMAIN.len() + 4 + game.len() + 32 + 32);
    message.extend_from_slice(LOGIN_DOMAIN);
    message.extend_from_slice(&(game.len() as u32).to_le_bytes());
    message.extend_from_slice(game);
    message.extend_from_slice(challenge);
    message.extend_from_slice(key.bytes());
    message
}

/// Whether `signature` is the proof, by the holder of `key`, of a login to `game` that the server asked
/// for with `challenge`. Strict: a key or signature of a weak or malleable form is refused.
pub fn verify(
    key: &PlayerKey,
    game: &NamespacedId,
    challenge: &[u8; 32],
    signature: &[u8; 64],
) -> bool {
    let Ok(verifying) = VerifyingKey::from_bytes(key.bytes()) else {
        return false;
    };
    verifying
        .verify_strict(
            &login_message(game, challenge, key),
            &Signature::from_bytes(signature),
        )
        .is_ok()
}

/// An install's keypair. The private half never leaves it: [`Debug`](fmt::Debug) shows only the public key.
#[derive(Clone)]
pub struct Identity {
    key: SigningKey,
}

impl Identity {
    /// A new identity from the operating system's randomness.
    pub fn generate() -> Self {
        Self {
            key: SigningKey::generate(&mut OsRng),
        }
    }

    /// The identity of this private key. Tests use fixed ones, so that a run is the same every time.
    pub fn from_secret(secret: [u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(&secret),
        }
    }

    /// The private key, to be kept in a file and nowhere else.
    pub fn secret(&self) -> [u8; 32] {
        self.key.to_bytes()
    }

    /// The public key: who this player is.
    pub fn player_key(&self) -> PlayerKey {
        PlayerKey::new(self.key.verifying_key().to_bytes())
    }

    /// The signature that proves this identity to a server of `game` that asked with `challenge`.
    pub fn prove(&self, game: &NamespacedId, challenge: &[u8; 32]) -> [u8; 64] {
        self.key
            .sign(&login_message(game, challenge, &self.player_key()))
            .to_bytes()
    }

    /// The hello a client of `game` sends to say who it is.
    pub fn hello(&self, game: &NamespacedId) -> ClientMessage {
        ClientMessage::Hello {
            protocol_version: PROTOCOL_VERSION,
            game: game.clone(),
            player_key: self.player_key(),
        }
    }

    /// The proof that answers a frame from the server, if the frame is its challenge. For programs that
    /// talk to a server by hand (tests, tools); the client's handshake does this itself.
    pub fn answer(&self, game: &NamespacedId, frame: &[u8]) -> Option<ClientMessage> {
        match ServerMessage::decode(frame) {
            Ok(ServerMessage::Challenge { nonce }) => Some(ClientMessage::Prove {
                signature: self.prove(game, &nonce),
            }),
            _ => None,
        }
    }

    /// Where an install keeps its identity unless told otherwise: `identity` in the game's data
    /// directory (`%APPDATA%\voxunami` on Windows, `~/Library/Application Support/voxunami` on macOS,
    /// `$XDG_DATA_HOME/voxunami` or `~/.local/share/voxunami` elsewhere). `None` if the home directory is
    /// not known.
    pub fn default_path() -> Option<PathBuf> {
        let environment = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let directory = if cfg!(windows) {
            PathBuf::from(environment("APPDATA")?)
        } else if cfg!(target_os = "macos") {
            PathBuf::from(environment("HOME")?).join("Library/Application Support")
        } else if let Some(data) = environment("XDG_DATA_HOME") {
            PathBuf::from(data)
        } else {
            PathBuf::from(environment("HOME")?).join(".local/share")
        };
        Some(directory.join("voxunami").join("identity"))
    }

    /// The identity kept in the file at `path`, or, if there is no file, a new one, kept there. A file
    /// that is not a key is an error and is never overwritten: it may be someone's identity, damaged.
    pub fn load_or_create(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let secret: [u8; 32] = bytes.try_into().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{} is not an identity (it is not 32 bytes)", path.display()),
                    )
                })?;
                Ok(Self::from_secret(secret))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let identity = Self::generate();
                write_new_private_file(path, &identity.secret())?;
                Ok(identity)
            }
            Err(error) => Err(error),
        }
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Identity({})", self.player_key())
    }
}

/// Writes a file that must not exist yet, readable by its owner alone where the system has such a thing.
fn write_new_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

/// Where a server gets the bytes it challenges players with.
#[derive(Debug)]
pub enum ChallengeSource {
    /// The operating system's randomness: what a real server uses, since a challenge that can be
    /// guessed is no challenge.
    Random,
    /// A counter from a seed, so that a test's stream is the same every time. Never for a real server.
    Counter { seed: u64, issued: u64 },
}

impl ChallengeSource {
    pub fn random() -> Self {
        Self::Random
    }

    pub fn counter(seed: u64) -> Self {
        Self::Counter { seed, issued: 0 }
    }

    /// The next challenge: never one given before (from a counter), or overwhelmingly never (random).
    pub fn next_challenge(&mut self) -> [u8; 32] {
        let mut challenge = [0u8; 32];
        match self {
            Self::Random => OsRng.fill_bytes(&mut challenge),
            Self::Counter { seed, issued } => {
                challenge[..8].copy_from_slice(&seed.to_le_bytes());
                challenge[8..16].copy_from_slice(&issued.to_le_bytes());
                challenge[16..].fill(0xA5);
                *issued += 1;
            }
        }
        challenge
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(text: &str) -> NamespacedId {
        text.parse().unwrap()
    }

    #[test]
    fn a_proof_made_by_the_holder_of_a_key_is_accepted_for_that_login_only() {
        let identity = Identity::from_secret([7; 32]);
        let key = identity.player_key();
        let challenge = [1; 32];
        let proof = identity.prove(&game("test:game"), &challenge);
        assert!(verify(&key, &game("test:game"), &challenge, &proof));
        // Not for another game, another challenge, another key, or after any change to the proof.
        assert!(!verify(&key, &game("test:other"), &challenge, &proof));
        assert!(!verify(&key, &game("test:game"), &[2; 32], &proof));
        let other = Identity::from_secret([8; 32]).player_key();
        assert!(!verify(&other, &game("test:game"), &challenge, &proof));
        for byte in 0..64 {
            let mut damaged = proof;
            damaged[byte] ^= 1;
            assert!(
                !verify(&key, &game("test:game"), &challenge, &damaged),
                "{byte}"
            );
        }
    }

    #[test]
    fn an_identity_is_its_secret_and_nothing_else() {
        let first = Identity::from_secret([3; 32]);
        let second = Identity::from_secret(first.secret());
        assert_eq!(first.player_key(), second.player_key());
        // Ed25519 signatures are deterministic, so the same identity proves the same login the same way.
        assert_eq!(
            first.prove(&game("test:game"), &[9; 32]),
            second.prove(&game("test:game"), &[9; 32])
        );
        assert_ne!(
            first.player_key(),
            Identity::from_secret([4; 32]).player_key()
        );
        assert_ne!(
            Identity::generate().player_key(),
            Identity::generate().player_key()
        );
    }

    #[test]
    fn the_private_half_is_never_shown() {
        let identity = Identity::from_secret([0xAB; 32]);
        let shown = format!("{identity:?}");
        assert!(shown.contains(&identity.player_key().to_string()));
        assert!(!shown.contains("abababab"), "{shown}");
    }

    #[test]
    fn the_hello_names_the_key_and_the_answer_is_the_proof_of_a_challenge_and_of_nothing_else() {
        let identity = Identity::from_secret([2; 32]);
        let game = game("test:game");
        assert_eq!(
            identity.hello(&game),
            ClientMessage::Hello {
                protocol_version: PROTOCOL_VERSION,
                game: game.clone(),
                player_key: identity.player_key(),
            }
        );
        let challenge = ServerMessage::Challenge { nonce: [4; 32] }.encode();
        let Some(ClientMessage::Prove { signature }) = identity.answer(&game, &challenge) else {
            panic!("a challenge is answered with a proof");
        };
        assert!(verify(&identity.player_key(), &game, &[4; 32], &signature));
        let welcome_or_junk = ServerMessage::TickCompleted {
            tick: protocol::Tick::ZERO,
        }
        .encode();
        assert_eq!(identity.answer(&game, &welcome_or_junk), None);
        assert_eq!(identity.answer(&game, &[255]), None);
    }

    #[test]
    fn what_is_signed_names_the_game_the_challenge_and_the_key_after_a_domain() {
        let key = PlayerKey::new([5; 32]);
        let message = login_message(&game("a:b"), &[6; 32], &key);
        let mut expected = b"voxunami login v1".to_vec();
        expected.extend([3, 0, 0, 0]);
        expected.extend(b"a:b");
        expected.extend([6; 32]);
        expected.extend([5; 32]);
        assert_eq!(message, expected);
    }

    #[test]
    fn the_public_key_of_a_known_secret_is_pinned() {
        // So that a change of library or of how a key is made cannot quietly change everyone's identity.
        let key = Identity::from_secret([1; 32]).player_key();
        assert_eq!(
            key.to_string(),
            "8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c"
        );
    }

    #[test]
    fn a_key_that_is_not_a_point_or_a_weak_one_proves_nothing() {
        let challenge = [0; 32];
        // All zeros is a point of small order: a signature of zeros must not be accepted for it.
        assert!(!verify(
            &PlayerKey::new([0; 32]),
            &game("test:game"),
            &challenge,
            &[0; 64]
        ));
        assert!(!verify(
            &PlayerKey::new([0xFF; 32]),
            &game("test:game"),
            &challenge,
            &[0; 64]
        ));
    }

    #[test]
    fn the_identity_point_with_a_signature_made_for_it_is_refused() {
        // The point (0, 1) is the group's identity: with a signature whose R is that point and whose scalar is 0, every
        // message "verifies" under the plain check, for anyone who claims that key. Only the strict check refuses it.
        let mut key = [0; 32];
        key[0] = 1;
        let mut signature = [0; 64];
        signature[0] = 1;
        assert!(!verify(
            &PlayerKey::new(key),
            &game("test:game"),
            &[3; 32],
            &signature
        ));
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "voxunami_identity_test_{}_{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    #[test]
    fn an_install_makes_its_identity_once_and_finds_it_again() {
        let directory = scratch("create");
        let path = directory.join("nested").join("identity");
        let first = Identity::load_or_create(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), first.secret());
        let again = Identity::load_or_create(&path).unwrap();
        assert_eq!(again.player_key(), first.player_key());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_file_that_is_not_an_identity_is_an_error_and_is_left_alone() {
        let directory = scratch("damaged");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("identity");
        for contents in [vec![], vec![1; 31], vec![1; 33]] {
            std::fs::write(&path, &contents).unwrap();
            let error = Identity::load_or_create(&path).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert_eq!(std::fs::read(&path).unwrap(), contents, "not overwritten");
        }
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_by_its_owner_alone() {
        use std::os::unix::fs::PermissionsExt;
        let directory = scratch("mode");
        let path = directory.join("identity");
        Identity::load_or_create(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_counter_gives_each_challenge_once_the_same_way_every_time_and_a_seed_its_own() {
        let mut first = ChallengeSource::counter(1);
        let mut second = ChallengeSource::counter(1);
        let challenges: Vec<[u8; 32]> = (0..5).map(|_| first.next_challenge()).collect();
        let again: Vec<[u8; 32]> = (0..5).map(|_| second.next_challenge()).collect();
        assert_eq!(challenges, again);
        for (index, challenge) in challenges.iter().enumerate() {
            assert!(!challenges[..index].contains(challenge));
        }
        assert_ne!(ChallengeSource::counter(2).next_challenge(), challenges[0]);
    }

    #[test]
    fn random_challenges_differ() {
        let mut source = ChallengeSource::random();
        let challenges: Vec<[u8; 32]> = (0..8).map(|_| source.next_challenge()).collect();
        for (index, challenge) in challenges.iter().enumerate() {
            assert!(!challenges[..index].contains(challenge));
        }
    }

    #[test]
    fn the_default_place_is_in_the_games_data_directory() {
        // The environment is the machine's, so only its shape is checked.
        if let Some(path) = Identity::default_path() {
            assert!(path.ends_with("voxunami/identity") || path.ends_with("voxunami\\identity"));
        }
    }
}
