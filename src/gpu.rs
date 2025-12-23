//! Integrated GPU context that manages both compute simulation and rendering
//!
//! This module provides `GpuSimRenderer` which combines the compute shader simulation
//! with GPU-accelerated rendering, sharing the same device, queue, and cell buffers.

use std::sync::Arc;

use js_sys::Date;

use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, Buffer, BufferUsages, CommandEncoderDescriptor, Device, FragmentState,
    Instance, LoadOp, MultisampleState, Operations, PipelineLayoutDescriptor, PrimitiveState,
    Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, ShaderStages, StoreOp, Surface, SurfaceConfiguration, TextureUsages,
    TextureViewDescriptor, VertexState,
    util::{BufferInitDescriptor, DeviceExt},
};
use winit::window::Window;

use crate::sim::{BurnState, SimulationFrame, SimulationParameters, gpucompute::GpuCell};

/// Integrated GPU context for simulation and rendering
///
/// This struct manages:
/// - Shared GPU device and queue
/// - Compute simulation context
/// - Render pipeline and surface
/// - Synchronization between compute and render passes
pub struct GpuSimRenderer {
    #[allow(dead_code)]
    instance: Instance, // Keep instance alive for the lifetime of the renderer
    device: Arc<Device>,
    queue: Arc<Queue>,
    compute: ComputeContextIntegrated,
    render: RenderContextIntegrated,
    width: usize,
    height: usize,
    window: Arc<Window>,
    /// Accumulated time in seconds for fractional tick handling
    accumulated_time: f64,
    /// Last frame timestamp in milliseconds
    last_frame_time: f64,
    /// For debug logging: last logged parameters
    last_logged_params: Option<SimulationParameters>,
    /// For debug logging: time of last tick rate log
    last_tick_log_time: f64,
    /// For debug logging: ticks since last log
    ticks_since_last_log: u32,
}

/// Compute context adapted for integrated rendering
struct ComputeContextIntegrated {
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
    pipeline: wgpu::ComputePipeline,
    steps: u32,
}

/// Render context for integrated GPU simulation
struct RenderContextIntegrated {
    surface: Surface<'static>,
    surface_config: SurfaceConfiguration,
    render_pipeline: RenderPipeline,
    cells_bind_group_1: BindGroup, // Bind group for buf_1
    cells_bind_group_2: BindGroup, // Bind group for buf_2
    size_bind_group: BindGroup,
}

impl GpuSimRenderer {
    /// Create a new integrated GPU context
    ///
    /// # Arguments
    /// * `window` - The window to render to
    /// * `start` - Initial simulation frame
    /// * `parameters` - Simulation parameters
    pub async fn new(
        window: Arc<Window>,
        start: SimulationFrame,
        parameters: SimulationParameters,
    ) -> Result<Self, anyhow::Error> {
        let instance = Instance::new(&wgpu::InstanceDescriptor::default());

        // Create surface first to find compatible adapter
        let surface = instance.create_surface(window.clone())?;

        // Request adapter compatible with surface
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await?;

        log::info!("Using adapter: {:?}", adapter.get_info());

        // Verify compute shader support
        let downlevel_caps = adapter.get_downlevel_capabilities();
        if !downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            return Err(anyhow::anyhow!("adapter does not support compute shaders"));
        }

        // Request device with compute support
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("firesim integrated device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        // Configure surface
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        // Create compute context
        let compute = Self::create_compute_context(&device, &start, parameters)?;

        // Create render context
        let render = Self::create_render_context(
            &device,
            surface,
            surface_config,
            surface_format,
            &compute.buf_1,
            &compute.buf_2,
            start.width as u32,
            start.height as u32,
        )?;

        Ok(Self {
            instance,
            device,
            queue,
            compute,
            render,
            width: start.width,
            height: start.height,
            window,
            accumulated_time: 0.0,
            last_frame_time: 0.0, // Will be set on first frame
            last_logged_params: None,
            last_tick_log_time: 0.0,
            ticks_since_last_log: 0,
        })
    }

    /// Request a redraw of the window
    /// Call this after rendering to keep the animation loop going
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Get window reference
    pub fn window(&self) -> &Window {
        &self.window
    }

