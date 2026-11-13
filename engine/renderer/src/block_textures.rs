//! The pictures of blocks on the GPU: for each size of picture in use a texture array with a layer for each picture and a chain of smaller pictures under it, and the
//! sampler that reads them (`docs/specs/block-textures-spec.md`, section 6).
//!
//! The pictures are not sent as images but as pixels, so nothing here decodes anything. A picture is nearest when it is bigger on the screen than in the file (a
//! block is blocky) and smoothly mixed down when it is smaller, so that a far block does not shimmer.

use wgpu::util::DeviceExt;

/// The most different sizes of picture there can be at once: one texture array each, bound in the voxel shader.
pub const MOST_PICTURE_SIZES: usize = 4;
/// The most pictures of one size (the layers of its array).
pub const MOST_PICTURES_OF_A_SIZE: usize = 256;

/// The pictures of one size: their side in pixels, and the pixels of each, 8-bit sRGB RGBA row after row from the top left (`size * size * 4` bytes).
#[derive(Clone, Debug, Default)]
pub struct PictureSize<'a> {
    pub size: u32,
    pub tiles: Vec<&'a [u8]>,
    /// The specular companion (LabPBR `_s`) of each picture, in the same order and of the same size, if it has one; or empty when none has. Its pixels are data (8-bit,
    /// not sRGB): the alpha is the emission, where 255 means none and 0 to 254 is the amount of the pixel's own light.
    pub specular: Vec<Option<&'a [u8]>>,
    /// The normal companion (LabPBR `_n`) of each picture, as `specular`: its pixels are data (8-bit, not sRGB), the red and green the tilt of the face there (128 is none,
    /// 0 and 255 a tilt of 1 either way) and the rest not read yet.
    pub normal: Vec<Option<&'a [u8]>>,
}

/// The texture arrays of the pictures and their sampler, for the voxel shader.
pub(crate) struct BlockTextures {
    /// A view of the array of each size, smallest first; a size that is not in use is an array of one magenta pixel that nothing reads.
    pub views: [wgpu::TextureView; MOST_PICTURE_SIZES],
    /// A view of the array of the specular pictures of each size, with the layers of the pictures; a size not in use is one pixel of nothing.
    pub specular_views: [wgpu::TextureView; MOST_PICTURE_SIZES],
    /// The same of the normal pictures.
    pub normal_views: [wgpu::TextureView; MOST_PICTURE_SIZES],
    pub sampler: wgpu::Sampler,
}

