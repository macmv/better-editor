use wgpu::*;

/// Similar to `wgpu::util::TextureBlitter`, but transforms a texture in OkLAB
/// to sRGB.
pub struct TextureBlitterConvert {
  pipeline:          RenderPipeline,
  bind_group_layout: BindGroupLayout,
  sampler:           Sampler,
}

impl TextureBlitterConvert {
  pub fn new(device: &Device, format: TextureFormat) -> Self {
    let sampler = device.create_sampler(&SamplerDescriptor {
      label: Some("TextureBlitterConvert::sampler"),
      address_mode_u: AddressMode::ClampToEdge,
      address_mode_v: AddressMode::ClampToEdge,
      address_mode_w: AddressMode::ClampToEdge,
      mag_filter: FilterMode::Nearest,
      ..Default::default()
    });

    let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
      label:   Some("TextureBlitterConvert::bind_group_layout"),
      entries: &[
        BindGroupLayoutEntry {
          binding:    0,
          visibility: ShaderStages::FRAGMENT,
          ty:         BindingType::Texture {
            sample_type:    TextureSampleType::Float { filterable: false },
            view_dimension: TextureViewDimension::D2,
            multisampled:   false,
          },
          count:      None,
        },
        BindGroupLayoutEntry {
          binding:    1,
          visibility: ShaderStages::FRAGMENT,
          ty:         BindingType::Sampler(SamplerBindingType::NonFiltering),
          count:      None,
        },
      ],
    });

    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
      label:                Some("TextureBlitterConvert::pipeline_layout"),
      bind_group_layouts:   &[&bind_group_layout],
      push_constant_ranges: &[],
    });

    let shader = device.create_shader_module(include_wgsl!("blit.wgsl"));
    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
      label:         Some("TextureBlitterConvert::pipeline"),
      layout:        Some(&pipeline_layout),
      vertex:        VertexState {
        module:              &shader,
        entry_point:         Some("vs_main"),
        compilation_options: PipelineCompilationOptions::default(),
        buffers:             &[],
      },
      primitive:     PrimitiveState {
        topology:           PrimitiveTopology::TriangleList,
        strip_index_format: None,
        front_face:         FrontFace::Ccw,
        cull_mode:          None,
        unclipped_depth:    false,
        polygon_mode:       wgt::PolygonMode::Fill,
        conservative:       false,
      },
      depth_stencil: None,
      multisample:   MultisampleState::default(),
      fragment:      Some(FragmentState {
        module:              &shader,
        entry_point:         Some("fs_main"),
        compilation_options: PipelineCompilationOptions::default(),
        targets:             &[Some(ColorTargetState {
          format:     format,
          blend:      Some(BlendState::ALPHA_BLENDING),
          write_mask: ColorWrites::ALL,
        })],
      }),
      multiview:     None,
      cache:         None,
    });

    TextureBlitterConvert { pipeline, bind_group_layout, sampler }
  }

  pub fn copy(
    &self,
    device: &Device,
    encoder: &mut CommandEncoder,
    source: &TextureView,
    target: &TextureView,
  ) {
    let bind_group = device.create_bind_group(&BindGroupDescriptor {
      label:   Some("TextureBlitterConvert::bind_group"),
      layout:  &self.bind_group_layout,
      entries: &[
        BindGroupEntry { binding: 0, resource: BindingResource::TextureView(source) },
        BindGroupEntry { binding: 1, resource: BindingResource::Sampler(&self.sampler) },
      ],
    });

    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
      label:                    Some("TextureBlitterConvert::pass"),
      color_attachments:        &[Some(RenderPassColorAttachment {
        view:           target,
        depth_slice:    None,
        resolve_target: None,
        ops:            wgt::Operations { load: LoadOp::Load, store: StoreOp::Store },
      })],
      depth_stencil_attachment: None,
      timestamp_writes:         None,
      occlusion_query_set:      None,
    });
    pass.set_pipeline(&self.pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.draw(0..3, 0..1);
  }
}
