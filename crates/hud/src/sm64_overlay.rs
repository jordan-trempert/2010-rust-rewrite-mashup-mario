use std::collections::HashMap;

use asset_game::MenuCatalog;
use bevy::prelude::*;

use crate::draw2d::{Draw2dCmd, Draw2dList, Draw2dOp, Draw2dProvenance, tessellate_fonts};
use crate::gpu_list::{HudTessPass, TessJob};
use crate::images::HudImages;

#[derive(Component)]
pub(crate) struct Sm64HudRaster;

fn wrap_dialog(text: &str, max_columns: usize) -> String {
    let mut out = String::new();
    for (paragraph_i, paragraph) in text.lines().enumerate() {
        if paragraph_i != 0 {
            out.push('\n');
        }

        let mut column = 0usize;
        for word in paragraph.split_whitespace() {
            let word_len = word.chars().count();
            if column != 0 && column + 1 + word_len > max_columns {
                out.push('\n');
                column = 0;
            } else if column != 0 {
                out.push(' ');
                column += 1;
            }
            out.push_str(word);
            column += word_len;
        }
    }
    out
}

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

    let material = asset_core::AssetRef::bare_name(&font.material).to_owned();
    if hud_images
        .get(crate::images::HUD_CHROME_NAMESPACE, &material, &mut images)
        .is_none()
    {
        return;
    }

    let mut cmds = Vec::new();

    // SM64 health is 0x100 per wedge, with normal maximum 0x880.
    let wedges = ((view.health.max(0) + 0xff) / 0x100).clamp(0, 8);
    let status = format!(
        "MARIO   POWER {}/8     COIN x {}",
        wedges,
        view.coins.max(0)
    );
    let status_scale = hud_iw4::normalized_text_scale(font.pixel_height, 0.34);
    let status_rect = surface.apply_rect(14.0, 24.0, status_scale, status_scale, 1, 1);
    cmds.push(Draw2dCmd {
        material_namespace: crate::images::HUD_CHROME_NAMESPACE,
        x: status_rect.x,
        y: status_rect.y,
        w: status_rect.w,
        h: status_rect.h,
        s0: 0.0,
        t0: 0.0,
        s1: 1.0,
        t1: 1.0,
        color: [1.0, 1.0, 1.0, 1.0],
        material: material.clone(),
        op: Draw2dOp::TextRun {
            font: font_name.to_owned(),
            scale: status_scale,
            text: status,
            loc_key: "SM64_NATIVE_HUD".to_owned(),
            style: 1,
            fx: None,
            glow: None,
        },
        provenance: Draw2dProvenance::CgDraw { site: "sm64_hud" },
        layer: 1,
    });

    /*
     * The ordinary Bevy Text dialog did not participate in the retained COD
     * HUD renderer: its Node background appeared as a black bar but its glyphs
     * never reached the final frame. Draw the native dialog with the same
     * IW4 font tessellator as the status line above.
     */
    if view.dialog_id >= 0 && !view.dialog_text.trim().is_empty() {
        let dialog_scale = hud_iw4::normalized_text_scale(font.pixel_height, 0.30);
        let wrapped = wrap_dialog(view.dialog_text.trim(), 68);
        let dialog = format!("{wrapped}\n\n[USE / FIRE] CONTINUE");
        cmds.push(Draw2dCmd {
            material_namespace: crate::images::HUD_CHROME_NAMESPACE,
            x: surface.width() * 0.16,
            y: surface.height() * 0.73,
            w: dialog_scale,
            h: dialog_scale,
            s0: 0.0,
            t0: 0.0,
            s1: 1.0,
            t1: 1.0,
            color: [1.0, 1.0, 1.0, 1.0],
            material: material.clone(),
            op: Draw2dOp::TextRun {
                font: font_name.to_owned(),
                scale: dialog_scale,
                text: dialog,
                loc_key: "SM64_NATIVE_DIALOG".to_owned(),
                style: 1,
                fx: None,
                glow: None,
            },
            provenance: Draw2dProvenance::CgDraw { site: "sm64_dialog" },
            layer: 2,
        });
    }

    let list = Draw2dList { cmds };
    let mut fonts = HashMap::new();
    fonts.insert(font_name.to_owned(), font);
    let (quads, _) = tessellate_fonts(&list, &fonts);
    if !quads.is_empty() {
        pass.sm64 = TessJob::Quads(quads);
    }
}
