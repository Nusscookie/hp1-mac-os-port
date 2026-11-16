//! Fly through a converted map.
//!
//!   hp1-viewer <converted folder> <map> [--shot out.png] [--camera x,y,z,yaw,pitch] [--size WxH]
//!
//! Mouse looks around, WASD moves, Q/E go down/up, Shift is faster, C prints the camera (for
//! `--camera`), Escape quits. With `--shot` it renders one image without opening a window.

mod actors;
mod textures;

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use glam::{Mat4, Vec3};
use hp1_data::ClassTable;
use hp1_data::level::LevelFile;
use hp1_data::mesh::load_binary;
use hp1_engine::level_mesh::{Style, TextureSize, build_level_mesh};
use hp1_engine::lighting::build_level_light_maps;
use hp1_engine::props::ActorProps;
use hp1_engine::units::{ROT_TO_RAD, rotator_matrix};
use hp1_platform::sdl3::event::{Event, WindowEvent};
use hp1_platform::sdl3::keyboard::{Keycode, Scancode};
use hp1_render::{Blend, DrawBatch, Geometry, Gpu, Image, Scene, SceneDesc, Vertex, WorldRenderer, wgpu};
use textures::Textures;

struct Args {
    data: PathBuf,
    map: String,
    shot: Option<PathBuf>,
    camera: Option<[f32; 5]>,
    size: (u32, u32),
}

fn parse_args() -> Result<Args> {
    let mut it = std::env::args().skip(1);
    let usage =
        "usage: hp1-viewer <converted folder> <map> [--shot out.png] [--camera x,y,z,yaw,pitch] [--size WxH]";
    let data = PathBuf::from(it.next().context(usage)?);
    let map = it.next().context(usage)?;
    let mut args = Args { data, map, shot: None, camera: None, size: (1280, 800) };
    while let Some(a) = it.next() {
        let v = it.next().with_context(|| format!("{a} needs a value"))?;
        match a.as_str() {
            "--shot" => args.shot = Some(v.into()),
            "--camera" => {
                let n: Vec<f32> = v.split(',').map(|s| s.trim().parse()).collect::<Result<_, _>>()?;
                args.camera = Some(n.try_into().map_err(|_| anyhow!("--camera wants 5 numbers"))?);
            }
            "--size" => {
                let (w, h) = v.split_once('x').context("--size wants WxH")?;
                args.size = (w.parse()?, h.parse()?);
            }
            _ => bail!("unknown option {a}\n{usage}"),
        }
    }
    Ok(args)
}

/// The free-flying camera, in the game's units (X forward, Y right, Z up; yaw/pitch in radians).
struct Camera {
    pos: Vec3,
    yaw: f32,
    pitch: f32,
}

impl Camera {
    fn axes(&self) -> (Vec3, Vec3, Vec3) {
        let m = rotator_matrix((self.pitch / ROT_TO_RAD) as i32, (self.yaw / ROT_TO_RAD) as i32, 0);
        (m.x_axis.truncate(), m.y_axis.truncate(), m.z_axis.truncate())
    }

    /// The same view seen from `origin` (for the sky).
    fn from(&self, origin: Vec3) -> Camera {
        Camera { pos: origin, yaw: self.yaw, pitch: self.pitch }
    }

    fn view_proj(&self, width: u32, height: u32) -> Mat4 {
        let (fwd, right, up) = self.axes();
        // Camera space: x right, y up, z forward (left-handed, like the game).
        let view = Mat4::from_cols(right.extend(0.0), up.extend(0.0), fwd.extend(0.0), glam::Vec4::W)
            .transpose()
            * Mat4::from_translation(-self.pos);
        let aspect = width as f32 / height.max(1) as f32;
        // The game's field of view is 90° across the screen.
        let fov_y = 2.0 * (1.0f32 / aspect).atan();
        Mat4::perspective_infinite_reverse_lh(fov_y, aspect, 4.0) * view
    }
}

struct Loaded {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    batches: Vec<DrawBatch>,
    textures: Vec<Image>,
    lightmaps: Vec<Image>,
    actors: actors::Actors,
    animations: Vec<textures::Animation>,
    fires: Vec<textures::FireTexture>,
    start: Camera,
    /// Where the sky is seen from (the sky zone's position), if the map has one.
    sky_origin: Option<Vec3>,
}

fn save_png(path: &Path, img: &Image) -> Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&img.rgba)?;
    Ok(())
}

