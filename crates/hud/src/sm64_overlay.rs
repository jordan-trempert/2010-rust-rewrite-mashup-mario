use bevy::prelude::*;
use crate::draw2d::{Draw2dProvenance, Draw2dQuad};
use crate::gpu_list::{HudTessPass, TessJob};
use crate::images::HudImages;

#[derive(Component)]
pub(crate) struct Sm64HudRaster;

pub(crate) fn update(
    surface: Res<crate::surface::Hud2dSurface>,
    view: Res<frame::Sm64HudView>,
    geometry: Res<frame::Sm64HudGeometry>,
    mut pass: ResMut<HudTessPass>,
    mut hud_images: ResMut<HudImages>,
) {
    pass.sm64 = TessJob::Hide;
    if !view.active || !surface.is_ready() { return; }
    // Preserve native glyphs, meter animation, dialog paging and draw order.
    let quads = geometry.0.iter().map(|triangle| {
        let material = if let Some(image) = &triangle.image {
            let name = format!("sm64/native_hud/{:?}", image.id());
            hud_images.insert_runtime(&name, image.clone());
            name
        } else { "white".to_owned() };
        // Degenerate second triangle, using the retained HUD triangle pipeline.
        let indices = [0, 1, 2, 2];
        Draw2dQuad {
            xy: indices.map(|j| [triangle.xy[j][0] * surface.width(), triangle.xy[j][1] * surface.height()]),
            st: indices.map(|j| triangle.uv[j]),
            color: triangle.rgba[0].map(|v| v as f32 / 255.0),
            material,
            material_namespace: crate::images::HUD_CHROME_NAMESPACE,
            provenance: Draw2dProvenance::CgDraw { site: "sm64_native" },
            layer: 1,
            clip: None,
        }
    }).collect::<Vec<_>>();
    if !quads.is_empty() { pass.sm64 = TessJob::Quads(quads); }
}
