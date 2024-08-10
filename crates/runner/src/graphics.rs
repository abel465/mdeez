use crate::controller::Controller;
use crate::{maybe_watch, CompiledShaderModules};
use shared::ShaderConstants;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder},
    window::Window,
};

mod shaders {
    #[allow(non_upper_case_globals)]
    pub const main_fs: &str = "main_fs";
    #[allow(non_upper_case_globals)]
    pub const main_vs: &str = "main_vs";
}

async fn run(
    event_loop: EventLoop<CompiledShaderModules>,
    window: Window,
    compiled_shader_modules: CompiledShaderModules,
) {
    let backends = wgpu::util::backend_bits_from_env()
        .unwrap_or(wgpu::Backends::VULKAN | wgpu::Backends::METAL);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        dx12_shader_compiler: wgpu::util::dx12_shader_compiler_from_env().unwrap_or_default(),
        ..Default::default()
    });

    let initial_surface = instance
        .create_surface(&window)
        .expect("Failed to create surface from window");

    let adapter =
        wgpu::util::initialize_adapter_from_env_or_default(&instance, Some(&initial_surface))
            .await
            .expect("Failed to find an appropriate adapter");

    let required_features = wgpu::Features::PUSH_CONSTANTS;
    let required_limits = wgpu::Limits {
        max_push_constant_size: 128,
        ..Default::default()
    };

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features,
                required_limits,
            },
            None,
        )
        .await
        .expect("Failed to create device");

    fn auto_configure_surface<'a>(
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        surface: wgpu::Surface<'a>,
        size: winit::dpi::PhysicalSize<u32>,
    ) -> (wgpu::Surface<'a>, wgpu::SurfaceConfiguration) {
        let mut surface_config = surface
            .get_default_config(adapter, size.width, size.height)
            .unwrap_or_else(|| {
                panic!(
                    "Missing formats/present modes in surface capabilities: {:#?}",
                    surface.get_capabilities(adapter)
                )
            });
        surface_config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(device, &surface_config);

        (surface, surface_config)
    }
    let mut surface_with_config =
        auto_configure_surface(&adapter, &device, initial_surface, window.inner_size());

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[],
        push_constant_ranges: &[wgpu::PushConstantRange {
            stages: wgpu::ShaderStages::FRAGMENT,
            range: 0..std::mem::size_of::<ShaderConstants>() as u32,
        }],
    });

    let mut render_pipeline = create_pipeline(
        &device,
        &pipeline_layout,
        surface_with_config.1.format,
        compiled_shader_modules,
    );

    let mut controller = Controller::new();

    event_loop
        .run(|event, event_loop_window_target| {
            // Have the closure take ownership of the resources.
            // `event_loop.run` never returns, therefore we must do this to ensure
            // the resources are properly cleaned up.
            let _ = (&instance, &adapter, &pipeline_layout);
            let render_pipeline = &mut render_pipeline;

            event_loop_window_target.set_control_flow(ControlFlow::Wait);
            match event {
                Event::Resumed => {
                    let new_surface = instance
                        .create_surface(&window)
                        .expect("Failed to create surface from window (after resume)");
                    surface_with_config =
                        auto_configure_surface(&adapter, &device, new_surface, window.inner_size());
                }
                Event::WindowEvent {
                    event: WindowEvent::Resized(size),
                    ..
                } => {
                    if size.width != 0 && size.height != 0 {
                        let (surface, surface_config) = &mut surface_with_config;
                        surface_config.width = size.width;
                        surface_config.height = size.height;
                        surface.configure(&device, surface_config);
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::RedrawRequested,
                    ..
                } => {
                    window.request_redraw();

                    let (surface, surface_config) = &mut surface_with_config;
                    let output = match surface.get_current_texture() {
                        Ok(surface) => surface,
                        Err(err) => {
                            eprintln!("get_current_texture error: {err:?}");
                            match err {
                                wgpu::SurfaceError::Lost => {
                                    surface.configure(&device, surface_config);
                                }
                                wgpu::SurfaceError::OutOfMemory => {
                                    event_loop_window_target.exit();
                                }
                                _ => (),
                            }
                            return;
                        }
                    };
                    let output_view = output
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    let mut encoder = device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                    {
                        let mut render_pass =
                            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: None,
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &output_view,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                ..Default::default()
                            });

                        controller.update();
                        let push_constants = controller.shader_constants(window.inner_size());

                        render_pass.set_pipeline(render_pipeline);
                        render_pass.set_push_constants(
                            wgpu::ShaderStages::FRAGMENT,
                            0,
                            bytemuck::bytes_of(&push_constants),
                        );
                        render_pass.draw(0..3, 0..1);
                    }

                    queue.submit(Some(encoder.finish()));
                    output.present();
                }
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => event_loop_window_target.exit(),
                Event::WindowEvent {
                    event:
                        WindowEvent::KeyboardInput {
                            event:
                                winit::event::KeyEvent {
                                    logical_key, state, ..
                                },
                            ..
                        },
                    ..
                } => {
                    if state == winit::event::ElementState::Pressed {
                        controller.on_key_press(logical_key, state);
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::MouseInput { state, button, .. },
                    ..
                } => {
                    controller.on_mouse_input(state, button);
                }
                Event::WindowEvent {
                    event: WindowEvent::CursorMoved { position, .. },
                    ..
                } => {
                    controller.on_mouse_move(position);
                }
                Event::UserEvent(new_module) => {
                    *render_pipeline = create_pipeline(
                        &device,
                        &pipeline_layout,
                        surface_with_config.1.format,
                        new_module,
                    );
                    window.request_redraw();
                    event_loop_window_target.set_control_flow(ControlFlow::Poll);
                }
                _ => {}
            }
        })
        .unwrap();
}

fn create_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    surface_format: wgpu::TextureFormat,
    compiled_shader_modules: CompiledShaderModules,
) -> wgpu::RenderPipeline {
    let create_module = |module| {
        let wgpu::ShaderModuleDescriptorSpirV { label, source } = module;
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label,
            source: wgpu::ShaderSource::SpirV(source),
        })
    };

    let vs_entry_point = shaders::main_vs;
    let fs_entry_point = shaders::main_fs;

    let vs_module_descr = compiled_shader_modules.spv_module_for_entry_point(vs_entry_point);
    let fs_module_descr = compiled_shader_modules.spv_module_for_entry_point(fs_entry_point);

    let vs_fs_same_module = std::ptr::eq(&vs_module_descr.source[..], &fs_module_descr.source[..]);

    let vs_module = &create_module(vs_module_descr);
    let fs_module;
    let fs_module = if vs_fs_same_module {
        vs_module
    } else {
        fs_module = create_module(fs_module_descr);
        &fs_module
    };

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: vs_module,
            entry_point: vs_entry_point,
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(wgpu::FragmentState {
            module: fs_module,
            entry_point: fs_entry_point,
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
    })
}

pub fn start() {
    let mut event_loop_builder = EventLoopBuilder::with_user_event();
    let event_loop = event_loop_builder.build().unwrap();

    let initial_shader = maybe_watch({
        let proxy = event_loop.create_proxy();
        Some(Box::new(move |res| match proxy.send_event(res) {
            Ok(it) => it,
            Err(_err) => panic!("Event loop dead"),
        }))
    });

    let window = winit::window::WindowBuilder::new()
        .with_title("pendulum")
        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0))
        .build(&event_loop)
        .unwrap();

    futures::executor::block_on(run(event_loop, window, initial_shader));
}
