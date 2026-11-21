//! A picture of a block face, read from a PNG file: a square tile of 8-bit RGBA, 8 to 256 pixels on a side (`docs/specs/block-textures-spec.md`, 4.2).
//!
//! Nothing here trusts the file: the header is looked at before any pixel is made room for, a file of another kind is an error that says which, and
//! nothing is converted but RGB to RGBA.

use std::io::Cursor;

use crate::error::PackError;

/// The least and the greatest side of a tile, in pixels (a power of two between them).
pub const SMALLEST_TILE: u32 = 8;
pub const GREATEST_TILE: u32 = 256;

/// The most bytes of a PNG file that is read: a 256 by 256 picture is a few hundred kilobytes at the worst, so a file over this is not a tile.
pub const GREATEST_FILE_BYTES: usize = 1 << 20;

/// A tile: `size` pixels on a side, 8-bit RGBA, row after row, from the top left. It may carry the two companions of a LabPBR picture (`docs/specs/block-textures-spec.md`,
/// section 4.4), of its own size: the specular picture (`_s`: smoothness, reflectance, porosity and, in the alpha, emission) and the normal picture (`_n`: the normal's
/// x and y, the occlusion and, in the alpha, the height). They are data and not colours: no part of them is a colour or a share of covering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    size: u32,
    rgba: Vec<u8>,
    specular: Option<Vec<u8>>,
    normal: Option<Vec<u8>>,
}

fn problem(file: &str, message: impl Into<String>) -> PackError {
    PackError {
        file: file.to_owned(),
        path: String::new(),
        message: message.into(),
    }
}

impl Tile {
    /// A tile from its pixels. Panics if they are not `size * size` of 4 bytes, or `size` is not a size a tile has.
    pub fn from_rgba(size: u32, rgba: Vec<u8>) -> Self {
        assert!(
            is_tile_size(size),
            "{size} is not the side of a tile (a power of two from {SMALLEST_TILE} to {GREATEST_TILE})"
        );
        assert_eq!(
            rgba.len(),
            size as usize * size as usize * 4,
            "a tile has four bytes for each pixel"
        );
        Self {
            size,
            rgba,
            specular: None,
            normal: None,
        }
    }

    /// The tile with a specular companion: `size * size` pixels of 4 bytes, as the tile's own. Panics if they are not.
    pub fn with_specular(mut self, rgba: Vec<u8>) -> Self {
        self.check_companion(&rgba);
        self.specular = Some(rgba);
        self
    }

    /// The tile with a normal companion: `size * size` pixels of 4 bytes, as the tile's own. Panics if they are not.
    pub fn with_normal(mut self, rgba: Vec<u8>) -> Self {
        self.check_companion(&rgba);
        self.normal = Some(rgba);
        self
    }

    fn check_companion(&self, rgba: &[u8]) {
        assert_eq!(
            rgba.len(),
            self.rgba.len(),
            "a companion picture is of the tile's size, with four bytes for each pixel"
        );
    }

    /// The specular companion's pixels (RGBA, the tile's size), if the tile has one.
    pub fn specular(&self) -> Option<&[u8]> {
        self.specular.as_deref()
    }

    /// The normal companion's pixels (RGBA, the tile's size), if the tile has one.
    pub fn normal(&self) -> Option<&[u8]> {
        self.normal.as_deref()
    }

    /// The bytes a tile takes as pixels, companions included.
    pub fn bytes(&self) -> usize {
        self.rgba.len()
            + self.specular.as_ref().map_or(0, Vec::len)
            + self.normal.as_ref().map_or(0, Vec::len)
    }

