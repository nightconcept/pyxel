use std::mem::size_of;

use glow::{HasContext, PixelUnpackData};

use crate::font::RcFont;
use crate::image::{rgb24_to_rgb8, Color, RcImage, Rgb24};
use crate::platform::{self, GlProfile};
use crate::pyxel::{self, Pyxel};
use crate::settings::{BACKGROUND_COLOR, MAX_COLORS, NUM_SCREEN_MODES};

// macOS cannot run the desktop GL shaders; use the GLES header there.
#[cfg(target_os = "macos")]
const GL_VERSION: &str = include_str!("shaders/gles_version.glsl");
#[cfg(not(target_os = "macos"))]
const GL_VERSION: &str = include_str!("shaders/gl_version.glsl");

const GLES_VERSION: &str = include_str!("shaders/gles_version.glsl");
const COMMON_VERT: &str = include_str!("shaders/common.vert");
const COMMON_FRAG: &str = include_str!("shaders/common.frag");
const SCREEN_FRAGS: [&str; NUM_SCREEN_MODES as usize] = [
    include_str!("shaders/crisp.frag"),
    include_str!("shaders/smooth.frag"),
    include_str!("shaders/retro.frag"),
];

const NUM_UNIFORMS: usize = 7;
const U_SCREEN_POS: usize = 0;
const U_SCREEN_SIZE: usize = 1;
const U_SCREEN_SCALE: usize = 2;
const U_NUM_COLORS: usize = 3;
const U_BACKGROUND_COLOR: usize = 4;
const U_SCREEN_TEXTURE: usize = 5;
const U_COLORS_TEXTURE: usize = 6;

const UNIFORM_NAMES: [&str; NUM_UNIFORMS] = [
    "u_screenPos",
    "u_screenSize",
    "u_screenScale",
    "u_numColors",
    "u_backgroundColor",
    "u_screenTexture",
    "u_colorsTexture",
];

// [pyxel-ne] Extra index images a user shader can sample as u_image0..3 (texture units 2..5).
pub const NUM_SHADER_IMAGES: u32 = 4;
const MAX_UNIFORM_FLOATS: usize = 1024;

const QUAD_VERTICES: [f32; 8] = [-1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0, -1.0];

pub struct ScreenShader {
    program: glow::Program,
    uniform_locations: [Option<glow::UniformLocation>; NUM_UNIFORMS],
    vertex_array: glow::VertexArray,
}

pub struct Graphics {
    is_gles: bool,
    screen_shaders: Vec<ScreenShader>,
    screen_texture: glow::NativeTexture,
    screen_texture_initialized: bool,
    colors_texture: glow::NativeTexture,
    cached_colors: Vec<Rgb24>,
    color_pixels: Vec<u8>,

    // [pyxel-ne] User screen shader and its inputs
    user_shader: Option<ScreenShader>,
    user_uniforms: Vec<(String, Vec<f32>)>,
    user_images: [Option<RcImage>; NUM_SHADER_IMAGES as usize],
    user_textures: [Option<glow::NativeTexture>; NUM_SHADER_IMAGES as usize],
    display_capture: Option<(String, u32)>,
}

impl Graphics {
    // OpenGL resources

    pub fn new() -> Self {
        // SAFETY: Platform initialization keeps this thread's GL context current.
        // Graphics owns every created handle, and uploads use complete typed slices.
        let is_gles = platform::gl_profile() == GlProfile::Gles;
        platform::with_gl_context(|gl| unsafe {
            if !is_gles {
                gl.disable(glow::FRAMEBUFFER_SRGB);
            }
            gl.disable(glow::BLEND);

            let screen_shaders = Self::create_screen_shaders(gl, is_gles);
            let screen_texture = Self::create_screen_texture(gl);
            let colors_texture = Self::create_colors_texture(gl);

            Self {
                is_gles,
                screen_shaders,
                screen_texture,
                screen_texture_initialized: false,
                colors_texture,
                cached_colors: Vec::new(),
                color_pixels: Vec::new(),
                user_shader: None,
                user_uniforms: Vec::new(),
                user_images: Default::default(),
                user_textures: Default::default(),
                display_capture: None,
            }
        })
    }

    pub(crate) fn invalidate_screen_texture(&mut self) {
        self.screen_texture_initialized = false;
    }

    // Shader setup

