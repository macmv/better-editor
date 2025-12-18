struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
  var out: VertexOutput;

  out.tex_coords = vec2<f32>(
    f32((vi << 1u) & 2u),
    f32(vi & 2u),
  );

  out.position = vec4<f32>(out.tex_coords * 2.0 - 1.0, 0.0, 1.0);

  // Invert y so the texture is not upside down
  out.tex_coords.y = 1.0 - out.tex_coords.y;
  return out;
}

@group(0) @binding(0)
var texture: texture_2d<f32>;
@group(0) @binding(1)
var texture_sampler: sampler;

@fragment
fn fs_main(vs: VertexOutput) -> @location(0) vec4<f32> {
  let color = textureSample(texture, texture_sampler, vs.tex_coords);
  let rgb = oklab_to_linear_rgb(vec3(
    color.r,
    color.g - 0.5,
    color.b - 0.5,
  ));
  let srgb = vec3<f32>(
    linear_to_srgb(rgb.r),
    linear_to_srgb(rgb.g),
    linear_to_srgb(rgb.b),
  );
  return vec4<f32>(srgb, color.a);
}

fn oklab_to_linear_rgb(c: vec3<f32>) -> vec3<f32> {
  let l_ = c.r + 0.3963377774f * c.g + 0.2158037573f * c.b;
  let m_ = c.r - 0.1055613458f * c.g - 0.0638541728f * c.b;
  let s_ = c.r - 0.0894841775f * c.g - 1.2914855480f * c.b;

  let l = l_*l_*l_;
  let m = m_*m_*m_;
  let s = s_*s_*s_;

  return vec3(
    4.0767416621f * l - 3.3077115913f * m + 0.2309699292f * s,
    -1.2684380046f * l + 2.6097574011f * m - 0.3413193965f * s,
    -0.0041960863f * l - 0.7034186147f * m + 1.7076147010f * s,
  );
}

fn linear_to_srgb(x: f32) -> f32 {
  if x >= 0.0031308 {
    return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
  } else {
    return 12.92 * x;
  }
}
