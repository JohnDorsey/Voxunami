//! The messages of the bridge: one JSON object a line (`tools/virtual_atlas/BRIDGE.md` is the description both sides keep).

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// The version of the protocol the program speaks; the tool refuses another.
pub const PROTOCOL: u32 = 1;
/// The PBR convention the game reads the companion pictures of a tile in (`docs/specs/block-textures-spec.md` section 4.4): a tool that paints them says in `welcome` which
/// convention its own setting is, and sends them only if it is this one.
pub const PBR_PROFILE: &str = "labpbr_1_3";
/// The port a tool listens on unless it was told another (on this machine only).
pub const DEFAULT_PORT: u16 = 47625;
/// The most a line of the tool's may hold before the rest of it is thrown away (the largest picture, 1024 by 1024 at four bytes a pixel, is under this in base64).
const MOST_LINE_BYTES: usize = 8 * 1024 * 1024;

pub fn encode_base64(bytes: &[u8]) -> String {
    super::base64::encode(bytes)
}

pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    super::base64::decode(text)
}

/// A picture of the game as the tool is told of it: an id of the game's own, the names of the files it may be (the tool uses the first it has), and its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogueTile {
    pub id: String,
    /// What the game groups it with (the namespace of its pack), for a tool that works on the pictures of one group.
    pub group: Option<String>,
    pub files: Vec<String>,
    pub size: u32,
}

/// Where the pointer is on a picture. `x` and `y` count pixels on the picture repeated in every direction (the pixel of the picture is `x` and `y` modulo its size),
/// so that a stroke that goes over the edge of one face onto the next goes on; they are only continuous within one `segment`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pointer {
    pub tile: Option<String>,
    pub x: i64,
    pub y: i64,
    pub left: bool,
    pub right: bool,
    pub segment: u64,
}

/// What the game says to the tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToTool {
    /// `profile` is the PBR convention the game reads companion pictures in; left out of the line when there is none.
    Hello {
        host: String,
        profile: Option<String>,
    },
    Tiles(Vec<CatalogueTile>),
    Pointer(Pointer),
}

impl ToTool {
    /// The message as one line of JSON, with its end of line.
    pub fn to_line(&self) -> Vec<u8> {
        let value = match self {
            Self::Hello { host, profile } => {
                let mut hello = json!({"type": "hello", "protocol": PROTOCOL, "host": host});
                if let Some(profile) = profile {
                    hello["profile"] = json!(profile);
                }
                hello
            }
            Self::Tiles(tiles) => json!({
                "type": "tiles",
                "tiles": tiles.iter().map(|tile| {
                    let mut entry = json!({"id": tile.id, "files": tile.files, "size": tile.size});
                    if let Some(group) = &tile.group {
                        entry["group"] = json!(group);
                    }
                    entry
                }).collect::<Vec<_>>(),
            }),
            Self::Pointer(pointer) => json!({
                "type": "pointer",
                "tile": pointer.tile,
                "x": pointer.x,
                "y": pointer.y,
                "left": pointer.left,
                "right": pointer.right,
                "segment": pointer.segment,
            }),
        };
        let mut line = value.to_string().into_bytes();
        line.push(b'\n');
        line
    }
}

/// What the tool says to the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FromTool {
    Welcome {
        tool: String,
        tile_size: u32,
        /// The PBR convention the tool's atlas is set to, if it has channels (a tool whose setting is none says nothing).
        profile: Option<String>,
    },
    /// Which of the game's pictures the tool has a file for and which not.
    Known {
        ids: Vec<String>,
        missing: Vec<String>,
    },
    /// What a picture is now: `size` by `size` pixels, four bytes (red, green, blue, alpha) each.
    Tile {
        id: String,
        size: u32,
        rgba: Vec<u8>,
        /// The companion pictures the tool's files for this tile would hold, by the ending of the file (`_s`, `_n`), each `size` by `size` pixels of four bytes. `None`: the tool
        /// said nothing of them (the picture keeps the companions it had). `Some` of an empty set: no companion file would exist, and the game clears its own.
        companions: Option<BTreeMap<String, Vec<u8>>>,
    },
    Error {
        text: String,
    },
    /// A kind this version does not know: ignored, so that a newer tool can still be used.
    Unknown,
}