impl BlockTextures {
    /// Panics if there are more sizes or pictures than there can be, or a picture is not as big as its size says.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, sizes: &[PictureSize<'_>]) -> Self {
        assert!(
            sizes.len() <= MOST_PICTURE_SIZES,
            "at most {MOST_PICTURE_SIZES} sizes of picture can be drawn"
        );
        // (A texture of one pixel stands in for every size that is not in use: nothing reads it, and one serves all four.)
        let mut stand_in: Option<wgpu::TextureView> = None;
        let views = std::array::from_fn(|class| match sizes.get(class) {
            Some(size) => array_of(device, queue, size),
            None => stand_in
                .get_or_insert_with(|| stand_in_array(device, queue))
                .clone(),
        });
        // (The same stand-in serves the specular arrays of the sizes not in use: nothing reads it there either, and a renderer with no pictures makes no texture more than it did
        // before there were specular pictures, which the integrated AMD GPU's first frames are sensitive to: `docs/test-flakes.md`.)
        let specular_views = std::array::from_fn(|class| match sizes.get(class) {
            Some(size) => specular_array_of(device, queue, size),
            None => stand_in
                .get_or_insert_with(|| stand_in_array(device, queue))
                .clone(),
        });
        let normal_views = std::array::from_fn(|class| match sizes.get(class) {
            Some(size) => normal_array_of(device, queue, size),
            None => stand_in
                .get_or_insert_with(|| stand_in_array(device, queue))
                .clone(),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("block pictures"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            views,
            specular_views,
            normal_views,
            sampler,
        }
    }
}

/// An array of one picture of one pixel that nothing reads.
fn stand_in_array(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    array_of(
        device,
        queue,
        &PictureSize {
            size: 1,
            tiles: vec![&[255, 0, 255, 255]],
            ..PictureSize::default()
        },
    )
}

fn array_of(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: &PictureSize<'_>,
) -> wgpu::TextureView {
    assert!(
        size.size.is_power_of_two(),
        "a size of picture is a power of two, not {}",
        size.size
    );
    assert!(
        (1..=MOST_PICTURES_OF_A_SIZE).contains(&size.tiles.len()),
        "a size of picture has 1 to {MOST_PICTURES_OF_A_SIZE} pictures, not {}",
        size.tiles.len()
    );
    let bytes = size.size as usize * size.size as usize * 4;
    let mut data = Vec::new();
    for tile in &size.tiles {
        assert_eq!(tile.len(), bytes, "a picture has four bytes for each pixel");
        // (Layer by layer, and in each the picture and then the smaller ones.)
        for level in mip_chain(size.size, tile) {
            data.extend_from_slice(&level);
        }
    }
    layered(
        device,
        queue,
        "block pictures",
        (size.size, size.tiles.len()),
        wgpu::TextureFormat::Rgba8UnormSrgb,
        &data,
    )
}

/// The specular companions of the pictures of one size as an array with the layers of the pictures (a picture with none has the pixel of no emission and nothing else), in
/// the form the shader reads: see [`engine_specular`]. Linear data, not colours.
fn specular_array_of(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: &PictureSize<'_>,
) -> wgpu::TextureView {
    companion_array_of(
        device,
        queue,
        size,
        &size.specular,
        &Companion {
            kind: "specular",
            label: "block specular pictures",
            none: NO_SPECULAR,
            engine_form: engine_specular,
        },
    )
}

/// The normal companions of the pictures of one size as an array with the layers of the pictures (a picture with none has the pixel of a flat face). Linear data, not
/// colours, and a smaller level is the plain average of the four pixels it stands for, which is the right one for normals too: the average of tilts is a smaller tilt, as the
/// bumps of a far picture add up to a flatter face.
fn normal_array_of(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: &PictureSize<'_>,
) -> wgpu::TextureView {
    companion_array_of(
        device,
        queue,
        size,
        &size.normal,
        &Companion {
            kind: "normal",
            label: "block normal pictures",
            none: NO_NORMAL,
            engine_form: <[u8]>::to_vec,
        },
    )
}

/// What a kind of companion picture is, to make an array of them: its name (for the messages), its label, the pixel of a picture that has none, and how the pixels of a file
/// become the ones the shader reads.
struct Companion {
    kind: &'static str,
    label: &'static str,
    none: [u8; 4],
    engine_form: fn(&[u8]) -> Vec<u8>,
}

/// The companions of the pictures of one size (`companions`, one for each picture or empty) as an array with the layers of the pictures, a picture with none having the
/// pixel `none` all over, and each with its levels of smaller pictures under it.
fn companion_array_of(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    size: &PictureSize<'_>,
    companions: &[Option<&[u8]>],
    kind: &Companion,
) -> wgpu::TextureView {
    assert!(
        companions.is_empty() || companions.len() == size.tiles.len(),
        "the {} pictures are one for each picture, or none",
        kind.kind
    );
    let bytes = size.size as usize * size.size as usize * 4;
    let mut data = Vec::new();
    for layer in 0..size.tiles.len() {
        let pixels = match companions.get(layer).copied().flatten() {
            Some(pixels) => {
                assert_eq!(
                    pixels.len(),
                    bytes,
                    "a picture has four bytes for each pixel"
                );
                (kind.engine_form)(pixels)
            }
            None => kind.none.repeat(bytes / 4),
        };
        for level in data_mip_chain(size.size, &pixels) {
            data.extend_from_slice(&level);
        }
    }
    layered(
        device,
        queue,
        kind.label,
        (size.size, size.tiles.len()),
        wgpu::TextureFormat::Rgba8Unorm,
        &data,
    )
}

/// What a pixel with no specular picture is: smoothness, reflectance and porosity nothing, and no emission (see [`engine_specular`]).
const NO_SPECULAR: [u8; 4] = [0, 0, 0, 0];

/// What a pixel with no normal picture is: no tilt (128 is the middle of the red and green bytes, see `picture_bump` in `pictures_on.wgsl`), and the ambient occlusion and the
/// height of LabPBR at their highest, which are not read.
const NO_NORMAL: [u8; 4] = [128, 128, 255, 255];

/// A texture array of `(side, layers)`, with the chain of smaller levels under each layer already in `data` (layer after layer).
fn layered(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &'static str,
    (side, layers): (u32, usize),
    format: wgpu::TextureFormat,
    data: &[u8],
) -> wgpu::TextureView {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: layers as u32,
            },
            mip_level_count: side.ilog2() + 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        data,
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some(label),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// A LabPBR specular picture as the shader reads it: the same pixels, except that the alpha, where 255 means "no emission" and 0 to 254 is the amount, becomes the amount
/// alone (255 becomes 0), so that the average of four pixels is the average of their emission, as a smaller level of the picture needs.
pub(crate) fn engine_specular(rgba: &[u8]) -> Vec<u8> {
    let mut pixels = rgba.to_vec();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        if pixel[3] == 255 {
            pixel[3] = 0;
        }
    }
    pixels
}

