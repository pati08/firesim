use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use bytemuck::{Pod, Zeroable};
use watch::WatchSender;
use wgpu::{
    Adapter, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, Buffer, BufferDescriptor, BufferUsages, ComputePassDescriptor,
    ComputePipeline, ComputePipelineDescriptor, Device, Instance, MapMode,
    PipelineCompilationOptions, PipelineLayoutDescriptor, Queue, ShaderStages,
    util::{BufferInitDescriptor, DeviceExt},
    wgt::CommandEncoderDescriptor,
};

use crate::sim::{BurnState, CellState, SimulationFrame, SimulationParameters};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct GpuCell {
    pub tree: f32,
    pub underbrush: f32,
    pub burning: u32,
    pub padding: u32,
}

/// Shared GPU resources (device, queue, instance)
pub struct GpuResources {
    pub instance: Instance,
    pub adapter: Adapter,
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
}

impl GpuResources {
    pub async fn new() -> Result<Self, anyhow::Error> {
        let instance = Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await?;

        let downlevel_caps = adapter.get_downlevel_capabilities();
        if !downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            return Err(anyhow::anyhow!("adapter does not support compute shaders"));
        }

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("firesim device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await?;

        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }

    /// Create GPU resources with a compatible surface for rendering
    pub async fn new_with_surface(
        surface: &wgpu::Surface<'_>,
    ) -> Result<Self, anyhow::Error> {
        let instance = Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(surface),
            })
            .await?;

        let downlevel_caps = adapter.get_downlevel_capabilities();
        if !downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            return Err(anyhow::anyhow!("adapter does not support compute shaders"));
        }

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("firesim device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await?;

        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }
}

pub struct ComputeContext {
    buf_1: Buffer,
    buf_2: Buffer,
    cells_bg: BindGroup,
    cells_bg_rev: BindGroup,
    params_bind_group: BindGroup,
    params_buf: Buffer,
    size_bind_group: BindGroup,
    flipped_bufs: bool,
    time_bind_group: BindGroup,
    time_buf: Buffer,
    old_params: SimulationParameters,
    queue: Arc<Queue>,
    pipeline: ComputePipeline,
    device: Arc<Device>,
    width: usize,
    height: usize,
    staging_buf: Buffer,
    staging_mapped: Arc<AtomicBool>,
    steps: Arc<AtomicU32>,
    frame_tx: WatchSender<SimulationFrame>,
}

async fn get_adapter() -> Result<Adapter, anyhow::Error> {
    let instance = Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: true,
            compatible_surface: None,
        })
        .await?;
    Ok(adapter)
}