impl FromTool {
    /// The message in one line (without its end of line), or what is wrong with it.
    pub fn parse(line: &[u8]) -> Result<Self, String> {
        let value: Value = serde_json::from_slice(line).map_err(|error| error.to_string())?;
        let text = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| format!("\"{key}\" is text"))
        };
        let number = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_u64)
                .and_then(|number| u32::try_from(number).ok())
                .ok_or_else(|| format!("\"{key}\" is a whole number"))
        };
        let texts = |key: &str| -> Result<Vec<String>, String> {
            value
                .get(key)
                .and_then(Value::as_array)
                .and_then(|list| {
                    list.iter()
                        .map(|item| item.as_str().map(str::to_owned))
                        .collect()
                })
                .ok_or_else(|| format!("\"{key}\" is a list of text"))
        };
        match value.get("type").and_then(Value::as_str) {
            Some("welcome") => Ok(Self::Welcome {
                tool: text("tool")?,
                tile_size: number("tile_size")?,
                profile: match value.get("profile") {
                    None => None,
                    Some(_) => Some(text("profile")?),
                },
            }),
            Some("known") => Ok(Self::Known {
                ids: texts("ids")?,
                missing: texts("missing")?,
            }),
            Some("tile") => {
                let (id, size) = (text("id")?, number("size")?);
                let rgba = decode_base64(&text("pixels")?)
                    .ok_or_else(|| "\"pixels\" is base64".to_owned())?;
                let wanted = u64::from(size) * u64::from(size) * 4;
                if rgba.len() as u64 != wanted {
                    return Err(format!(
                        "a tile of {size} by {size} has {wanted} bytes of pixels, not {}",
                        rgba.len()
                    ));
                }
                let companions = match value.get("companions") {
                    None => None,
                    Some(Value::Object(files)) => {
                        let mut found = BTreeMap::new();
                        for (ending, picture) in files {
                            let bytes = picture
                                .as_str()
                                .and_then(decode_base64)
                                .ok_or_else(|| format!("the companion \"{ending}\" is base64"))?;
                            if bytes.len() as u64 != wanted {
                                return Err(format!(
                                    "the companion \"{ending}\" of a tile of {size} by {size} has {wanted} bytes of pixels, not {}",
                                    bytes.len()
                                ));
                            }
                            found.insert(ending.clone(), bytes);
                        }
                        Some(found)
                    }
                    Some(_) => return Err("\"companions\" is an object of pictures".to_owned()),
                };
                Ok(Self::Tile {
                    id,
                    size,
                    rgba,
                    companions,
                })
            }
            Some("error") => Ok(Self::Error {
                text: text("text")?,
            }),
            Some(_) => Ok(Self::Unknown),
            None => Err("a message has a \"type\"".to_owned()),
        }
    }
}

/// Cuts what arrives into lines (without their ends of line). A line longer than the most is thrown away, with the rest of it up to the next end of line.
#[derive(Debug, Default)]
pub struct LineBuffer {
    pending: Vec<u8>,
    skipping: bool,
}

impl LineBuffer {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut lines = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                if self.skipping {
                    self.skipping = false;
                } else if !self.pending.iter().all(u8::is_ascii_whitespace) {
                    lines.push(std::mem::take(&mut self.pending));
                }
                self.pending.clear();
            } else if !self.skipping {
                self.pending.push(byte);
                if self.pending.len() > MOST_LINE_BYTES {
                    self.pending.clear();
                    self.skipping = true;
                }
            }
        }
        lines
    }
}