/// The levels under a picture of data, down to one pixel: each pixel of a smaller one is the plain average of the four it stands for in each channel, rounded (no channel is a
/// colour, and none is how much of the pixel is there).
pub(crate) fn data_mip_chain(size: u32, rgba: &[u8]) -> Vec<Vec<u8>> {
    let mut levels = vec![rgba.to_vec()];
    let mut side = size as usize;
    while side > 1 {
        let previous = levels.last().expect("there is a level");
        let next_side = side / 2;
        let mut next = Vec::with_capacity(next_side * next_side * 4);
        for y in 0..next_side {
            for x in 0..next_side {
                for channel in 0..4 {
                    let sum: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)]
                        .iter()
                        .map(|(dx, dy)| {
                            u32::from(previous[((2 * y + dy) * side + 2 * x + dx) * 4 + channel])
                        })
                        .sum();
                    next.push(((sum + 2) / 4) as u8);
                }
            }
        }
        levels.push(next);
        side = next_side;
    }
    levels
}

/// The picture and the smaller ones under it, down to one pixel: each pixel of a smaller one is the average of the four it stands for, worked out in linear light (the
/// way the screen blends) with each pixel counting as much as it is opaque (so the black that clear pixels are stored as does not darken what is beside them), and the
/// opacity averaged as it is. (Four clear pixels have nothing to weigh by, and give the plain average of their colours.)
pub(crate) fn mip_chain(size: u32, rgba: &[u8]) -> Vec<Vec<u8>> {
    let linear_of: Vec<f32> = (0..=255u32)
        .map(|byte| {
            let value = byte as f64 / 255.0;
            if value <= 0.04045 {
                (value / 12.92) as f32
            } else {
                libm::pow((value + 0.055) / 1.055, 2.4) as f32
            }
        })
        .collect();
    let srgb_byte = |linear: f32| -> u8 {
        let linear = f64::from(linear);
        let value = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * libm::pow(linear, 1.0 / 2.4) - 0.055
        };
        (value * 255.0).round().clamp(0.0, 255.0) as u8
    };
    let mut levels = vec![rgba.to_vec()];
    let mut side = size as usize;
    while side > 1 {
        let previous = levels.last().expect("there is a level");
        let next_side = side / 2;
        let mut next = Vec::with_capacity(next_side * next_side * 4);
        for y in 0..next_side {
            for x in 0..next_side {
                let mut weighted = [0.0f32; 3];
                let mut plain = [0.0f32; 3];
                let mut weight = 0.0f32;
                let mut opacity = 0.0f32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let at = ((2 * y + dy) * side + 2 * x + dx) * 4;
                    let alpha = f32::from(previous[at + 3]) / 255.0;
                    for channel in 0..3 {
                        let linear = linear_of[usize::from(previous[at + channel])];
                        weighted[channel] += linear * alpha;
                        plain[channel] += linear;
                    }
                    weight += alpha;
                    opacity += f32::from(previous[at + 3]);
                }
                if weight > 0.0 {
                    next.extend(weighted.map(|sum| srgb_byte(sum / weight)));
                } else {
                    next.extend(plain.map(|sum| srgb_byte(sum / 4.0)));
                }
                next.push((opacity / 4.0).round() as u8);
            }
        }
        levels.push(next);
        side = next_side;
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(size: u32, rgba: [u8; 4]) -> Vec<u8> {
        (0..size * size).flat_map(|_| rgba).collect()
    }

    #[test]
    fn a_picture_has_a_level_for_every_halving_down_to_one_pixel() {
        for (size, levels) in [(8u32, 4usize), (16, 5), (64, 7), (256, 9)] {
            let chain = mip_chain(size, &plain(size, [1, 2, 3, 255]));
            assert_eq!(chain.len(), levels, "{size}");
            let mut side = size as usize;
            for level in &chain {
                assert_eq!(level.len(), side * side * 4);
                side = (side / 2).max(1);
            }
            assert_eq!(chain.last().unwrap().len(), 4);
        }
        assert_eq!(mip_chain(1, &[9, 9, 9, 9]), [vec![9, 9, 9, 9]]);
    }

    #[test]
    fn a_picture_of_one_colour_stays_that_colour_all_the_way_down() {
        for colour in [
            [0, 0, 0, 255],
            [255, 255, 255, 255],
            [191, 148, 92, 255],
            [37, 47, 229, 128],
        ] {
            for level in mip_chain(16, &plain(16, colour)) {
                for pixel in level.as_chunks::<4>().0 {
                    for channel in 0..4 {
                        assert!(
                            pixel[channel].abs_diff(colour[channel]) <= 1,
                            "{colour:?}: {pixel:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn black_beside_white_averages_in_linear_light_not_as_bytes() {
        // Half the pixels black and half white: the screen would blend them to a linear 0.5, which is the sRGB byte 188, not 128.
        let mut picture = Vec::new();
        for pixel in 0..4 {
            picture.extend_from_slice(&if pixel % 2 == 0 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            });
        }
        let chain = mip_chain(2, &picture);
        assert_eq!(chain.len(), 2);
        for channel in 0..3 {
            assert!(chain[1][channel].abs_diff(188) <= 1, "{:?}", chain[1]);
        }
        assert_eq!(chain[1][3], 255);
    }

    #[test]
    fn each_pixel_of_a_smaller_level_is_the_four_it_stands_for_and_opacity_is_averaged_as_it_is() {
        // Four pixels: opacities 255, 255, 0, 0 average to 128 (127.5 rounded).
        let mut picture = Vec::new();
        for pixel in 0..4 {
            picture.extend_from_slice(&[10, 20, 30, if pixel < 2 { 255 } else { 0 }]);
        }
        let chain = mip_chain(2, &picture);
        assert_eq!(chain[1], [10, 20, 30, 128]);
        // The top left four of a 4 by 4 become the top left pixel, and no others do.
        let mut four = plain(4, [0, 0, 0, 255]);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            four[(y * 4 + x) * 4..(y * 4 + x) * 4 + 3].copy_from_slice(&[255, 255, 255]);
        }
        let level = &mip_chain(4, &four)[1];
        assert_eq!(&level[0..4], [255, 255, 255, 255]);
        assert_eq!(&level[4..8], [0, 0, 0, 255]);
        assert_eq!(&level[8..12], [0, 0, 0, 255]);
        assert_eq!(&level[12..16], [0, 0, 0, 255]);
    }

    #[test]
    fn the_colour_of_a_clear_pixel_does_not_tint_the_pixels_around_it_in_a_smaller_level() {
        // The pictures of leaves and glass store black in their clear pixels. One opaque red pixel and three clear black ones: the smaller pixel is a quarter as opaque, and
        // is still red, not a dark red.
        let mut picture = vec![255, 0, 0, 255];
        for _ in 0..3 {
            picture.extend_from_slice(&[0, 0, 0, 0]);
        }
        assert_eq!(mip_chain(2, &picture)[1], [255, 0, 0, 64]);
        // The same with the opaque pixel in each of the four places.
        for place in 0..4 {
            let mut moved = vec![0u8; 16];
            moved[place * 4..place * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
            assert_eq!(mip_chain(2, &moved)[1], [255, 0, 0, 64], "{place}");
        }
    }

    #[test]
    fn a_pixel_counts_in_proportion_to_its_opacity_in_the_colour_of_a_smaller_level() {
        // White, fully opaque, and black at opacity 128 (and two clear ones): in linear light white weighs 255 and black 128, so the colour is 255 / 383 = 0.666 of white,
        // which is the sRGB byte 213.
        let picture = [255, 255, 255, 255, 0, 0, 0, 128, 99, 99, 99, 0, 7, 7, 7, 0];
        let level = mip_chain(2, &picture)[1].clone();
        for channel in 0..3 {
            assert!(level[channel].abs_diff(213) <= 1, "{level:?}");
        }
        assert_eq!(level[3], 96);
    }

    #[test]
    fn four_clear_pixels_give_a_clear_pixel_of_their_plain_average_colour() {
        // There is nothing to weigh by, so the colours count alike (and the result is clear whatever it is).
        let picture = [10, 20, 30, 0, 10, 20, 30, 0, 10, 20, 30, 0, 10, 20, 30, 0];
        assert_eq!(mip_chain(2, &picture)[1], [10, 20, 30, 0]);
    }

    // ---- the specular companions: data, not colours ----

    #[test]
    fn a_specular_pictures_alpha_of_255_means_no_emission_and_becomes_0_and_nothing_else_changes() {
        let picture = [1, 2, 3, 255, 4, 5, 6, 254, 7, 8, 9, 0, 255, 255, 255, 255];
        assert_eq!(
            engine_specular(&picture),
            [1, 2, 3, 0, 4, 5, 6, 254, 7, 8, 9, 0, 255, 255, 255, 0]
        );
    }

    #[test]
    fn the_levels_of_data_are_the_plain_average_of_four_in_every_channel_with_no_weighing_by_the_alpha()
     {
        // Alpha 0 on three of the pixels must not take the fourth's colour out of the average, nor the alpha count as a share of covering.
        let picture = [
            100, 0, 255, 200, //
            0, 40, 255, 0, //
            0, 0, 255, 0, //
            0, 0, 255, 0,
        ];
        let chain = data_mip_chain(2, &picture);
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0], picture);
        assert_eq!(chain[1], [25, 10, 255, 50]);
        // Rounded to the nearest (half up), and the levels go down to one pixel.
        let odd = [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0];
        assert_eq!(data_mip_chain(2, &odd)[1], [1, 1, 1, 1]);
        let eight: Vec<u8> = (0..8 * 8).flat_map(|_| [9, 9, 9, 9]).collect();
        let sizes: Vec<usize> = data_mip_chain(8, &eight).iter().map(Vec::len).collect();
        assert_eq!(sizes, [256, 64, 16, 4]);
    }

    #[test]
    fn the_emission_of_a_smaller_level_is_the_average_of_the_emission_it_stands_for_with_none_counting_as_zero()
     {
        // A pixel of full emission (254) among three with none (255) is a quarter of the emission, not three quarters of 255 and a quarter of 254.
        let picture = engine_specular(&[0, 0, 0, 254, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
        let level = &data_mip_chain(2, &picture)[1];
        assert_eq!(level[3], 64, "(254 + 0 + 0 + 0 + 2) / 4");
    }

    #[test]
    fn the_tilts_of_a_smaller_level_of_a_normal_picture_are_the_average_of_the_tilts_so_opposite_bumps_flatten()
     {
        // Two pixels tilted right by 127/127 and two tilted left by 127/127 (bytes 255 and 1 on either side of 128) stand for a flat place from far off; and a picture that is
        // tilted the same everywhere keeps its tilt.
        let opposite = [
            255, 128, 255, 255, //
            1, 128, 255, 255, //
            255, 128, 255, 255, //
            1, 128, 255, 255,
        ];
        assert_eq!(data_mip_chain(2, &opposite)[1], [128, 128, 255, 255]);
        let same: Vec<u8> = (0..4).flat_map(|_| [200, 90, 255, 255]).collect();
        assert_eq!(data_mip_chain(2, &same)[1], [200, 90, 255, 255]);
        // The pixel a picture with no normal picture has is flat to the shader (128 is no tilt), so that it is the same at every level.
        let none: Vec<u8> = (0..4).flat_map(|_| NO_NORMAL).collect();
        assert_eq!(data_mip_chain(2, &none)[1], NO_NORMAL);
    }
}