    /// The side of the tile, in pixels.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// The pixels, 8-bit RGBA, row after row from the top left.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Reads a tile from the bytes of a PNG file. `file` is how the author knows the file, for the messages.
    pub fn from_png(file: &str, bytes: &[u8]) -> Result<Self, PackError> {
        if bytes.len() > GREATEST_FILE_BYTES {
            return Err(problem(
                file,
                format!(
                    "this file is {} bytes and a picture is at most {GREATEST_FILE_BYTES}",
                    bytes.len()
                ),
            ));
        }
        let decoder = png::Decoder::new(Cursor::new(bytes));
        let mut reader = decoder
            .read_info()
            .map_err(|error| problem(file, format!("this is not a PNG picture: {error}")))?;
        let (width, height, color, depth) = {
            let info = reader.info();
            (info.width, info.height, info.color_type, info.bit_depth)
        };
        if width != height {
            return Err(problem(
                file,
                format!("a picture is square, and this one is {width} by {height} pixels"),
            ));
        }
        if !is_tile_size(width) {
            return Err(problem(
                file,
                format!(
                    "a picture is a power of two from {SMALLEST_TILE} to {GREATEST_TILE} pixels on a side, and this one is {width}"
                ),
            ));
        }
        if depth != png::BitDepth::Eight
            || !matches!(color, png::ColorType::Rgb | png::ColorType::Rgba)
        {
            return Err(problem(
                file,
                format!(
                    "a picture is 8-bit RGB or RGBA, and this one is {} bits of {} (it is not converted)",
                    depth as u8,
                    color_name(color)
                ),
            ));
        }
        let mut buffer = vec![
            0;
            reader.output_buffer_size().ok_or_else(|| {
                problem(file, "this picture is too large to be read")
            })?
        ];
        let frame = reader
            .next_frame(&mut buffer)
            .map_err(|error| problem(file, format!("this PNG cannot be read: {error}")))?;
        buffer.truncate(frame.buffer_size());
        let rgba = match color {
            png::ColorType::Rgba => buffer,
            _ => buffer
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|[red, green, blue]| [*red, *green, *blue, 255])
                .collect(),
        };
        Ok(Self::from_rgba(width, rgba))
    }

    /// What the tile adds up to for an average: the linear-light colour of each pixel times its opacity, summed, and the opacity summed.
    fn sums(&self) -> ([f64; 3], f64) {
        let mut colour = [0.0; 3];
        let mut opacity = 0.0;
        for pixel in self.rgba.as_chunks::<4>().0 {
            let alpha = f64::from(pixel[3]) / 255.0;
            for (sum, byte) in colour.iter_mut().zip(pixel) {
                *sum += linear_from_srgb(f64::from(*byte) / 255.0) * alpha;
            }
            opacity += alpha;
        }
        (colour, opacity)
    }
}

/// Whether `size` is the side of a tile.
pub fn is_tile_size(size: u32) -> bool {
    size.is_power_of_two() && (SMALLEST_TILE..=GREATEST_TILE).contains(&size)
}

fn color_name(color: png::ColorType) -> &'static str {
    match color {
        png::ColorType::Grayscale => "grey",
        png::ColorType::GrayscaleAlpha => "grey with opacity",
        png::ColorType::Indexed => "palette colours",
        png::ColorType::Rgb => "RGB",
        png::ColorType::Rgba => "RGBA",
    }
}

fn linear_from_srgb(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        libm::pow((value + 0.055) / 1.055, 2.4)
    }
}

fn srgb_from_linear(value: f64) -> f64 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * libm::pow(value, 1.0 / 2.4) - 0.055
    }
}