fn load(data: &Path, map: &str) -> Result<Loaded> {
    let t = Instant::now();
    let level: LevelFile = load_binary(&data.join("levels").join(format!("{map}.level")))
        .with_context(|| format!("loading map {map} (run `hp1-tools convert` first)"))?;
    let classes: ClassTable = serde_json::from_slice(&std::fs::read(data.join("classes.json"))?)?;
    let mut textures = Textures::open(data)?;

    let light_maps = build_level_light_maps(&level, &classes);
    let mesh = build_level_mesh(&level, &light_maps, |path| {
        let t = textures.info(path)?;
        Some(TextureSize {
            width: t.width,
            height: t.height,
            draw_scale: textures.draw_scale(path),
            masked: t.masked,
        })
    });

    let mut indices = Vec::new();
    let mut batches = Vec::new();
    for b in &mesh.batches {
        let tex = b.texture.as_deref().map_or(0, |t| textures.get(t));
        let blend = match b.style {
            Style::Opaque => Blend::Opaque,
            Style::Sky => Blend::Sky,
            Style::Masked => Blend::Masked,
            Style::Translucent => Blend::Translucent,
            Style::Modulated => Blend::Modulated,
        };
        batches.push(DrawBatch {
            texture: tex,
            lightmap: b.lightmap_page,
            blend,
            first_index: indices.len() as u32,
            index_count: b.indices.len() as u32,
        });
        indices.extend_from_slice(&b.indices);
    }
    let vertices: Vec<Vertex> = mesh
        .vertices
        .iter()
        .map(|v| Vertex { pos: v.pos, uv: v.uv, lm_uv: v.lm_uv, color: [1.0; 3] })
        .collect();
    let lightmaps: Vec<Image> =
        mesh.light_pages.into_iter().map(|p| Image { width: p.size, height: p.size, rgba: p.rgba }).collect();

    // Start where the player starts, at eye height.
    let start = level
        .actors
        .iter()
        .flatten()
        .find(|a| a.class.ends_with(".PlayerStart"))
        .map(|a| {
            let p = ActorProps::new(a, &classes);
            let [pitch, yaw, _] = p.rotator("Rotation");
            Camera {
                pos: Vec3::from(p.vec3("Location")) + Vec3::Z * 40.0,
                yaw: yaw as f32 * ROT_TO_RAD,
                pitch: pitch as f32 * ROT_TO_RAD,
            }
        })
        .unwrap_or(Camera { pos: Vec3::ZERO, yaw: 0.0, pitch: 0.0 });

    let sky_origin = level
        .actors
        .iter()
        .flatten()
        .find(|a| classes.is_a(&a.class, "Engine.SkyZoneInfo"))
        .map(|a| Vec3::from(ActorProps::new(a, &classes).vec3("Location")));

    let actors = actors::Actors::load(data, &level, &classes, &mut textures)?;
    let animations = textures.animations();
    let mut fires = textures.fires();
    if !fires.is_empty() {
        let names: Vec<&str> = fires.iter().map(|f| f.path.as_str()).collect();
        println!("fire textures: {}", names.join(", "));
    }
    // Show the primed fires from the start (also in screenshots).
    for f in &mut fires {
        let img = f.image();
        if let Some(slot) = textures.images.get_mut(f.index as usize) {
            *slot = img;
        }
    }
    let rebuild = Instant::now();
    actors.build_moving(1.0);
    println!("{} moving objects, rebuilt per frame in {:.2?}", actors.moving_count(), rebuild.elapsed());

    println!(
        "{map}: {} triangles in {} batches, {} objects ({} without a mesh file), {} textures ({} missing), {} light map pages, loaded in {:.2?}",
        indices.len() / 3,
        batches.len(),
        actors.count,
        actors.skipped,
        textures.images.len() - 1,
        textures.missing,
        lightmaps.len(),
        t.elapsed()
    );
    Ok(Loaded {
        vertices,
        indices,
        batches,
        textures: textures.images,
        lightmaps,
        actors,
        animations,
        fires,
        start,
        sky_origin,
    })
}

/// Uploads the level; geometry slot 1 holds the still objects, slot 2 the moving ones.
fn upload(gpu: &Gpu, renderer: &WorldRenderer, l: &Loaded) -> Scene {
    let mut scene = renderer.upload(
        gpu,
        &SceneDesc {
            vertices: &l.vertices,
            indices: &l.indices,
            batches: l.batches.clone(),
            textures: &l.textures,
            lightmaps: &l.lightmaps,
        },
    );
    let still = l.actors.build_still();
    scene.geometry.push(Geometry::new(gpu, &still.vertices, &still.indices, still.batches));
    let moving = l.actors.build_moving(0.0);
    scene.geometry.push(Geometry::new(gpu, &moving.vertices, &moving.indices, moving.batches));
    scene
}

