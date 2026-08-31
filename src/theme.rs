//! Mieli's visual language.
//!
//! The palette borrows the calm, paper-like neutrals from mikes.cv while
//! keeping blue for navigation and selection, and green for success states.
//! All colors are installed through Bezel's appearance-aware palette hook so
//! system light/dark mode keeps the same semantic roles.

use bezel::{
    gpui::{Hsla, hsla},
    theme::{Appearance, Theme},
};

fn tone(hue: f32, saturation: f32, lightness: f32) -> Hsla {
    hsla(hue, saturation, lightness, 1.0)
}

fn wash(color: Hsla, opacity: f32) -> Hsla {
    color.opacity(opacity)
}

/// Build Mieli's palette for the requested system appearance.
pub fn palette(appearance: Appearance) -> Theme {
    let mut theme = Theme::for_appearance(appearance);

    match appearance {
        Appearance::Light => light_palette(&mut theme),
        Appearance::Dark => dark_palette(&mut theme),
    }

    theme
}

fn light_palette(theme: &mut Theme) {
    // Warm neutral paper canvas with one calm cobalt accent.
    let background = tone(0.10, 0.18, 0.965); // #F8F6F4
    let ink = tone(0.145833, 0.210526, 0.074510); // #17160F
    let strong_ink = tone(0.10, 0.10, 0.055); // #100F0E
    let body = tone(0.10, 0.06, 0.30); // #504D4A
    let accessible_faint = tone(0.10, 0.04, 0.42); // #706B68
    let metadata = tone(0.10, 0.05, 0.55); // #928C88
    let keyline = tone(0.08, 0.09, 0.83); // #D8D3D0
    let card = tone(0.10, 0.20, 0.985); // #FCFBF9
    let sunken = tone(0.08, 0.10, 0.92); // #ECEAE7
    let chip = tone(0.08, 0.08, 0.95); // #F4F3F1
    let blue = tone(0.64, 0.70, 0.60); // #5266E0
    let green = tone(0.401042, 0.680851, 0.368627); // #1E9E52

    theme.bg = background;
    theme.surface = chip;
    theme.surface_raised = sunken;
    theme.surface_card = card;
    theme.surface_dialog = card;
    theme.surface_overlay = card;
    theme.surface_raised_hover = tone(0.08, 0.10, 0.90); // #E8E5E2

    theme.element_hover = wash(ink, 0.035);
    theme.element_active = wash(ink, 0.085);
    theme.border = wash(keyline, 0.82);
    theme.border_strong = keyline;

    theme.text = ink;
    theme.text_muted = body;
    theme.text_faint = accessible_faint;
    theme.text_dim = metadata;
    theme.solid = strong_ink;
    theme.on_solid = card;

    // Blue is the structural/action color; green is reserved for success.
    theme.accent = blue;
    theme.accent_strong = blue;
    theme.on_accent = card;
    theme.success = green;
    theme.success_muted = wash(green, 0.18);

    theme.input_bg = card;
    theme.selection = wash(blue, 0.22);
    theme.cursor = wash(blue, 0.72);
    theme.caret = ink;
    theme.code_text = strong_ink;
    theme.code_wash = wash(ink, 0.05);
}

fn dark_palette(theme: &mut Theme) {
    // Warm charcoal surfaces preserve the same hierarchy after inversion.
    let background = tone(0.083333, 0.041667, 0.094118); // #191817
    let ink = tone(0.10, 0.12, 0.93); // #F1EFEB
    let strong_ink = tone(0.10, 0.10, 0.95); // #F5F4F2
    let body = tone(0.10, 0.03, 0.73); // #BEBBB6
    let accessible_faint = tone(0.10, 0.02, 0.80); // #D1CECA
    let metadata = tone(0.08, 0.03, 0.52); // #88837F
    let keyline = tone(0.08, 0.04, 0.24); // #403D3A
    let keyline_strong = tone(0.08, 0.05, 0.30); // #4F4C48
    let surface = tone(0.08, 0.03, 0.14); // #252321
    let raised = tone(0.08, 0.04, 0.18); // #2F2C2A
    let blue = tone(0.63, 0.72, 0.68); // #738CE8
    let green = tone(0.398148, 0.576000, 0.490196); // #35C56D

    theme.bg = background;
    theme.surface = surface;
    theme.surface_raised = raised;
    theme.surface_card = surface;
    theme.surface_dialog = surface;
    theme.surface_overlay = raised;
    theme.surface_raised_hover = tone(0.055556, 0.040000, 0.195000);

    theme.element_hover = wash(strong_ink, 0.045);
    theme.element_active = wash(strong_ink, 0.095);
    theme.border = wash(keyline, 0.90);
    theme.border_strong = wash(keyline_strong, 0.98);

    theme.text = ink;
    theme.text_muted = body;
    theme.text_faint = accessible_faint;
    theme.text_dim = metadata;
    theme.solid = strong_ink;
    theme.on_solid = background;

    theme.accent = blue;
    theme.accent_strong = blue;
    theme.on_accent = strong_ink;
    theme.success = green;
    theme.success_muted = wash(green, 0.22);

    theme.input_bg = surface;
    theme.selection = wash(blue, 0.30);
    theme.cursor = wash(blue, 0.78);
    theme.caret = strong_ink;
    theme.code_text = strong_ink;
    theme.code_wash = wash(strong_ink, 0.08);
}