/// The average colour of tiles, each counted `weight` times: the pixels' colours in linear light (the way the renderer blends), weighted by their opacity, as
/// an sRGB colour. `None` if the tiles are completely transparent, since they have no colour.
pub fn average_color(tiles: &[(&Tile, u32)]) -> Option<[f32; 3]> {
    let mut colour = [0.0; 3];
    let mut opacity = 0.0;
    for (tile, weight) in tiles {
        // Per pixel of the tile, so that a face counts as a face whatever the size of its picture.
        let pixels = f64::from(tile.size()) * f64::from(tile.size());
        let (sums, covered) = tile.sums();
        let weight = f64::from(*weight) / pixels;
        for (total, sum) in colour.iter_mut().zip(sums) {
            *total += sum * weight;
        }
        opacity += covered * weight;
    }
    (opacity > 0.0)
        .then(|| colour.map(|total| srgb_from_linear(total / opacity).clamp(0.0, 1.0) as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes of a PNG with these pixels, in this colour type and bit depth.
    fn png_with(
        width: u32,
        height: u32,
        color: png::ColorType,
        depth: png::BitDepth,
        data: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        let mut writer = encoder.write_header().expect("a header");
        writer.write_image_data(data).expect("the pixels");
        writer.finish().expect("the end");
        bytes
    }

    /// A tile of one colour as a PNG file.
    fn plain_png(size: u32, rgba: [u8; 4]) -> Vec<u8> {
        let data: Vec<u8> = (0..size * size).flat_map(|_| rgba).collect();
        png_with(
            size,
            size,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &data,
        )
    }

    fn plain_tile(size: u32, rgba: [u8; 4]) -> Tile {
        Tile::from_rgba(size, (0..size * size).flat_map(|_| rgba).collect())
    }

    const FILE: &str = "my_pack/textures/oak.png";

    #[test]
    fn a_tile_may_carry_a_specular_and_a_normal_companion_of_its_own_size_and_counts_their_bytes() {
        let tile = plain_tile(8, [1, 2, 3, 4]);
        assert_eq!((tile.specular(), tile.normal()), (None, None));
        assert_eq!(tile.bytes(), 8 * 8 * 4);
        let with = tile
            .clone()
            .with_specular(plain_tile(8, [5, 6, 7, 8]).rgba().to_vec())
            .with_normal(plain_tile(8, [9, 10, 11, 12]).rgba().to_vec());
        assert_eq!(&with.specular().unwrap()[0..4], [5, 6, 7, 8]);
        assert_eq!(&with.normal().unwrap()[0..4], [9, 10, 11, 12]);
        assert_eq!(with.rgba(), tile.rgba(), "the picture itself is the same");
        assert_eq!(with.bytes(), 3 * 8 * 8 * 4);
        assert_ne!(with, tile, "a companion is part of what a tile is");
    }

    #[test]
    #[should_panic(expected = "a companion picture is of the tile's size")]
    fn a_companion_of_another_size_is_refused() {
        let _ = plain_tile(8, [0; 4]).with_specular(plain_tile(16, [0; 4]).rgba().to_vec());
    }

    fn refusal(bytes: &[u8]) -> String {
        match Tile::from_png(FILE, bytes) {
            Err(error) => error.to_string(),
            Ok(tile) => panic!("accepted a tile of {}", tile.size()),
        }
    }

    #[test]
    fn a_square_8_bit_rgba_picture_of_a_power_of_two_is_a_tile_with_its_pixels() {
        for size in [8, 16, 32, 64, 128, 256] {
            let tile = Tile::from_png(FILE, &plain_png(size, [10, 20, 30, 40])).unwrap();
            assert_eq!(tile.size(), size);
            assert_eq!(tile.rgba().len(), (size * size * 4) as usize);
            assert!(
                tile.rgba()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == [10, 20, 30, 40])
            );
        }
        // The pixels come in the order of the file: row after row from the top left.
        let mut data = vec![0u8; 8 * 8 * 4];
        data[0..4].copy_from_slice(&[1, 2, 3, 4]);
        data[(8 * 8 - 1) * 4..].copy_from_slice(&[5, 6, 7, 8]);
        let tile = Tile::from_png(
            FILE,
            &png_with(8, 8, png::ColorType::Rgba, png::BitDepth::Eight, &data),
        )
        .unwrap();
        assert_eq!(&tile.rgba()[0..4], [1, 2, 3, 4]);
        assert_eq!(&tile.rgba()[tile.rgba().len() - 4..], [5, 6, 7, 8]);
    }

    #[test]
    fn an_rgb_picture_is_a_tile_that_is_everywhere_opaque() {
        let data: Vec<u8> = (0..16 * 16).flat_map(|_| [9, 8, 7]).collect();
        let tile = Tile::from_png(
            FILE,
            &png_with(16, 16, png::ColorType::Rgb, png::BitDepth::Eight, &data),
        )
        .unwrap();
        assert!(
            tile.rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [9, 8, 7, 255])
        );
    }

    #[test]
    fn a_file_that_is_not_a_tile_is_refused_with_the_reason() {
        let grey = png_with(
            16,
            16,
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            &[0; 256],
        );
        assert_eq!(
            refusal(&grey),
            format!(
                "{FILE}: a picture is 8-bit RGB or RGBA, and this one is 8 bits of grey (it is not converted)"
            )
        );
        let sixteen = png_with(
            16,
            16,
            png::ColorType::Rgba,
            png::BitDepth::Sixteen,
            &[0; 16 * 16 * 8],
        );
        assert_eq!(
            refusal(&sixteen),
            format!(
                "{FILE}: a picture is 8-bit RGB or RGBA, and this one is 16 bits of RGBA (it is not converted)"
            )
        );
        let not_square = png_with(
            16,
            8,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &[0; 16 * 8 * 4],
        );
        assert_eq!(
            refusal(&not_square),
            format!("{FILE}: a picture is square, and this one is 16 by 8 pixels")
        );
        for side in [4u32, 12, 24, 512] {
            let data = vec![0; (side * side * 4) as usize];
            let bytes = png_with(
                side,
                side,
                png::ColorType::Rgba,
                png::BitDepth::Eight,
                &data,
            );
            assert_eq!(
                refusal(&bytes),
                format!(
                    "{FILE}: a picture is a power of two from 8 to 256 pixels on a side, and this one is {side}"
                ),
                "{side}"
            );
        }
        assert!(
            refusal(b"GIF89a not a png")
                .starts_with(&format!("{FILE}: this is not a PNG picture: "))
        );
        assert!(refusal(&[]).starts_with(&format!("{FILE}: this is not a PNG picture: ")));
        let whole = plain_png(16, [1, 2, 3, 4]);
        assert!(
            refusal(&whole[..whole.len() - 20])
                .starts_with(&format!("{FILE}: this PNG cannot be read: "))
        );
        let huge = vec![0u8; GREATEST_FILE_BYTES + 1];
        assert_eq!(
            refusal(&huge),
            format!(
                "{FILE}: this file is {} bytes and a picture is at most {GREATEST_FILE_BYTES}",
                GREATEST_FILE_BYTES + 1
            )
        );
    }

    #[test]
    fn a_picture_that_says_it_is_enormous_is_refused_before_room_is_made_for_it() {
        // A header that claims 60000 by 60000 pixels, with no pixels: the size is refused from the header alone (this test would be killed by memory if it were not).
        let mut bytes = png_with(
            8,
            8,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &[0; 8 * 8 * 4],
        );
        // (IHDR: width and height are at bytes 16..24; the checksum is not looked at before the size.)
        bytes[16..20].copy_from_slice(&60000u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&60000u32.to_be_bytes());
        let message = refusal(&bytes);
        assert!(
            message.contains("a power of two from 8 to 256")
                || message.contains("this is not a PNG picture"),
            "{message}"
        );
    }

    fn close(found: [f32; 3], wanted: [f32; 3], within: f32) {
        for (found, wanted) in found.into_iter().zip(wanted) {
            assert!((found - wanted).abs() <= within, "{found} is not {wanted}");
        }
    }

    #[test]
    fn the_average_of_a_tile_of_one_colour_is_that_colour() {
        let tile = plain_tile(8, [191, 148, 92, 255]);
        close(
            average_color(&[(&tile, 1)]).unwrap(),
            [191.0 / 255.0, 148.0 / 255.0, 92.0 / 255.0],
            1e-6,
        );
        // Counting it more times changes nothing.
        close(
            average_color(&[(&tile, 5)]).unwrap(),
            [191.0 / 255.0, 148.0 / 255.0, 92.0 / 255.0],
            1e-6,
        );
    }

    #[test]
    fn colours_are_averaged_in_linear_light_not_as_the_bytes_are() {
        // Half black, half white: in linear light 0.5, which is 0.7354 as an sRGB value (the bytes would give 0.5).
        let mut pixels = Vec::new();
        for row in 0..8 {
            for _ in 0..8 {
                pixels.extend_from_slice(if row < 4 {
                    &[0, 0, 0, 255]
                } else {
                    &[255, 255, 255, 255]
                });
            }
        }
        let tile = Tile::from_rgba(8, pixels);
        close(average_color(&[(&tile, 1)]).unwrap(), [0.735_357; 3], 1e-4);
    }

    #[test]
    fn a_tile_counts_as_many_times_as_it_is_given_so_a_side_counts_four_times_the_top() {
        let white = plain_tile(8, [255, 255, 255, 255]);
        let black = plain_tile(8, [0, 0, 0, 255]);
        // One white and four black: a fifth of the light, which is 0.4845 as an sRGB value.
        close(
            average_color(&[(&white, 1), (&black, 4)]).unwrap(),
            [0.484_5; 3],
            1e-3,
        );
        // The same tiles counted the other way round are not the same colour.
        let other = average_color(&[(&white, 4), (&black, 1)]).unwrap();
        assert!(other[0] > 0.9, "{other:?}");
    }

    #[test]
    fn a_face_counts_as_a_face_whatever_size_its_picture_is() {
        // A top of 16 by 16 pixels, white, and a side of 8 by 8, black, counted four times: a fifth of the light, as if both were the same size (not 256 pixels
        // against 4 x 64, which would be half).
        let white = plain_tile(16, [255, 255, 255, 255]);
        let black = plain_tile(8, [0, 0, 0, 255]);
        close(
            average_color(&[(&white, 1), (&black, 4)]).unwrap(),
            [0.484_5; 3],
            1e-3,
        );
        let same_size = plain_tile(16, [0, 0, 0, 255]);
        assert_eq!(
            average_color(&[(&white, 1), (&black, 4)]),
            average_color(&[(&white, 1), (&same_size, 4)])
        );
    }

    #[test]
    fn transparent_pixels_do_not_count_and_a_tile_that_is_all_transparent_has_no_colour() {
        // Half the pixels are red but clear, the others are opaque blue: the average is blue.
        let mut pixels = Vec::new();
        for index in 0..64 {
            pixels.extend_from_slice(if index % 2 == 0 {
                &[255, 0, 0, 0]
            } else {
                &[0, 0, 255, 255]
            });
        }
        let tile = Tile::from_rgba(8, pixels);
        close(average_color(&[(&tile, 1)]).unwrap(), [0.0, 0.0, 1.0], 1e-6);
        let clear = plain_tile(8, [10, 20, 30, 0]);
        assert_eq!(average_color(&[(&clear, 1)]), None);
        assert_eq!(average_color(&[]), None);
        // A partly clear tile weighs less than an opaque one of the same colours.
        let faint = plain_tile(8, [255, 255, 255, 51]);
        let black = plain_tile(8, [0, 0, 0, 255]);
        let mixed = average_color(&[(&faint, 1), (&black, 1)]).unwrap();
        assert!(mixed[0] > 0.3 && mixed[0] < 0.6, "{mixed:?}");
    }

    #[test]
    #[should_panic(expected = "not the side of a tile")]
    fn a_tile_cannot_be_made_of_a_side_that_is_not_one() {
        Tile::from_rgba(12, vec![0; 12 * 12 * 4]);
    }
}