fn update_moving(gpu: &Gpu, scene: &mut Scene, l: &mut Loaded, time: f32) {
    let moving = l.actors.build_moving(time);
    scene.geometry[2].update(gpu, &moving.vertices, &moving.indices, moving.batches);
    for a in &l.animations {
        let frame = (time * a.fps) as usize % a.frames.len();
        if let Some(slot) = scene.texture_map.get_mut(a.base as usize) {
            *slot = a.frames[frame];
        }
    }
    for f in &mut l.fires {
        if let Some(img) = f.update(time) {
            scene.update_texture(gpu, f.index, &img);
        }
    }
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let loaded = load(&args.data, &args.map)?;
    let mut camera = loaded.start;
    if let Some([x, y, z, yaw, pitch]) = args.camera {
        camera = Camera { pos: Vec3::new(x, y, z), yaw: yaw.to_radians(), pitch: pitch.to_radians() };
    }
    let mut loaded = Loaded { start: Camera { pos: Vec3::ZERO, yaw: 0.0, pitch: 0.0 }, ..loaded };

    if let Some(out) = &args.shot {
        let gpu = Gpu::new(hp1_render::create_instance(), None).map_err(|e| anyhow!(e))?;
        let renderer = WorldRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let scene = upload(&gpu, &renderer, &loaded);
        let (w, h) = args.size;
        let img = hp1_render::render_offscreen(
            &gpu,
            &renderer,
            &scene,
            w,
            h,
            camera.view_proj(w, h),
            loaded.sky_origin.map(|o| camera.from(o).view_proj(w, h)),
        );
        save_png(out, &img)?;
        println!("wrote {}", out.display());
        return Ok(());
    }

    let mut platform =
        hp1_platform::Platform::new(&format!("HP1 viewer — {}", args.map), args.size.0, args.size.1)
            .map_err(|e| anyhow!(e))?;
    let instance = hp1_render::create_instance();
    // SAFETY: the window outlives the surface; both live until the end of `main`.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_display_and_window(
            &platform.window,
            &platform.window,
        )?)
    }?;
    let gpu = Gpu::new(instance, Some(&surface)).map_err(|e| anyhow!(e))?;
    let (mut width, mut height) = platform.pixel_size();
    let caps = surface.get_capabilities(&gpu.adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
        .unwrap_or(caps.formats[0]);
    let mut config =
        surface.get_default_config(&gpu.adapter, width, height).context("surface not supported")?;
    config.format = format;
    config.present_mode = wgpu::PresentMode::AutoVsync;
    surface.configure(&gpu.device, &config);
    let renderer = WorldRenderer::new(&gpu, format);
    let mut scene = upload(&gpu, &renderer, &loaded);
    let mut depth = hp1_render::create_depth(&gpu, width, height);

    platform.set_mouse_captured(true);
    let mut last = Instant::now();
    let started = Instant::now();
    'running: loop {
        for event in platform.events.poll_iter() {
            match event {
                Event::Quit { .. } | Event::KeyDown { keycode: Some(Keycode::Escape), .. } => break 'running,
                Event::KeyDown { keycode: Some(Keycode::C), repeat: false, .. } => println!(
                    "--camera {:.0},{:.0},{:.0},{:.1},{:.1}",
                    camera.pos.x,
                    camera.pos.y,
                    camera.pos.z,
                    camera.yaw.to_degrees(),
                    camera.pitch.to_degrees()
                ),
                Event::MouseMotion { xrel, yrel, .. } => {
                    camera.yaw += xrel * 0.003;
                    camera.pitch = (camera.pitch - yrel * 0.003).clamp(-1.55, 1.55);
                }
                Event::Window { win_event: WindowEvent::PixelSizeChanged(w, h), .. } => {
                    (width, height) = (w.max(1) as u32, h.max(1) as u32);
                    config.width = width;
                    config.height = height;
                    surface.configure(&gpu.device, &config);
                    depth = hp1_render::create_depth(&gpu, width, height);
                }
                _ => {}
            }
        }
        let dt = last.elapsed().as_secs_f32().min(0.1);
        last = Instant::now();
        let keys = platform.events.keyboard_state();
        let (fwd, right, _) = camera.axes();
        let mut wish = Vec3::ZERO;
        for (key, dir) in [
            (Scancode::W, fwd),
            (Scancode::S, -fwd),
            (Scancode::D, right),
            (Scancode::A, -right),
            (Scancode::E, Vec3::Z),
            (Scancode::Q, -Vec3::Z),
        ] {
            if keys.is_scancode_pressed(key) {
                wish += dir;
            }
        }
        let speed = if keys.is_scancode_pressed(Scancode::LShift) { 2400.0 } else { 600.0 };
        camera.pos += wish.normalize_or_zero() * speed * dt;

        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&gpu.device, &config);
                continue;
            }
            // Hidden or not ready: wait instead of spinning.
            _ => {
                std::thread::sleep(std::time::Duration::from_millis(16));
                continue;
            }
        };
        update_moving(&gpu, &mut scene, &mut loaded, started.elapsed().as_secs_f32());
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let sky = loaded.sky_origin.map(|o| camera.from(o).view_proj(width, height));
        renderer.draw(
            &gpu,
            &mut encoder,
            &scene,
            &view,
            &depth,
            camera.view_proj(width, height),
            sky,
            [0.0; 3],
        );
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
    }
    Ok(())
}
