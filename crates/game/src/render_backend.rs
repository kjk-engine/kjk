//! The graphics API kjk draws with.
//!
//! kjk draws with Vulkan, DX12 or Metal. Some old PCs have a graphics card
//! with no Vulkan or DX12 driver, such as NVIDIA's Fermi cards (the GT 720M)
//! or Intel's HD 4000 and older on Windows. On those PCs wgpu used to settle
//! on a software renderer that runs on the CPU at under one frame a second.
//! When no real graphics card answers on Vulkan or DX12, kjk also offers
//! OpenGL, which those cards do support. wgpu ranks a real card above a
//! software renderer, so OpenGL wins on such a PC and loses nowhere else.
//!
//! A `WGPU_BACKEND` set by the player (for example `WGPU_BACKEND=gl`) still
//! picks the API by itself.
//!
//! On OpenGL every camera draws into a 16-bit float texture (Bevy's `Hdr`).
//! kjk's world cameras keep sRGB-encoded values in an `Rgba8Unorm` texture
//! (`scene::encoded_view`), and Bevy creates that texture with an sRGB view
//! format beside it. wgpu's OpenGL backend cannot give a texture a second
//! view format, so creating it fails and the game quits at the main menu.
//! A float texture needs no second format. Bevy's final copy to the window
//! still decodes the stored sRGB values in its shader, so the picture is the
//! same.
//!
//! On OpenGL kjk's world shader also reads the shadow maps through one
//! sampler only. Soft shadows search the map for casters with a second,
//! plain sampler beside the comparing one, and OpenGL ties a texture to a
//! single sampler, so the shader failed to build and the game quit. Without
//! the search the shadows keep their filtered edges but lose the penumbra
//! that grows with the distance to the caster.
//!
//! When OpenGL may be picked, Bevy builds its pipelines on the render thread
//! one at a time. Bevy normally builds them on worker threads in parallel.
//! wgpu's OpenGL backend has one context that every thread must lock, and on
//! Windows a thread gives up after waiting one second. A slow card builds a
//! large shader in more than a second, so the other threads gave up and the
//! game crashed at the main menu.

use bevy::camera::Hdr;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the renderer started on OpenGL. Set once the renderer is up,
/// before any pipeline is built.
static ON_GL: AtomicBool = AtomicBool::new(false);

/// Whether the renderer runs on OpenGL.
pub fn on_gl() -> bool {
    ON_GL.load(Ordering::Relaxed)
}

/// Fits kjk's cameras to the graphics API the renderer started on.
pub struct RenderBackendPlugin;

impl Plugin for RenderBackendPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(float_target_on_gl);
    }

    /// Runs after Bevy's renderer has stored the adapter it started on.
    fn finish(&self, app: &mut App) {
        let gl = app
            .world()
            .get_resource::<RenderAdapterInfo>()
            .is_some_and(|adapter| Backends::from(adapter.backend) == Backends::GL);
        ON_GL.store(gl, Ordering::Relaxed);
    }
}

/// Gives a new camera a float main texture when the renderer runs on
/// OpenGL (see the module's notes).
fn float_target_on_gl(add: On<Add, Camera>, mut commands: Commands) {
    if on_gl() {
        commands.entity(add.entity).insert(Hdr);
    }
}

/// Bevy's renderer, on the graphics APIs this PC can draw with.
pub fn render_plugin() -> RenderPlugin {
    let mut settings = WgpuSettings::default();
    if Backends::from_env().is_none() {
        settings.backends = Some(backends());
    }
    let gl_possible = settings
        .backends
        .is_some_and(|backends| backends.contains(Backends::GL));
    RenderPlugin {
        render_creation: RenderCreation::Automatic(Box::new(settings)),
        synchronous_pipeline_compilation: gl_possible,
        ..Default::default()
    }
}

/// Vulkan, DX12 and Metal, with OpenGL as well when none of them reaches a
/// real graphics card.
fn backends() -> Backends {
    #[cfg(not(target_os = "macos"))]
    if !hardware_adapter(Backends::PRIMARY) {
        bevy::log::warn!("no graphics card answers on Vulkan or DX12: offering OpenGL as well");
        return Backends::PRIMARY | Backends::GL;
    }
    Backends::PRIMARY
}

/// Whether any of these APIs reaches a graphics card that is not a software
/// renderer (Microsoft Basic Render Driver, SwiftShader, llvmpipe).
#[cfg(not(target_os = "macos"))]
fn hardware_adapter(backends: Backends) -> bool {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapters = bevy::tasks::block_on(instance.enumerate_adapters(backends));
    adapters
        .iter()
        .any(|adapter| adapter.get_info().device_type != wgpu::DeviceType::Cpu)
}
