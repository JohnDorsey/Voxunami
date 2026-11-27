//! The wasm module: `init(seed_low, seed_high)` makes the generator and returns the address of the output buffer; `generate(x, y, z)` fills that buffer
//! with the 32,768 materials of a brick (u32, little endian). No imports: a module that asks the host for nothing.

use std::cell::RefCell;

use spike_generator::SpikeGenerator;
use voxel_space::BRICK_VOXEL_COUNT;

thread_local! {
    static STATE: RefCell<Option<(SpikeGenerator, Vec<u32>)>> = const { RefCell::new(None) };
}

#[unsafe(no_mangle)]
pub extern "C" fn init(seed_low: u32, seed_high: u32) -> u32 {
    let seed = u64::from(seed_low) | (u64::from(seed_high) << 32);
    STATE.with(|state| {
        let buffer = vec![0u32; BRICK_VOXEL_COUNT];
        let address = buffer.as_ptr() as usize as u32;
        *state.borrow_mut() = Some((SpikeGenerator::new(seed), buffer));
        address
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn generate(x: i32, y: i32, z: i32) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let (generator, buffer) = state.as_mut().expect("init was called");
        generator.materials(x, y, z, buffer);
    });
}