    unsafe fn create_screen_shaders(gl: &mut glow::Context, is_gles: bool) -> Vec<ScreenShader> {
        // Compile one program per fragment shader
        SCREEN_FRAGS
            .iter()
            .map(|screen_frag| {
                Self::build_screen_shader(gl, is_gles, screen_frag)
                    .unwrap_or_else(|e| panic!("{e}"))
            })
            .collect()
    }

    // [pyxel-ne] Shared by the built-in modes and the user shader. Returns the GL info log on failure.
    unsafe fn build_screen_shader(
        gl: &mut glow::Context,
        is_gles: bool,
        screen_frag: &str,
    ) -> Result<ScreenShader, String> {
        let glsl_version = if is_gles { GLES_VERSION } else { GL_VERSION };

        // Vertex shader
        let vertex_shader = gl
            .create_shader(glow::VERTEX_SHADER)
            .expect("Failed to create OpenGL vertex shader");
        gl.shader_source(vertex_shader, &format!("{glsl_version}{COMMON_VERT}"));
        gl.compile_shader(vertex_shader);
        if !gl.get_shader_compile_status(vertex_shader) {
            let log = gl.get_shader_info_log(vertex_shader);
            gl.delete_shader(vertex_shader);
            return Err(format!("\n[vertex shader]\n{log}"));
        }

        // Fragment shader
        let fragment_shader = gl
            .create_shader(glow::FRAGMENT_SHADER)
            .expect("Failed to create OpenGL fragment shader");
        gl.shader_source(
            fragment_shader,
            &format!("{glsl_version}{COMMON_FRAG}{screen_frag}"),
        );
        gl.compile_shader(fragment_shader);
        if !gl.get_shader_compile_status(fragment_shader) {
            let log = gl.get_shader_info_log(fragment_shader);
            gl.delete_shader(vertex_shader);
            gl.delete_shader(fragment_shader);
            return Err(format!("\n[fragment shader]\n{log}"));
        }

        // Shader program
        let program = gl
            .create_program()
            .expect("Failed to create OpenGL shader program");
        gl.attach_shader(program, vertex_shader);
        gl.attach_shader(program, fragment_shader);
        gl.link_program(program);
        gl.detach_shader(program, vertex_shader);
        gl.delete_shader(vertex_shader);
        gl.detach_shader(program, fragment_shader);
        gl.delete_shader(fragment_shader);
        if !gl.get_program_link_status(program) {
            let log = gl.get_program_info_log(program);
            gl.delete_program(program);
            return Err(log);
        }

        // Uniform locations
        let uniform_locations: [Option<glow::UniformLocation>; NUM_UNIFORMS] =
            std::array::from_fn(|i| gl.get_uniform_location(program, UNIFORM_NAMES[i]));

        // Vertex array
        let vertex_array = gl
            .create_vertex_array()
            .expect("Failed to create OpenGL vertex array");
        let vertex_buffer = gl.create_buffer().expect("Failed to create OpenGL buffer");

        gl.bind_vertex_array(Some(vertex_array));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vertex_buffer));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            QUAD_VERTICES.align_to::<u8>().1,
            glow::STATIC_DRAW,
        );

        let position = gl
            .get_attrib_location(program, "position")
            .expect("Failed to find OpenGL attribute 'position'");
        gl.vertex_attrib_pointer_f32(
            position,
            2,
            glow::FLOAT,
            false,
            2 * size_of::<f32>() as i32,
            0,
        );
        gl.enable_vertex_attrib_array(position);

        Ok(ScreenShader {
            program,
            uniform_locations,
            vertex_array,
        })
    }

    // Texture setup

    unsafe fn set_texture_nearest_clamp(gl: &mut glow::Context) {
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
    }

    unsafe fn create_screen_texture(gl: &mut glow::Context) -> glow::NativeTexture {
        let screen_texture = gl
            .create_texture()
            .expect("Failed to create OpenGL screen texture");
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(screen_texture));
        Self::set_texture_nearest_clamp(gl);
        screen_texture
    }

    unsafe fn create_colors_texture(gl: &mut glow::Context) -> glow::NativeTexture {
        let colors_texture = gl
            .create_texture()
            .expect("Failed to create OpenGL colors texture");
        gl.active_texture(glow::TEXTURE1);
        gl.bind_texture(glow::TEXTURE_2D, Some(colors_texture));
        Self::set_texture_nearest_clamp(gl);
        colors_texture
    }
}

