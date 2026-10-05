use std::collections::HashMap;

use asset_game::MenuCatalog;
use bevy::prelude::*;

use crate::draw2d::{Draw2dCmd, Draw2dList, Draw2dOp, Draw2dProvenance, tessellate_fonts};
use crate::gpu_list::{HudTessPass, TessJob};
use crate::images::HudImages;

#[derive(Component)]
pub(crate) struct Sm64HudRaster;

pub(crate) fn update(
    surface: Res<crate::surface::Hud2dSurface>,
    view: Res<frame::Sm64HudView>,
    catalog: Option<Res<MenuCatalog>>,
    mut pass: ResMut<HudTessPass>,
    mut hud_images: ResMut<HudImages>,
    mut images: ResMut<Assets<Image>>,
) {
    pass.sm64 = TessJob::Hide;
    if !view.active || !surface.is_ready() {
        return;
    }

    let Some(catalog) = catalog else {
        return;
    };
    let font_name = crate::font_overlay::HUD_SMALL_FONT;
    let Some(font) = catalog.font(font_name) else {
        return;
    };

    // SM64 health is 0x100 per wedge, with normal maximum 0x880. Round the
    // partial wedge up exactly like the bridge-side HUD state did.
    let wedges = ((view.health.max(0) + 0xff) / 0x100).clamp(0, 8);
    let text = format!(
        "MARIO   POWER {}/8     COIN x {}",
        wedges,
        view.coins.max(0)
    );

    let text_scale = 0.34;
    let nscale = hud_iw4::normalized_text_scale(font.pixel_height, text_scale);
    let rect = surface.apply_rect(14.0, 24.0, nscale, nscale, 1, 1);
    let material = asset_core::AssetRef::bare_name(&font.material).to_owned();
    if hud_images
        .get(crate::images::HUD_CHROME_NAMESPACE, &material, &mut images)
        .is_none()
    {
        return;
    }

    let list = Draw2dList {
        cmds: vec![Draw2dCmd {
            material_namespace: crate::images::HUD_CHROME_NAMESPACE,
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            s0: 0.0,
            t0: 0.0,
            s1: 1.0,
            t1: 1.0,
            color: [1.0, 1.0, 1.0, 1.0],
            material: material.clone(),
            op: Draw2dOp::TextRun {
                font: font_name.to_owned(),
                scale: nscale,
                text,
                loc_key: "SM64_NATIVE_HUD".to_owned(),
                style: 1,
                fx: None,
                glow: None,
            },
            provenance: Draw2dProvenance::CgDraw { site: "sm64_hud" },
            layer: 1,
        }],
    };
    let mut fonts = HashMap::new();
    fonts.insert(font_name.to_owned(), font);
    let (quads, _) = tessellate_fonts(&list, &fonts);
    if !quads.is_empty() {
        pass.sm64 = TessJob::Quads(quads);
    }
}