    fn create_compute_context(
        device: &Device,
        start: &SimulationFrame,
        parameters: SimulationParameters,
    ) -> Result<ComputeContextIntegrated, anyhow::Error> {
        let start_data: Vec<_> = start
            .grid
            .iter()
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
            source: wgpu::ShaderSource::Wgsl(include_str!("./sim/shader.wgsl").into()),
        });

        // Create cell buffers with STORAGE usage
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

        // Cells bind group layout
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

        // Parameters bind group
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

        // Time bind group
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

        // Size bind group
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

        // Create compute pipeline
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

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("simulation compute pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: None,
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(ComputeContextIntegrated {
            buf_1,
            buf_2,
            cells_bg,
            cells_bg_rev,
            params_buf,
            params_bind_group: params_bg,
            size_bind_group: size_bg,
            flipped_bufs: false,
            time_bind_group: time_bg,
            time_buf,
            old_params: parameters,
            pipeline,
            steps: 0,
        })
    }

    fn create_render_context(
        device: &Device,
        surface: Surface<'static>,
        surface_config: SurfaceConfiguration,
        surface_format: wgpu::TextureFormat,
        buf_1: &Buffer,
        buf_2: &Buffer,
        grid_width: u32,
        grid_height: u32,
    ) -> Result<RenderContextIntegrated, anyhow::Error> {
        // Load render shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("./rendering/render.wgsl").into()),
        });

        // Cells bind group layout for rendering (read-only access)
        let cells_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("render cells bind group layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Create bind groups for both buffers
        let cells_bind_group_1 = device.create_bind_group(&BindGroupDescriptor {
            label: Some("render cells bind group (buf1)"),
            layout: &cells_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: buf_1.as_entire_binding(),
            }],
        });

        let cells_bind_group_2 = device.create_bind_group(&BindGroupDescriptor {
            label: Some("render cells bind group (buf2)"),
            layout: &cells_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: buf_2.as_entire_binding(),
            }],
        });

        // Size bind group
        let size_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("render size bind group layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let size_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("render size buffer"),
            contents: bytemuck::cast_slice(&[grid_width, grid_height]),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let size_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("render size bind group"),
            layout: &size_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: size_buffer.as_entire_binding(),
            }],
        });

        // Create render pipeline
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("render pipeline layout"),
            bind_group_layouts: &[&cells_bind_group_layout, &size_bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        Ok(RenderContextIntegrated {
            surface,
            surface_config,
            render_pipeline,
            cells_bind_group_1,
            cells_bind_group_2,
            size_bind_group,
        })
    }

    /// Execute one simulation step
    pub fn compute_step(&mut self, parameters: SimulationParameters) {
        // Update parameters if changed
        if parameters != self.compute.old_params {
            self.compute.old_params = parameters;
            self.queue
                .write_buffer(&self.compute.params_buf, 0, bytemuck::bytes_of(&parameters));
        }

        // Update time
        self.queue.write_buffer(
            &self.compute.time_buf,
            0,
            bytemuck::bytes_of(&self.compute.steps),
        );

        let num_dispatches = (self.width * self.height).div_ceil(64) as u32;

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("compute encoder"),
            });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("simulation step compute pass"),
                ..Default::default()
            });

            pass.set_pipeline(&self.compute.pipeline);
            pass.set_bind_group(
                0,
                if self.compute.flipped_bufs {
                    &self.compute.cells_bg_rev
                } else {
                    &self.compute.cells_bg
                },
                &[],
            );
            pass.set_bind_group(1, &self.compute.params_bind_group, &[]);
            pass.set_bind_group(2, &self.compute.size_bind_group, &[]);
            pass.set_bind_group(3, &self.compute.time_bind_group, &[]);
            pass.dispatch_workgroups(num_dispatches, 1, 1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));

        self.compute.flipped_bufs = !self.compute.flipped_bufs;
        self.compute.steps += 1;
    }

    /// Get current time in milliseconds
    fn now() -> f64 {
        Date::now()
    }

    /// Execute compute steps based on elapsed time and render
    pub fn step_and_render(
        &mut self,
        parameters: SimulationParameters,
    ) -> Result<(), wgpu::SurfaceError> {
        // Calculate elapsed time and steps to run
        let now = Self::now();
        let elapsed_ms = if self.last_frame_time == 0.0 {
            // First frame - run one step
            0.0
        } else {
            now - self.last_frame_time
        };
        self.last_frame_time = now;

        // Calculate how many simulation steps to run
        // tick_rate is ticks per second, elapsed_ms is in milliseconds
        let tick_rate = parameters.tick_rate as f64;
        let steps_to_run = if tick_rate > 0.0 {
            // Add elapsed time to accumulated time
            self.accumulated_time += elapsed_ms / 1000.0; // Convert to seconds

            // Calculate steps based on tick rate
            let seconds_per_tick = 1.0 / tick_rate;
            let steps = (self.accumulated_time / seconds_per_tick).floor() as u32;

            // Keep remainder for next frame
            self.accumulated_time -= steps as f64 * seconds_per_tick;

            // Cap at reasonable maximum to prevent lag spirals
            steps.min(100)
        } else {
            0 // tick_rate of 0 means paused
        };

        // Log when parameters change
        if self.last_logged_params.as_ref() != Some(&parameters) {
            log::info!(
                "Parameters changed: tick_rate={} ticks/sec, tree_growth={:.2e}/tick, tree_death={:.2e}/tick",
                parameters.tick_rate,
                parameters.tree_growth_rate,
                parameters.tree_death_rate
            );
            self.last_logged_params = Some(parameters);
        }

        // Track ticks for logging
        self.ticks_since_last_log += steps_to_run;

        // Log actual tick rate every 2 seconds
        if now - self.last_tick_log_time >= 2000.0 && self.last_tick_log_time > 0.0 {
            let elapsed_sec = (now - self.last_tick_log_time) / 1000.0;
            let actual_tick_rate = self.ticks_since_last_log as f64 / elapsed_sec;
            log::info!(
                "Actual tick rate: {:.1} ticks/sec (target: {} ticks/sec), {} ticks in {:.1}s",
                actual_tick_rate,
                parameters.tick_rate,
                self.ticks_since_last_log,
                elapsed_sec
            );
            self.last_tick_log_time = now;
            self.ticks_since_last_log = 0;
        } else if self.last_tick_log_time == 0.0 {
            self.last_tick_log_time = now;
        }

        // Update parameters if changed
        if parameters != self.compute.old_params {
            self.compute.old_params = parameters;
            self.queue
                .write_buffer(&self.compute.params_buf, 0, bytemuck::bytes_of(&parameters));
        }

        let num_dispatches = (self.width * self.height).div_ceil(64) as u32;

        // Get surface texture
        let output = self.render.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("compute and render encoder"),
            });

        // Run multiple compute passes if needed
        // Each step must be submitted separately so the time_buf write takes effect
        // before the compute pass reads it (otherwise all passes see the last written value)
        for _ in 0..steps_to_run {
            // Update time buffer for this step
            self.queue.write_buffer(
                &self.compute.time_buf,
                0,
                bytemuck::bytes_of(&self.compute.steps),
            );

            let mut step_encoder = self
                .device
                .create_command_encoder(&CommandEncoderDescriptor {
                    label: Some("compute step encoder"),
                });

            {
                let mut pass = step_encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("simulation step compute pass"),
                    ..Default::default()
                });

                pass.set_pipeline(&self.compute.pipeline);
                pass.set_bind_group(
                    0,
                    if self.compute.flipped_bufs {
                        &self.compute.cells_bg_rev
                    } else {
                        &self.compute.cells_bg
                    },
                    &[],
                );
                pass.set_bind_group(1, &self.compute.params_bind_group, &[]);
                pass.set_bind_group(2, &self.compute.size_bind_group, &[]);
                pass.set_bind_group(3, &self.compute.time_bind_group, &[]);
                pass.dispatch_workgroups(num_dispatches, 1, 1);
            }

            // Submit each step separately so time_buf is correct for each pass
            self.queue.submit(std::iter::once(step_encoder.finish()));

            self.compute.flipped_bufs = !self.compute.flipped_bufs;
            self.compute.steps += 1;
        }

        // Render pass - reads from the most recent output buffer
        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("render pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.render.render_pipeline);

            // Read from the current output buffer (after all compute passes)
            // flipped_bufs now reflects the final state after all steps
            let cells_bind_group = if self.compute.flipped_bufs {
                &self.render.cells_bind_group_2 // buf_2 has latest
            } else {
                &self.render.cells_bind_group_1 // buf_1 has latest
            };

            render_pass.set_bind_group(0, cells_bind_group, &[]);
            render_pass.set_bind_group(1, &self.render.size_bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Render the current simulation state without advancing the simulation
    pub fn render(&self) -> Result<(), wgpu::SurfaceError> {
        let output = self.render.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("render encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("render pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.render.render_pipeline);

            // Read from the current output buffer
            // After step_and_render flips the flag:
            // - If flipped_bufs == true: last compute was buf1→buf2, so buf2 has latest
            // - If flipped_bufs == false: last compute was buf2→buf1, so buf1 has latest
            let cells_bind_group = if self.compute.flipped_bufs {
                &self.render.cells_bind_group_2 // buf_2 has latest
            } else {
                &self.render.cells_bind_group_1 // buf_1 has latest
            };

            render_pass.set_bind_group(0, cells_bind_group, &[]);
            render_pass.set_bind_group(1, &self.render.size_bind_group, &[]);
            render_pass.draw(0..3, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Resize the render surface
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.render.surface_config.width = width;
            self.render.surface_config.height = height;
            self.render
                .surface
                .configure(&self.device, &self.render.surface_config);
        }
    }

    /// Get simulation dimensions
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Get current step count
    pub fn steps(&self) -> u32 {
        self.compute.steps
    }

    /// Get reference to device
    pub fn device(&self) -> &Arc<Device> {
        &self.device
    }

    /// Get reference to queue
    pub fn queue(&self) -> &Arc<Queue> {
        &self.queue
    }
}