impl Pyxel {
    // View transformation

    pub fn set_clip_rect(&self, x: f32, y: f32, width: f32, height: f32) {
        rc_mut!(pyxel::screen()).set_clip_rect(x, y, width, height);
    }

    pub fn reset_clip_rect(&self) {
        rc_mut!(pyxel::screen()).reset_clip_rect();
    }

    pub fn set_camera(&self, x: f32, y: f32) {
        rc_mut!(pyxel::screen()).set_camera(x, y);
    }

    pub fn reset_camera(&self) {
        rc_mut!(pyxel::screen()).reset_camera();
    }

    pub fn map_color(&self, src_color: Color, dst_color: Color) {
        rc_mut!(pyxel::screen()).map_color(src_color, dst_color);
    }

    pub fn reset_color_map(&self) {
        rc_mut!(pyxel::screen()).reset_color_map();
    }

    pub fn set_dithering(&self, alpha: f32) {
        rc_mut!(pyxel::screen()).set_dithering(alpha);
    }

    pub fn clear(&self, color: Color) {
        rc_mut!(pyxel::screen()).clear(color);
    }

    // Drawing primitives

    pub fn pixel(&self, x: f32, y: f32) -> Color {
        rc_ref!(pyxel::screen()).pixel(x, y)
    }

    pub fn set_pixel(&self, x: f32, y: f32, color: Color) {
        rc_mut!(pyxel::screen()).set_pixel(x, y, color);
    }

    pub fn draw_line(&self, x1: f32, y1: f32, x2: f32, y2: f32, color: Color) {
        rc_mut!(pyxel::screen()).draw_line(x1, y1, x2, y2, color);
    }

    pub fn draw_rect(&self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        rc_mut!(pyxel::screen()).draw_rect(x, y, width, height, color);
    }

    pub fn draw_rect_border(&self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        rc_mut!(pyxel::screen()).draw_rect_border(x, y, width, height, color);
    }

    pub fn draw_circle(&self, x: f32, y: f32, radius: f32, color: Color) {
        rc_mut!(pyxel::screen()).draw_circle(x, y, radius, color);
    }

    pub fn draw_circle_border(&self, x: f32, y: f32, radius: f32, color: Color) {
        rc_mut!(pyxel::screen()).draw_circle_border(x, y, radius, color);
    }

    pub fn draw_ellipse(&self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        rc_mut!(pyxel::screen()).draw_ellipse(x, y, width, height, color);
    }

    pub fn draw_ellipse_border(&self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        rc_mut!(pyxel::screen()).draw_ellipse_border(x, y, width, height, color);
    }

    pub fn draw_triangle(
        &self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
        color: Color,
    ) {
        rc_mut!(pyxel::screen()).draw_triangle(x1, y1, x2, y2, x3, y3, color);
    }

    pub fn draw_triangle_border(
        &self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
        color: Color,
    ) {
        rc_mut!(pyxel::screen()).draw_triangle_border(x1, y1, x2, y2, x3, y3, color);
    }

    pub fn flood_fill(&self, x: f32, y: f32, color: Color) {
        rc_mut!(pyxel::screen()).flood_fill(x, y, color);
    }

    // Image & tilemap

    pub fn draw_image(
        &self,
        x: f32,
        y: f32,
        image_index: u32,
        image_x: f32,
        image_y: f32,
        width: f32,
        height: f32,
        transparent: Option<Color>,
        rotate: Option<f32>,
        scale: Option<f32>,
    ) {
        rc_mut!(pyxel::screen()).draw_image(
            x,
            y,
            &pyxel::images()[image_index as usize],
            image_x,
            image_y,
            width,
            height,
            transparent,
            rotate,
            scale,
        );
    }

    pub fn draw_tilemap(
        &self,
        x: f32,
        y: f32,
        tilemap_index: u32,
        tilemap_x: f32,
        tilemap_y: f32,
        width: f32,
        height: f32,
        transparent: Option<Color>,
        rotate: Option<f32>,
        scale: Option<f32>,
    ) {
        rc_mut!(pyxel::screen()).draw_tilemap(
            x,
            y,
            &pyxel::tilemaps()[tilemap_index as usize],
            tilemap_x,
            tilemap_y,
            width,
            height,
            transparent,
            rotate,
            scale,
        );
    }

    // 3D graphics

