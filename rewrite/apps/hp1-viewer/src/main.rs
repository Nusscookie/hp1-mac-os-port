//! Fly through a converted map.
//!
//!   hp1-viewer <converted folder> <map> [--shot out.png] [--camera x,y,z,yaw,pitch] [--view x,y,z,yaw,pitch] [--size WxH]
//!              [--music <name>] [--mute] [--trigger <tag>] [--run <seconds>]
//!
//! Mouse looks around, WASD moves, Q/E go down/up, Shift is faster, C prints the camera (for
//! `--camera`), Escape quits. F switches between flying and walking as Harry seen from behind
//! (mouse turns him, WASD runs and strafes, Space jumps). `--walk` takes the screenshot that way. With `--shot` it renders one image without opening a window, after
//! optionally sending `--trigger` and running the world for `--run` seconds.

mod actors;
mod hud;
mod movers;
mod particles;
mod player;
mod sound;
mod textures;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use glam::{Mat4, Vec3};
use hp1_data::level::LevelFile;
use hp1_data::mesh::load_binary;
use hp1_data::{ClassTable, GestureTable};
use hp1_engine::gesture::Pattern;
use hp1_engine::level_mesh::{LightPacker, build_model_mesh};
use hp1_engine::lighting::build_level_light_maps;
use hp1_engine::props::ActorProps;
use hp1_engine::units::{ROT_TO_RAD, rotator_matrix};
use hp1_engine::world::{Entity, Output, WandInput, World};
use hp1_platform::sdl3::event::{Event, WindowEvent};
use hp1_platform::sdl3::keyboard::{Keycode, Scancode};
use hp1_platform::sdl3::mouse::MouseButton;
use hp1_render::{DrawBatch, Geometry, Gpu, Image, Scene, SceneDesc, Vertex, WorldRenderer, wgpu};
use textures::Textures;

struct Args {
    data: PathBuf,
    map: String,
    shot: Option<PathBuf>,
    camera: Option<[f32; 5]>,
    /// Shot mode: where the picture is taken from at the end, whatever the game did.
    view: Option<[f32; 5]>,
    size: (u32, u32),
    music: Option<String>,
    mute: bool,
    trigger: Vec<String>,
    run: f32,
    walk: bool,
    /// Shot mode: Harry walks forward the whole time.
    forward: bool,
    /// Testing: the level's cutscenes don't run.
    no_cutscenes: bool,
    /// Shot mode: spell lessons are traced by themselves in this many seconds, up to a part of
    /// the pattern (`seconds[,part]`).
    trace: Option<(f32, f32)>,
}

