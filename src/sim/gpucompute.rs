use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, Buffer, BufferDescriptor, BufferUsages, ComputePassDescriptor,
    ComputePipeline, ComputePipelineDescriptor, Device, Instance, MapMode,
    PipelineCompilationOptions, PipelineLayoutDescriptor, Queue, ShaderStages,
    util::{BufferInitDescriptor, DeviceExt},
    wgt::CommandEncoderDescriptor,
};

use crate::sim::{BurnState, CellState, SimulationFrame, SimulationParameters};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct GpuCell {
    tree: f32,
    underbrush: f32,
    burning: u32,
    padding: u32,
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
    old_params: SimulationParameters,
    queue: Queue,
    pipeline: ComputePipeline,
    device: Device,
    width: usize,
    height: usize,
    staging_buf: Buffer,
}
pub async fn create_device() -> Result<(Device, Queue), anyhow::Error> {
    let instance = Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await?;
    let downlevel_caps = adapter.get_downlevel_capabilities();
    if !downlevel_caps
        .flags
        .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
    {
        return Err(anyhow::anyhow!("adapter does not support compute shaders"));
    }
    Ok(adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("firesim compute device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        })
        .await?)
}
impl ComputeContext {
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
            pass.dispatch_workgroups(num_dispatches as u32, 1, 1);
        }
        let src_buf = if self.flipped_bufs {
            &self.buf_1
        } else {
            &self.buf_2
        };
        encoder.copy_buffer_to_buffer(src_buf, 0, &self.staging_buf, 0, src_buf.size());

        self.queue.submit(std::iter::once(encoder.finish()));

        self.staging_buf.map_async(MapMode::Read, .., |_| ());

        self.flipped_bufs = !self.flipped_bufs;
    }
    fn update_params(&mut self, new: SimulationParameters) {
        self.old_params = new;
        self.queue
            .write_buffer(&self.params_buf, 0, bytemuck::bytes_of(&new));
    }
    pub fn create(
        device: Device,
        queue: Queue,
        start: SimulationFrame,
        parameters: SimulationParameters,
    ) -> Result<Self, anyhow::Error> {
        let start_data: Vec<_> = start
            .grid
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
        // Create a buffer with the data we want to process on the GPU.
        //
        // `create_buffer_init` is a utility provided by `wgpu::util::DeviceExt` which simplifies creating
        // a buffer with some initial data.
        //
        // We use the `bytemuck` crate to cast the slice of f32 to a &[u8] to be uploaded to the GPU.
        let buf_1 = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("cells buffer 1"),
            contents: bytemuck::cast_slice(&start_data),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });

        // Now we create a buffer to store the output data.
        let buf_2 = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cells buffer 2"),
            size: buf_1.size(),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let params_buf = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("simulation parameters buffer"),
            contents: bytemuck::bytes_of(&parameters),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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
            bind_group_layouts: &[&cells_bg_layout, &params_bg_layout, &size_bg_layout],
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
        })
    }
    pub async fn get_latest(&self) -> SimulationFrame {
        let (tx, rx) = futures_intrusive::channel::shared::oneshot_channel();

        // 1. Request the map
        self.staging_buf
            .map_async(wgpu::MapMode::Read, .., move |res| {
                tx.send(res).unwrap();
            });

        // 2. Tell the GPU to catch up
        // Using Wait ensures that the callback above WILL be called before this returns
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU device lost");

        // 3. Wait for the channel to confirm mapping is successful
        rx.receive()
            .await
            .unwrap()
            .expect("Failed to map staging buffer");
        let buf_view = self.staging_buf.get_mapped_range(..);
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
            width: self.width,
            height: self.height,
        };
        drop(buf_view);
        self.staging_buf.unmap();
        frame
    }
}
