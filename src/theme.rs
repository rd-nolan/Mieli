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
    // Warm paper canvas, ink-like text, and a restrained lavender keyline.
    let background = tone(0.0, 0.333333, 0.970588); // #FAF5F5
    let ink = tone(0.145833, 0.210526, 0.074510); // #17160F
    let strong_ink = tone(0.0, 0.0, 0.023529); // #060606
    let body = tone(0.583333, 0.014706, 0.266667); // #434445
    let accessible_faint = tone(0.0, 0.0, 0.400000); // #666666
    let metadata = tone(0.150000, 0.041667, 0.529412); // #8C8B82
    let keyline = tone(0.692308, 0.136842, 0.813725); // #CBC9D6
    let card = tone(0.0, 0.0, 1.0); // #FFFFFF
    let sunken = tone(0.0, 0.135135, 0.927451); // #EFEAEA
    let chip = tone(0.0, 0.071429, 0.945098); // #F2F0F0
    let blue = tone(0.643389, 0.832558, 0.578431); // #3A53ED
    let green = tone(0.401042, 0.680851, 0.368627); // #1E9E52

    theme.bg = background;
    theme.surface = chip;
    theme.surface_raised = sunken;
    theme.surface_card = card;
    theme.surface_dialog = card;
    theme.surface_overlay = card;
    theme.surface_raised_hover = tone(0.0, 0.115000, 0.900000);

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
    let ink = tone(0.111111, 0.257143, 0.931373); // #F2EFE9
    let strong_ink = tone(0.100000, 0.200000, 0.950980); // #F5F3F0
    let body = tone(0.111111, 0.020979, 0.719608); // #B9B8B6
    let accessible_faint = tone(0.111111, 0.015000, 0.820000); // #D5D3D0
    let metadata = tone(0.0, 0.004016, 0.488235); // #7D7C7C
    let keyline = tone(0.690476, 0.058824, 0.233333); // #39383F
    let keyline_strong = tone(0.690476, 0.055000, 0.290000); // #4A4848
    let surface = tone(0.083333, 0.028571, 0.137255); // #242322
    let raised = tone(0.055556, 0.036145, 0.162745); // #2B2928
    let blue = tone(0.613744, 1.0, 0.586275); // #2C6FFF
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