fn parse_args() -> Result<Args> {
    let mut it = std::env::args().skip(1);
    let usage = "usage: hp1-viewer <converted folder> <map> [--shot out.png] [--camera x,y,z,yaw,pitch] [--size WxH] [--music <name>] [--mute] [--trigger <tag>] [--run <seconds>] [--walk] [--forward] [--no-cutscenes] [--trace <seconds>[,part]]";
    let data = PathBuf::from(it.next().context(usage)?);
    let map = it.next().context(usage)?;
    let mut args = Args {
        data,
        map,
        shot: None,
        camera: None,
        view: None,
        size: (1280, 800),
        music: None,
        mute: false,
        trigger: Vec::new(),
        run: 0.0,
        walk: false,
        forward: false,
        no_cutscenes: false,
        trace: None,
    };
    while let Some(a) = it.next() {
        if a == "--forward" || a == "--no-cutscenes" {
            if a == "--forward" {
                args.forward = true;
            } else {
                args.no_cutscenes = true;
            }
            continue;
        }
        if a == "--mute" || a == "--walk" {
            if a == "--mute" {
                args.mute = true;
            } else {
                args.walk = true;
            }
            continue;
        }
        let v = it.next().with_context(|| format!("{a} needs a value"))?;
        match a.as_str() {
            "--shot" => args.shot = Some(v.into()),
            "--music" => args.music = Some(v),
            "--trigger" => args.trigger.push(v),
            "--run" => args.run = v.parse()?,
            "--trace" => {
                let (secs, part) = v.split_once(',').unwrap_or((&v, "1"));
                args.trace = Some((secs.trim().parse()?, part.trim().parse()?));
            }
            "--camera" | "--view" => {
                let n: Vec<f32> = v.split(',').map(|s| s.trim().parse()).collect::<Result<_, _>>()?;
                let n: [f32; 5] = n.try_into().map_err(|_| anyhow!("{a} wants 5 numbers"))?;
                if a == "--camera" { args.camera = Some(n) } else { args.view = Some(n) }
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

    /// A camera at the view's position looking at its target.
    /// The view the world uses for things flying to the HUD.
    fn view(&self) -> hp1_engine::camera::View {
        hp1_engine::camera::View { pos: self.pos, target: self.pos + self.axes().0 }
    }

    fn looking(v: hp1_engine::camera::View) -> Camera {
        let (yaw, pitch) = v.angles();
        Camera { pos: v.pos, yaw, pitch }
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
    particles: particles::ParticleDraw,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    batches: Vec<DrawBatch>,
    textures: Vec<Image>,
    lightmaps: Vec<Image>,
    actors: actors::Actors,
    animations: Vec<textures::Animation>,
    fires: Vec<textures::FireTexture>,
    movers: movers::Movers,
    /// Whether Harry is drawn (walking mode, or the level has its own Harry).
    show_player: bool,
    /// The running level (it owns the level data and class table).
    world: World,
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
    let mut packer = LightPacker::default();
    let mesh =
        build_model_mesh(&level.model, &light_maps, &mut packer, |p| movers::texture_size(&textures, p));
    let (vertices, indices, batches) = movers::to_draw(&mesh, &mut textures);
    let world = World::new(level.clone(), classes.clone());
    let movers = movers::Movers::build(&world, &level, &classes, &mut packer, &mut textures);
    let lightmaps: Vec<Image> =
        packer.pages.into_iter().map(|p| Image { width: p.size, height: p.size, rgba: p.rgba }).collect();

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

    let mut actors = actors::Actors::load(data, &level, &classes, &mut textures)?;
    if !actors.load_player(data, &level, &classes, &mut textures, "HarryPotter.Harry")? {
        eprintln!("Harry's mesh is missing; walking mode shows no character");
    }
    let particles = particles::ParticleDraw::load(&level, &classes, &mut textures);
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
        "{map}: {} triangles in {} batches, {} objects ({} without a mesh file), {} movers, {} textures ({} missing), {} light map pages, loaded in {:.2?}",
        indices.len() / 3,
        batches.len(),
        actors.count,
        actors.skipped,
        movers.count(),
        textures.images.len() - 1,
        textures.missing,
        lightmaps.len(),
        t.elapsed()
    );
    Ok(Loaded {
        particles,
        vertices,
        indices,
        batches,
        textures: textures.images,
        lightmaps,
        actors,
        animations,
        fires,
        movers,
        show_player: false,
        world,
        start,
        sky_origin,
    })
}

/// Uploads the level; geometry slot 1 holds the still objects, slot 2 the moving ones, slot 3 the
/// movers.
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
    scene.geometry.push(Geometry::new(gpu, &[], &[], Vec::new()));
    // Slot 4: Harry; slot 5: particles.
    scene.geometry.push(Geometry::new(gpu, &[], &[], Vec::new()));
    scene.geometry.push(Geometry::new(gpu, &[], &[], Vec::new()));
    scene
}

/// Rebuilds the particles for the view.
fn update_particles(gpu: &Gpu, scene: &mut Scene, l: &Loaded, view: &hp1_engine::camera::View) {
    let built = l.particles.build(&l.world, view);
    scene.geometry[5].update(gpu, &built.vertices, &built.indices, built.batches);
}

fn update_moving(gpu: &Gpu, scene: &mut Scene, l: &mut Loaded, time: f32) {
    let spawned = std::mem::take(&mut l.world.spawned);
    l.actors.follow(&l.world, &spawned);
    for &id in &spawned {
        if let Some(list) = l.actors.sequences(id) {
            l.world.set_sequences(id, list);
        }
    }
    let moving = l.actors.build_moving(time);
    scene.geometry[2].update(gpu, &moving.vertices, &moving.indices, moving.batches);
    if let Some((v, i, b)) = l.movers.build_if_moved(&l.world) {
        scene.geometry[3].update(gpu, &v, &i, b);
    }
    let harry = if l.show_player { l.actors.build_player() } else { actors::Built::default() };
    scene.geometry[4].update(gpu, &harry.vertices, &harry.indices, harry.batches);
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

fn pose_player(l: &mut Loaded, p: &player::Player) {
    l.show_player = true;
    l.actors.pose_player(&l.world.level, &l.world.classes, p.body.pos, p.yaw, &p.anim, p.anim_time);
}

/// Shows Harry where the world has him (not walking: standing, or driven by a cutscene).
fn pose_from_world(l: &mut Loaded, harry: usize) {
    let Some(e) = l.world.get(harry) else { return };
    let (pos, yaw) = (e.pos, e.rot[1] as f32 * ROT_TO_RAD);
    let (anim, time) = e.anim.as_ref().map_or(("breath".to_owned(), 0.0), |a| (a.name.clone(), a.time));
    l.show_player = true;
    l.actors.pose_player(&l.world.level, &l.world.classes, pos, yaw, &anim, time);
}

/// Prints what the world says (subtitles, level changes, script problems) and the cutscene
/// mode switching; sounds are left for the sound code.
fn report(world: &mut World, cut_mode: &mut bool) {
    world.outputs.retain(|o| match o {
        Output::Message { text, seconds } => {
            println!("[{seconds:.1} s] {text}");
            false
        }
        Output::ChangeLevel { map } => {
            println!("cutscene: go to level {map}");
            false
        }
        Output::Note(n) => {
            eprintln!("{n}");
            false
        }
        _ => true,
    });
    if world.cut_mode != *cut_mode {
        *cut_mode = world.cut_mode;
        println!("cutscene mode {} at {:.1} s", if world.cut_mode { "on" } else { "off" }, world.time);
    }
}

/// One frame of the player's view: walking (input unless a cutscene has Harry), and the
/// cutscene camera over everything while it is active.
fn walk_frame(
    world: &mut World,
    p: &mut player::Player,
    harry: usize,
    input: glam::Vec2,
    jump: bool,
    dt: f32,
) -> hp1_engine::camera::View {
    if std::mem::take(&mut world.camera.left) {
        p.cam.pos = world.camera.pos;
    }
    let view = if world.puppet_busy(harry) {
        p.follow_world(world, harry, dt)
    } else {
        if world.take_restart(harry) {
            p.adopt(world, harry);
        }
        p.step(world, harry, input, jump, dt)
    };
    if world.camera.active {
        world.camera.view()
    } else {
        world.camera.follow(view);
        view
    }
}

/// The spell patterns (`gestures.json`).
fn load_gestures(data: &Path) -> HashMap<String, Pattern> {
    let table: GestureTable = std::fs::read(data.join("gestures.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    table.gestures.iter().map(|(k, g)| (k.clone(), Pattern::from_data(g))).collect()
}

/// Shot mode: moves the wand along the lesson's pattern by itself, reaching `part` of it after
/// `seconds`, with the draw button held until then.
fn auto_trace(world: &World, (seconds, part): (f32, f32), traced: &mut f32, dt: f32) -> WandInput {
    let mut input = WandInput { draw: *traced / seconds < part, ..Default::default() };
    if let Some((at, path)) = &world.wand_at
        && let Some(pattern) = world.gestures.get(path)
    {
        *traced += dt;
        let target = pattern.along((*traced / seconds).min(part));
        input.mouse = (target - *at) / hp1_engine::lesson::WAND_PER_PIXEL;
    }
    input
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let loaded = load(&args.data, &args.map)?;
    let mut camera = loaded.start;
    if let Some([x, y, z, yaw, pitch]) = args.camera {
        camera = Camera { pos: Vec3::new(x, y, z), yaw: yaw.to_radians(), pitch: pitch.to_radians() };
    }
    let mut loaded = Loaded { start: Camera { pos: Vec3::ZERO, yaw: 0.0, pitch: 0.0 }, ..loaded };
    // A stand-in for Harry that follows the flying camera, so triggers notice it.
    let ghost = loaded.world.spawn(Entity::pawn("Harry", "HarryPotter.Harry", camera.pos, 15.0, 42.0, true));
    // The level's own Harry is the player, as in the original (its cutscenes name him).
    let harry = match loaded.world.placed_harry() {
        Some(id) => {
            if let Some(e) = loaded.world.get_mut(id) {
                (e.is_pawn, e.is_player, e.collide_actors) = (true, true, true);
            }
            loaded.actors.forget(id);
            loaded.show_player = true;
            id
        }
        None => ghost,
    };
    loaded.world.player = Some(harry);
    for id in loaded.actors.animated() {
        if let Some(list) = loaded.actors.sequences(id) {
            loaded.world.set_sequences(id, list);
        }
    }
    if let Some(list) = loaded.actors.player_sequences() {
        loaded.world.set_sequences(harry, list);
    }
    loaded.world.lines = Box::new(sound::DialogLines::open(&args.data)?);
    loaded.world.gestures = load_gestures(&args.data);
    loaded.actors.drive(&loaded.world);
    if args.no_cutscenes {
        let classes = loaded.world.classes.clone();
        for e in loaded.world.actors.iter_mut().flatten() {
            if classes.is_a(&e.class, "HPBase.CutScene") {
                e.behaviour = None;
            }
        }
    }
    let mut cut_mode = false;

    if let Some(out) = &args.shot {
        let gpu = Gpu::new(hp1_render::create_instance(), None).map_err(|e| anyhow!(e))?;
        let mut renderer = WorldRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let mut hud_draw = hud::HudDraw::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, &args.data);
        let hud_size = [args.size.0 as f32, args.size.1 as f32];
        let mut hud_quads = Vec::new();
        let mut scene = upload(&gpu, &renderer, &loaded);
        for tag in &args.trigger {
            loaded.world.trigger_event(tag, None);
        }
        let mut walker = args.walk.then(|| {
            let start = if harry == ghost || args.camera.is_some() {
                camera.pos - Vec3::Z * player::EYE
            } else {
                loaded.world.get(harry).map_or(camera.pos, |e| e.pos)
            };
            if harry != ghost
                && let Some(e) = loaded.world.get_mut(ghost)
            {
                e.pos = Vec3::splat(1.0e6);
            }
            player::Player::new(&loaded.world, harry, start, camera.yaw)
        });
        let frames = (args.run * 60.0) as usize + if args.walk { 60 } else { 0 };
        let mut traced = 0.0;
        for _ in 0..frames {
            if let Some(trace) = args.trace {
                if loaded.world.wand_wanted {
                    loaded.world.wand_input = auto_trace(&loaded.world, trace, &mut traced, 1.0 / 60.0);
                } else {
                    traced = 0.0;
                }
            }
            loaded.world.tick(1.0 / 60.0);
            if let Some(p) = &mut walker {
                let input = if args.forward { glam::Vec2::X } else { glam::Vec2::ZERO };
                camera = Camera::looking(walk_frame(&mut loaded.world, p, harry, input, false, 1.0 / 60.0));
            } else if loaded.world.camera.active {
                camera = Camera::looking(loaded.world.camera.view());
            }
            loaded.world.view = Some(camera.view());
            report(&mut loaded.world, &mut cut_mode);
            // The HUD's bars move once per drawn frame.
            hud_quads = hud_draw.quads(&gpu, &mut loaded.world, hud_size);
            loaded.world.outputs.retain(|o| !matches!(o, Output::Voice { .. }));
        }
        for o in loaded.world.outputs.drain(..) {
            println!("world: {o:?}");
        }
        if loaded.world.camera.active {
            camera = Camera::looking(loaded.world.camera.view());
            println!("cutscene camera at {:.0}", camera.pos);
        }
        for e in loaded.world.actors.iter().flatten().filter(|e| e.anim.is_some()) {
            let a = e.anim.as_ref().map_or("", |a| a.name.as_str());
            println!("character {} ({}) at {:.0} playing {a}", e.name, e.class, e.pos);
        }
        match &walker {
            Some(p) if !loaded.world.puppet_busy(harry) => {
                pose_player(&mut loaded, p);
                println!("Harry stands at {:?}, playing {}", p.body.pos, p.anim);
            }
            _ if loaded.show_player => pose_from_world(&mut loaded, harry),
            _ => {}
        }
        if let Some([x, y, z, yaw, pitch]) = args.view {
            camera = Camera { pos: Vec3::new(x, y, z), yaw: yaw.to_radians(), pitch: pitch.to_radians() };
        }
        update_moving(&gpu, &mut scene, &mut loaded, args.run.max(1.0));
        update_particles(&gpu, &mut scene, &loaded, &camera.view());
        let alive: usize = loaded.world.particles.values().map(|s| s.particles.len()).sum();
        println!("{} particle systems, {alive} particles", loaded.world.particles.len());
        let (w, h) = args.size;
        renderer.brightness = loaded.world.fade.brightness();
        let img = hp1_render::render_offscreen(
            &gpu,
            &renderer,
            &scene,
            w,
            h,
            camera.view_proj(w, h),
            loaded.sky_origin.map(|o| camera.from(o).view_proj(w, h)),
            Some((&hud_draw.overlay, hud_size, &hud_quads)),
        );
        save_png(out, &img)?;
        println!("wrote {}", out.display());
        if !args.mute {
            // Check the sound setup without playing anything: mix one second and report its level.
            let mixer = std::sync::Arc::new(std::sync::Mutex::new(hp1_audio::Mixer::new(44100)));
            let mut lib = sound::Library::open(&args.data)?;
            let (count, song) = sound::start(
                &mut lib,
                &mixer,
                &loaded.world.level,
                &loaded.world.classes,
                args.music.as_deref(),
                camera.pos,
            );
            let (_, right, _) = camera.axes();
            let mut m = mixer.lock().unwrap_or_else(|e| e.into_inner());
            m.set_listener(camera.pos, right);
            let mut buf = vec![0.0f32; 44100 * 2];
            m.mix(&mut buf);
            let peak = buf.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
            println!(
                "{count} ambient sounds, music: {}, peak level of the first second here: {peak:.2}",
                song.as_deref().unwrap_or("none")
            );
        }
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
    let mut renderer = WorldRenderer::new(&gpu, format);
    let mut hud_draw = hud::HudDraw::new(&gpu, format, &args.data);
    let mut scene = upload(&gpu, &renderer, &loaded);
    let mut depth = hp1_render::create_depth(&gpu, width, height);

    // Sound: ambient sounds of placed objects and one music track.
    let mixer = std::sync::Arc::new(std::sync::Mutex::new(hp1_audio::Mixer::new(44100)));
    let mut lib = sound::Library::open(&args.data)?;
    let mut ambient = std::collections::HashMap::new();
    let _audio = if args.mute {
        None
    } else {
        let m = mixer.clone();
        match platform.open_audio(44100, move |out| m.lock().unwrap_or_else(|e| e.into_inner()).mix(out)) {
            Ok(output) => {
                let (count, song) = sound::start(
                    &mut lib,
                    &mixer,
                    &loaded.world.level,
                    &loaded.world.classes,
                    args.music.as_deref(),
                    camera.pos,
                );
                println!("{count} ambient sounds, music: {}", song.as_deref().unwrap_or("none"));
                Some(output)
            }
            Err(e) => {
                eprintln!("no sound: {e}");
                None
            }
        }
    };

    let mut walker: Option<player::Player> = None;
    let mut jump = false;
    let mut draw_held = false;

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
                Event::KeyDown { keycode: Some(Keycode::F), repeat: false, .. } => {
                    walker = match walker {
                        Some(_) => None,
                        None => {
                            // Walk from where the level's Harry is; without one, from the camera.
                            let start = match loaded.world.get(harry) {
                                Some(e) if harry != ghost => e.pos,
                                _ => camera.pos - Vec3::Z * player::EYE,
                            };
                            if harry != ghost
                                && let Some(e) = loaded.world.get_mut(ghost)
                            {
                                e.pos = Vec3::splat(1.0e6);
                            }
                            Some(player::Player::new(&loaded.world, harry, start, camera.yaw))
                        }
                    };
                    loaded.show_player = walker.is_some() || harry != ghost;
                    println!("{}", if walker.is_some() { "walking" } else { "flying" });
                }
                Event::KeyDown { keycode: Some(Keycode::Space), repeat: false, .. } => jump = true,
                Event::MouseButtonDown { mouse_btn: MouseButton::Right, .. } => draw_held = true,
                Event::MouseButtonUp { mouse_btn: MouseButton::Right, .. } => draw_held = false,
                Event::MouseMotion { xrel, yrel, .. } if loaded.world.wand_wanted => {
                    loaded.world.wand_input.mouse += glam::Vec2::new(xrel, yrel);
                }
                Event::MouseMotion { xrel, yrel, .. } => match &mut walker {
                    Some(p) => {
                        p.yaw += xrel * 0.003;
                        if p.body.vel.truncate().length() < 10.0 {
                            p.cam.tilt -= yrel * 0.003;
                        }
                    }
                    None => {
                        camera.yaw += xrel * 0.003;
                        camera.pitch = (camera.pitch - yrel * 0.003).clamp(-1.55, 1.55);
                    }
                },
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
        let world = &mut loaded.world;
        if world.wand_wanted {
            let pressed = |k| if keys.is_scancode_pressed(k) { 1.0 } else { 0.0 };
            world.wand_input.draw = draw_held;
            world.wand_input.keys = glam::Vec2::new(
                pressed(Scancode::D) - pressed(Scancode::A),
                pressed(Scancode::W) - pressed(Scancode::S),
            );
        }
        world.tick(dt);
        if let Some(p) = &mut walker {
            let pressed = |k| if keys.is_scancode_pressed(k) { 1.0 } else { 0.0 };
            let input = glam::Vec2::new(
                pressed(Scancode::W) - pressed(Scancode::S),
                pressed(Scancode::D) - pressed(Scancode::A),
            );
            camera = Camera::looking(walk_frame(world, p, harry, input, jump, dt));
        } else if world.camera.active {
            camera = Camera::looking(world.camera.view());
        } else {
            world.camera.left = false;
            let speed = if keys.is_scancode_pressed(Scancode::LShift) { 2400.0 } else { 600.0 };
            camera.pos += wish.normalize_or_zero() * speed * dt;
            if let Some(e) = world.get_mut(ghost) {
                e.pos = camera.pos - Vec3::Z * player::EYE;
            }
            let (fwd, ..) = camera.axes();
            world.camera.follow(hp1_engine::camera::View { pos: camera.pos, target: camera.pos + fwd });
        }
        jump = false;
        world.view = Some(camera.view());
        report(world, &mut cut_mode);
        if args.mute {
            world.outputs.clear();
        } else {
            sound::play_outputs(&mut lib, &mixer, &mut world.outputs, &mut ambient);
        }
        mixer.lock().unwrap_or_else(|e| e.into_inner()).set_listener(camera.pos, right);

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
        match &walker {
            Some(p) if !loaded.world.puppet_busy(harry) => pose_player(&mut loaded, p),
            _ if loaded.show_player => pose_from_world(&mut loaded, harry),
            _ => {}
        }
        renderer.brightness = loaded.world.fade.brightness();
        update_moving(&gpu, &mut scene, &mut loaded, started.elapsed().as_secs_f32());
        update_particles(&gpu, &mut scene, &loaded, &camera.view());
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
        // The HUD in window points, so it keeps its size on high-density screens.
        let (pw, ph) = platform.point_size();
        let hud_size = [pw.max(1) as f32, ph.max(1) as f32];
        let quads = hud_draw.quads(&gpu, &mut loaded.world, hud_size);
        hud_draw.overlay.draw(&gpu, &mut encoder, &view, hud_size, &quads);
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
    }
    Ok(())
}