pub async fn create_device() -> Result<(Device, Queue), anyhow::Error> {
    let adapter = get_adapter().await?;
    let downlevel_caps = adapter.get_downlevel_capabilities();
    if !downlevel_caps
        .flags
        .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
    {
        return Err(anyhow::anyhow!("adapter does not support compute shaders"));
    }
    let device = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("firesim compute device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        })
        .await?;
    Ok(device)
}

impl ComputeContext {
    /// Get the current output buffer (the one that was last written to)
    /// This is the buffer that should be used for rendering
    pub fn current_output_buffer(&self) -> &Buffer {
        if self.flipped_bufs {
            &self.buf_1
        } else {
            &self.buf_2
        }
    }

    /// Get the current input buffer
    pub fn current_input_buffer(&self) -> &Buffer {
        if self.flipped_bufs {
            &self.buf_2
        } else {
            &self.buf_1
        }
    }

    /// Get both buffers for creating render bind groups
    pub fn get_buffers(&self) -> (&Buffer, &Buffer) {
        (&self.buf_1, &self.buf_2)
    }

    /// Returns true if buffers are flipped (buf_2 is input, buf_1 is output)
    pub fn is_flipped(&self) -> bool {
        self.flipped_bufs
    }

    /// Get grid dimensions
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Get shared device reference
    pub fn device(&self) -> &Arc<Device> {
        &self.device
    }

    /// Get shared queue reference
    pub fn queue(&self) -> &Arc<Queue> {
        &self.queue
    }

    pub fn compute_step(&mut self, parameters: SimulationParameters) {
        if parameters != self.old_params {
            self.update_params(parameters);
        }
        let num_dispatches = self.buf_1.size().div_ceil(64);
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("simulation step encoder"),
            });
        self.queue.write_buffer(
            &self.time_buf,
            0,
            bytemuck::bytes_of(&self.steps.load(Ordering::Relaxed)),
        );
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("simulation step compute pass"),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(
                0,
                if self.flipped_bufs {
                    &self.cells_bg_rev
                } else {
                    &self.cells_bg
                },
                &[],
            );
            pass.set_bind_group(1, &self.params_bind_group, &[]);
            pass.set_bind_group(2, &self.size_bind_group, &[]);
            pass.set_bind_group(3, &self.time_bind_group, &[]);
            pass.dispatch_workgroups(num_dispatches as u32, 1, 1);
        }
        if !self.staging_mapped.load(Ordering::SeqCst) {
            let src_buf = if self.flipped_bufs {
                &self.buf_1
            } else {
                &self.buf_2
            };
            encoder.copy_buffer_to_buffer(src_buf, 0, &self.staging_buf, 0, src_buf.size());
        }

        self.queue.submit(std::iter::once(encoder.finish()));

        self.flipped_bufs = !self.flipped_bufs;
        self.steps.fetch_add(1, Ordering::Relaxed);
    }

    fn update_params(&mut self, new: SimulationParameters) {
        self.old_params = new;
        self.queue
            .write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&new));
    }

    /// Create a compute context using shared GPU resources
    pub fn create_with_resources(
        resources: &GpuResources,
        start: SimulationFrame,
        parameters: SimulationParameters,
        frame_tx: WatchSender<SimulationFrame>,
    ) -> Result<Self, anyhow::Error> {
        Self::create_internal(
            Arc::clone(&resources.device),
            Arc::clone(&resources.queue),
            start,
            parameters,
            frame_tx,
        )
    }

    pub fn create(
        device: Device,
        queue: Queue,
        start: SimulationFrame,
        parameters: SimulationParameters,
        frame_tx: WatchSender<SimulationFrame>,
    ) -> Result<Self, anyhow::Error> {
        Self::create_internal(Arc::new(device), Arc::new(queue), start, parameters, frame_tx)
    }

    fn create_internal(
        device: Arc<Device>,
        queue: Arc<Queue>,
        start: SimulationFrame,
        parameters: SimulationParameters,
        frame_tx: WatchSender<SimulationFrame>,
    ) -> Result<Self, anyhow::Error> {
        let start_data: Vec<_> = start
            .grid
            .clone()
            .into_iter()
            .map(|i| GpuCell {
                burning: match i.burning {
                    BurnState::NotBurning => 0,
                    BurnState::Burning { ticks_remaining } => ticks_remaining,
                },
                tree: if i.tree { 1.0 } else { 0.0 },
                underbrush: i.underbrush,
                padding: 0,
            })
            .collect();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("simulation compute shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("./shader.wgsl").into()),
        });

        // Create buffers with STORAGE usage for compute and rendering
        let buf_1 = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("cells buffer 1"),
            contents: bytemuck::cast_slice(&start_data),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        });

        let buf_2 = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cells buffer 2"),
            size: buf_1.size(),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let params_buf = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("simulation parameters buffer"),
            contents: bytemuck::bytes_of(&parameters),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let cells_bg_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("cells bind group layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let cells_bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("cells bind group (buf1 -> buf2)"),
            layout: &cells_bg_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: buf_1.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: buf_2.as_entire_binding(),
                },
            ],
        });

        let cells_bg_rev = device.create_bind_group(&BindGroupDescriptor {
            label: Some("cells bind group (buf2 -> buf1)"),
            layout: &cells_bg_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: buf_2.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: buf_1.as_entire_binding(),
                },
            ],
        });

        let params_bg_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("parameters bind group layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let params_bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("parameters bind group"),
            layout: &params_bg_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: params_buf.as_entire_binding(),
            }],
        });

        let time_buf = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("time buffer"),
            contents: &[0, 0, 0, 0],
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let time_bg_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("time bind group layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let time_bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("time bind group"),
            layout: &time_bg_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: time_buf.as_entire_binding(),
            }],
        });

        let size_bg_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grid size bind group layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let size_buf = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("grid size buffer"),
            contents: bytemuck::cast_slice(&[start.width as u32, start.height as u32]),
            usage: BufferUsages::UNIFORM,
        });

        let size_bg = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grid size bind group"),
            layout: &size_bg_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: size_buf.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("simulation pipeline layout"),
            bind_group_layouts: &[
                &cells_bg_layout,
                &params_bg_layout,
                &size_bg_layout,
                &time_bg_layout,
            ],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("simulation compute pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: None,
            compilation_options: PipelineCompilationOptions::default(),
            cache: None,
        });

        let staging_buf = device.create_buffer(&BufferDescriptor {
            label: Some("staging buffer"),
            size: buf_1.size(),
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            buf_1,
            buf_2,
            cells_bg,
            cells_bg_rev,
            params_buf,
            params_bind_group: params_bg,
            size_bind_group: size_bg,
            flipped_bufs: false,
            old_params: parameters,
            queue,
            device,
            pipeline,
            width: start.width,
            height: start.height,
            staging_buf,
            staging_mapped: Arc::new(AtomicBool::new(false)),
            time_bind_group: time_bg,
            time_buf,
            steps: Arc::new(AtomicU32::new(0)),
            frame_tx,
        })
    }

    pub fn send_latest(&self) {
        if !self
            .staging_mapped
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            let tx = self.frame_tx.clone();
            let buf = self.staging_buf.clone();
            let width = self.width;
            let height = self.height;
            self.staging_mapped.store(true, Ordering::SeqCst);
            let staging_mapped = Arc::clone(&self.staging_mapped);
            self.staging_buf.map_async(MapMode::Read, .., move |v| {
                if v.is_err() {
                    log::error!("map error");
                    return;
                }
                let buf_view = buf.get_mapped_range(..);
                let cells: &[GpuCell] = bytemuck::cast_slice(buf_view.as_ref());
                let frame = SimulationFrame {
                    grid: cells
                        .iter()
                        .map(|i| CellState {
                            burning: if i.burning > 0 {
                                BurnState::Burning {
                                    ticks_remaining: i.burning,
                                }
                            } else {
                                BurnState::NotBurning
                            },
                            underbrush: i.underbrush,
                            tree: i.tree > 0.0,
                        })
                        .collect(),
                    width,
                    height,
                };
                drop(buf_view);
                buf.unmap();
                staging_mapped.store(false, Ordering::SeqCst);
                tx.send(frame);
            });
        }
    }
}