    pub fn draw_image_3d(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        image_index: u32,
        pos: (f32, f32, f32),
        rot: (f32, f32, f32),
        fov: Option<f32>,
        transparent: Option<Color>,
    ) {
        rc_mut!(pyxel::screen()).draw_image_3d(
            x,
            y,
            width,
            height,
            &pyxel::images()[image_index as usize],
            pos,
            rot,
            fov,
            transparent,
        );
    }

    pub fn draw_tilemap_3d(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        tilemap_index: u32,
        pos: (f32, f32, f32),
        rot: (f32, f32, f32),
        fov: Option<f32>,
        transparent: Option<Color>,
    ) {
        rc_mut!(pyxel::screen()).draw_tilemap_3d(
            x,
            y,
            width,
            height,
            &pyxel::tilemaps()[tilemap_index as usize],
            pos,
            rot,
            fov,
            transparent,
        );
    }

    // Text & rendering

    pub fn draw_text(&self, x: f32, y: f32, text: &str, color: Color, font: Option<&RcFont>) {
        rc_mut!(pyxel::screen()).draw_text(x, y, text, color, font);
    }

    pub(crate) fn render_screen(&mut self) {
        if self.graphics.is_none() {
            return;
        }

        // SAFETY: The platform keeps the GL context current until shutdown.
        // Graphics owns the bound handles, and texture dimensions match the slices.
        let (window_width, window_height) = platform::window_size();
        platform::with_gl_context(|gl| unsafe {
            self.set_viewport(gl, window_width, window_height);
            self.use_screen_shader(gl, window_height);
            self.bind_user_shader_inputs(gl);
            self.bind_screen_texture(gl);
            self.bind_colors_texture(gl);
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
            self.save_display_capture(gl, window_width, window_height);
        });
    }

    unsafe fn set_viewport(&self, gl: &mut glow::Context, window_width: u32, window_height: u32) {
        gl.viewport(0, 0, window_width as i32, window_height as i32);
    }

    unsafe fn use_screen_shader(&self, gl: &mut glow::Context, window_height: u32) {
        let graphics = self.graphics.as_ref().unwrap();
        let shader = graphics
            .user_shader
            .as_ref()
            .unwrap_or(&graphics.screen_shaders[self.system.screen_mode as usize]);
        gl.use_program(Some(shader.program));
        let uniforms = &shader.uniform_locations;

        // Screen placement and size uniforms
        if let Some(location) = &uniforms[U_SCREEN_POS] {
            gl.uniform_2_f32(
                Some(location),
                self.system.screen_x as f32,
                (window_height as i32
                    - self.system.screen_y
                    - (*pyxel::height() as f32 * self.system.screen_scale) as i32)
                    as f32,
            );
        }

        if let Some(location) = &uniforms[U_SCREEN_SIZE] {
            gl.uniform_2_f32(
                Some(location),
                *pyxel::width() as f32 * self.system.screen_scale,
                *pyxel::height() as f32 * self.system.screen_scale,
            );
        }

        if let Some(location) = &uniforms[U_SCREEN_SCALE] {
            gl.uniform_1_f32(Some(location), self.system.screen_scale);
        }

        // Palette and texture uniforms
        if let Some(location) = &uniforms[U_NUM_COLORS] {
            gl.uniform_1_i32(Some(location), pyxel::colors().len() as i32);
        }

        if let Some(location) = &uniforms[U_BACKGROUND_COLOR] {
            let (r, g, b) = rgb24_to_rgb8(BACKGROUND_COLOR);
            gl.uniform_3_f32(
                Some(location),
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
            );
        }

        if let Some(location) = &uniforms[U_SCREEN_TEXTURE] {
            gl.uniform_1_i32(Some(location), 0);
        }

        if let Some(location) = &uniforms[U_COLORS_TEXTURE] {
            gl.uniform_1_i32(Some(location), 1);
        }

        gl.bind_vertex_array(Some(shader.vertex_array));
    }

