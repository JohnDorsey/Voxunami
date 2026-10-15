use wgpu::{Adapter, Backends, DeviceType};

/// Which kind of graphics adapter to prefer when a machine has several.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterPreference {
    /// A discrete GPU if there is one, otherwise an integrated GPU.
    HighPerformance,
    /// Only an integrated GPU; used to check the game runs on minimum hardware.
    Integrated,
}

#[derive(Debug, thiserror::Error)]
pub enum GraphicsError {
    #[error("no suitable graphics adapter found ({0})")]
    NoAdapter(String),
    #[error("could not create a graphics device on {adapter}: {source}")]
    Device {
        adapter: String,
        source: wgpu::RequestDeviceError,
    },
}

/// A graphics device and queue, with the adapter and instance they came from.
#[derive(Clone, Debug)]
pub struct GraphicsContext {
    pub instance: wgpu::Instance,
    pub adapter: Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// How wgpu describes the adapter in use, for logs and reports.
pub fn describe(adapter: &Adapter) -> String {
    let info = adapter.get_info();
    format!("{} ({:?}, {:?})", info.name, info.device_type, info.backend)
}

pub fn new_instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle())
}

fn backend_rank(adapter: &Adapter) -> u8 {
    match adapter.get_info().backend {
        wgpu::Backend::Vulkan => 0,
        wgpu::Backend::Dx12 => 1,
        wgpu::Backend::Metal => 2,
        _ => 3,
    }
}

impl GraphicsContext {
    /// A context without a window, for rendering to images and for tests.
    pub fn headless(preference: AdapterPreference) -> Result<Self, GraphicsError> {
        Self::new(new_instance(), preference, None)
    }

    /// Picks an adapter for `preference` (one that can present to `surface`, if given) and opens a
    /// device on it.
    pub fn new(
        instance: wgpu::Instance,
        preference: AdapterPreference,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, GraphicsError> {
        let mut candidates: Vec<Adapter> =
            pollster::block_on(instance.enumerate_adapters(Backends::PRIMARY))
                .into_iter()
                .filter(|adapter| {
                    surface.is_none_or(|surface| adapter.is_surface_supported(surface))
                })
                .filter(|adapter| match preference {
                    AdapterPreference::HighPerformance => {
                        !matches!(adapter.get_info().device_type, DeviceType::Cpu)
                    }
                    AdapterPreference::Integrated => {
                        adapter.get_info().device_type == DeviceType::IntegratedGpu
                    }
                })
                .collect();
        let type_rank = |adapter: &Adapter| match adapter.get_info().device_type {
            DeviceType::DiscreteGpu => 0,
            DeviceType::IntegratedGpu => 1,
            DeviceType::VirtualGpu => 2,
            _ => 3,
        };
        candidates.sort_by_key(|adapter| (type_rank(adapter), backend_rank(adapter)));
        let adapter = candidates
            .into_iter()
            .next()
            .ok_or_else(|| GraphicsError::NoAdapter(format!("{preference:?}")))?;
        Self::with_adapter(instance, adapter)
    }

    /// Opens a device on a specific adapter.
    pub fn with_adapter(instance: wgpu::Instance, adapter: Adapter) -> Result<Self, GraphicsError> {
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("voxel device"),
            ..Default::default()
        }))
        .map_err(|source| GraphicsError::Device {
            adapter: describe(&adapter),
            source,
        })?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// One context for every physical (non-software) adapter and backend combination found.
    pub fn all_physical_adapters() -> Vec<Self> {
        let instance = new_instance();
        pollster::block_on(instance.enumerate_adapters(Backends::PRIMARY))
            .into_iter()
            .filter(|adapter| !matches!(adapter.get_info().device_type, DeviceType::Cpu))
            .filter_map(|adapter| Self::with_adapter(instance.clone(), adapter).ok())
            .collect()
    }
}