    unsafe fn bind_screen_texture(&mut self, gl: &mut glow::Context) {
        let graphics = self.graphics.as_mut().unwrap();
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(graphics.screen_texture));
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);

        let (internal_format, format) = if graphics.is_gles {
            (glow::LUMINANCE as i32, glow::LUMINANCE)
        } else {
            (glow::R8 as i32, glow::RED)
        };

        let screen_width = *pyxel::width() as i32;
        let screen_height = *pyxel::height() as i32;
        let screen_rc = pyxel::screen().clone();
        let screen = rc_ref!(screen_rc);
        let data = &screen.canvas.data;

        // Upload or refresh the screen color-index texture.
        if graphics.screen_texture_initialized {
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                screen_width,
                screen_height,
                format,
                glow::UNSIGNED_BYTE,
                PixelUnpackData::Slice(Some(data)),
            );
        } else {
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                internal_format,
                screen_width,
                screen_height,
                0,
                format,
                glow::UNSIGNED_BYTE,
                PixelUnpackData::Slice(Some(data)),
            );
            graphics.screen_texture_initialized = true;
        }
    }

    unsafe fn bind_colors_texture(&mut self, gl: &mut glow::Context) {
        let colors = pyxel::colors();
        assert!(
            !colors.is_empty() && colors.len() <= MAX_COLORS as usize,
            "Number of colors must be between 1 and {MAX_COLORS}",
        );

        let graphics = self.graphics.as_mut().unwrap();
        gl.active_texture(glow::TEXTURE1);
        gl.bind_texture(glow::TEXTURE_2D, Some(graphics.colors_texture));

        if graphics.cached_colors == *colors {
            return;
        }

        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);

        let pixels = &mut graphics.color_pixels;
        pixels.clear();
        // Pack RGB palette entries for the 1D color texture.
        for &c in colors.iter() {
            let (r, g, b) = rgb24_to_rgb8(c);
            pixels.push(r);
            pixels.push(g);
            pixels.push(b);
        }

        if graphics.cached_colors.len() == colors.len() {
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                colors.len() as i32,
                1,
                glow::RGB,
                glow::UNSIGNED_BYTE,
                PixelUnpackData::Slice(Some(pixels)),
            );
        } else {
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGB as i32,
                colors.len() as i32,
                1,
                0,
                glow::RGB,
                glow::UNSIGNED_BYTE,
                PixelUnpackData::Slice(Some(pixels)),
            );
        }
        graphics.cached_colors.clone_from(&colors);
    }

    // [pyxel-ne] User screen shader

    /// Replace the screen shader with `code`, a fragment shader body appended after common.frag.
    /// `None` restores the built-in shader picked by `screen_mode`. On a compile error the previous
    /// shader stays active and the GL log is returned.
    pub fn set_screen_shader(&mut self, code: Option<&str>) -> Result<(), String> {
        let Some(graphics) = self.graphics.as_mut() else {
            return Ok(()); // Headless: nothing to draw with.
        };
        // SAFETY: The platform keeps the GL context current; Graphics owns the replaced program.
        platform::with_gl_context(|gl| unsafe {
            let shader = match code {
                Some(code) => Some(Graphics::build_screen_shader(gl, graphics.is_gles, code)?),
                None => None,
            };
            if let Some(old) = std::mem::replace(&mut graphics.user_shader, shader) {
                gl.delete_program(old.program);
                gl.delete_vertex_array(old.vertex_array);
            }
            Ok(())
        })
    }

    /// Set a float, vec2, vec3, vec4 (1-4 values) or vec4[] (a multiple of 4 values) uniform.
    pub fn set_shader_uniform(&mut self, name: &str, values: &[f32]) -> Result<(), String> {
        let n = values.len();
        if n == 0 || n > MAX_UNIFORM_FLOATS || (n > 4 && !n.is_multiple_of(4)) {
            return Err(format!(
                "uniform '{name}' needs 1-4 values or a multiple of 4 up to {MAX_UNIFORM_FLOATS}, got {n}"
            ));
        }
        let Some(graphics) = self.graphics.as_mut() else {
            return Ok(());
        };
        match graphics.user_uniforms.iter_mut().find(|(k, _)| k == name) {
            Some((_, v)) => values.clone_into(v),
            None => graphics
                .user_uniforms
                .push((name.to_string(), values.to_vec())),
        }
        Ok(())
    }

    /// Let the user shader sample `image` (raw color indices) as `u_image{slot}`, sized `u_imageSize{slot}`.
    pub fn set_shader_image(&mut self, slot: u32, image: Option<RcImage>) -> Result<(), String> {
        if slot >= NUM_SHADER_IMAGES {
            return Err(format!("slot must be 0-{}", NUM_SHADER_IMAGES - 1));
        }
        if let Some(graphics) = self.graphics.as_mut() {
            graphics.user_images[slot as usize] = image;
        }
        Ok(())
    }

    unsafe fn bind_user_shader_inputs(&mut self, gl: &mut glow::Context) {
        let graphics = self.graphics.as_mut().unwrap();
        let Some(shader) = graphics.user_shader.as_ref() else {
            return;
        };
        let program = shader.program;

        for (name, values) in &graphics.user_uniforms {
            let Some(location) = gl.get_uniform_location(program, name) else {
                continue; // Unused uniforms are optimized out; that is not an error.
            };
            match values.len() {
                1 => gl.uniform_1_f32(Some(&location), values[0]),
                2 => gl.uniform_2_f32_slice(Some(&location), values),
                3 => gl.uniform_3_f32_slice(Some(&location), values),
                _ => gl.uniform_4_f32_slice(Some(&location), values),
            }
        }

        let (internal_format, format) = if graphics.is_gles {
            (glow::LUMINANCE as i32, glow::LUMINANCE)
        } else {
            (glow::R8 as i32, glow::RED)
        };
        for slot in 0..NUM_SHADER_IMAGES as usize {
            let Some(image) = graphics.user_images[slot].clone() else {
                continue;
            };
            let texture = *graphics.user_textures[slot].get_or_insert_with(|| {
                let texture = gl
                    .create_texture()
                    .expect("Failed to create OpenGL shader image texture");
                gl.active_texture(glow::TEXTURE2 + slot as u32);
                gl.bind_texture(glow::TEXTURE_2D, Some(texture));
                Graphics::set_texture_nearest_clamp(gl);
                texture
            });
            gl.active_texture(glow::TEXTURE2 + slot as u32);
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);

            let image = rc_ref!(image);
            let width = image.width() as i32;
            let height = image.height() as i32;
            // Re-upload every frame: shader images are small and usually redrawn each frame.
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                internal_format,
                width,
                height,
                0,
                format,
                glow::UNSIGNED_BYTE,
                PixelUnpackData::Slice(Some(&image.canvas.data)),
            );
            if let Some(location) = gl.get_uniform_location(program, &format!("u_image{slot}")) {
                gl.uniform_1_i32(Some(&location), 2 + slot as i32);
            }
            if let Some(location) = gl.get_uniform_location(program, &format!("u_imageSize{slot}"))
            {
                gl.uniform_2_f32(Some(&location), width as f32, height as f32);
            }
        }
    }

    // [pyxel-ne] Display capture: the shaded output, which screenshot() cannot see

    /// Save the next rendered frame as it appears in the window (after the screen shader),
    /// sampled at each screen pixel's centre and scaled by `scale`.
    pub fn request_display_capture(&mut self, filename: &str, scale: u32) {
        if let Some(graphics) = self.graphics.as_mut() {
            graphics.display_capture = Some((filename.to_string(), scale.max(1)));
        }
    }

    unsafe fn save_display_capture(
        &mut self,
        gl: &mut glow::Context,
        window_width: u32,
        window_height: u32,
    ) {
        let Some((filename, scale)) = self.graphics.as_mut().unwrap().display_capture.take() else {
            return;
        };
        let mut pixels = vec![0u8; (window_width * window_height * 4) as usize];
        gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
        gl.read_pixels(
            0,
            0,
            window_width as i32,
            window_height as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut pixels)),
        );

        let width = *pyxel::width();
        let height = *pyxel::height();
        let screen_scale = self.system.screen_scale;
        let mut image = image::RgbImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let wx = self.system.screen_x as f32 + (x as f32 + 0.5) * screen_scale;
                let wy = self.system.screen_y as f32 + (y as f32 + 0.5) * screen_scale;
                let gx = (wx as u32).min(window_width - 1);
                let gy = (window_height - 1).saturating_sub(wy as u32); // GL rows run bottom-up.
                let i = ((gy * window_width + gx) * 4) as usize;
                image.put_pixel(x, y, image::Rgb([pixels[i], pixels[i + 1], pixels[i + 2]]));
            }
        }
        let image = image::imageops::resize(
            &image,
            width * scale,
            height * scale,
            image::imageops::FilterType::Nearest,
        );
        let filename = crate::utils::add_file_extension(&filename, ".png");
        if image.save(&filename).is_err() {
            println!("Failed to save file '{filename}'");
        }
    }
}
