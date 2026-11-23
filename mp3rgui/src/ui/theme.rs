//! Colours, spacing and text sizes for the light and dark themes. The accent
//! is the website's green, so the app and mp3rgain.tyna.ninja look related.

use egui::style::{Selection, WidgetVisuals, Widgets};
use egui::{Color32, CornerRadius, FontId, Margin, Shadow, Stroke, TextStyle, Theme, Visuals};

pub struct Palette {
    pub base: Color32,
    pub panel: Color32,
    pub stripe: Color32,
    pub well: Color32,
    pub raised: Color32,
    pub raised_hover: Color32,
    pub raised_active: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub text_strong: Color32,
    pub text: Color32,
    pub muted: Color32,
    /// Fill of primary buttons, checked boxes and the progress bar.
    pub accent: Color32,
    /// Text on an `accent` fill.
    pub on_accent: Color32,
    /// The accent as text or a thin stroke on `base`.
    pub accent_text: Color32,
    pub selection: Color32,
    pub warn: Color32,
    pub bad: Color32,
    shadow_alpha: u8,
}

const DARK: Palette = Palette {
    base: Color32::from_rgb(0x10, 0x11, 0x13),
    panel: Color32::from_rgb(0x17, 0x19, 0x1c),
    stripe: Color32::from_rgb(0x15, 0x17, 0x1a),
    well: Color32::from_rgb(0x0b, 0x0c, 0x0d),
    raised: Color32::from_rgb(0x21, 0x24, 0x28),
    raised_hover: Color32::from_rgb(0x2a, 0x2e, 0x33),
    raised_active: Color32::from_rgb(0x33, 0x38, 0x3e),
    line: Color32::from_rgb(0x27, 0x2a, 0x2f),
    line_strong: Color32::from_rgb(0x3a, 0x3f, 0x47),
    text_strong: Color32::from_rgb(0xf5, 0xf6, 0xf7),
    text: Color32::from_rgb(0xc9, 0xcd, 0xd3),
    muted: Color32::from_rgb(0x8b, 0x91, 0x9b),
    accent: Color32::from_rgb(0x3d, 0xdc, 0x84),
    on_accent: Color32::from_rgb(0x05, 0x24, 0x11),
    accent_text: Color32::from_rgb(0x3d, 0xdc, 0x84),
    selection: Color32::from_rgb(0x17, 0x3a, 0x29),
    warn: Color32::from_rgb(0xf2, 0xb8, 0x5b),
    bad: Color32::from_rgb(0xff, 0x6b, 0x6b),
    shadow_alpha: 110,
};

const LIGHT: Palette = Palette {
    base: Color32::from_rgb(0xff, 0xff, 0xff),
    panel: Color32::from_rgb(0xf4, 0xf5, 0xf7),
    stripe: Color32::from_rgb(0xf8, 0xf9, 0xfa),
    well: Color32::from_rgb(0xff, 0xff, 0xff),
    raised: Color32::from_rgb(0xff, 0xff, 0xff),
    raised_hover: Color32::from_rgb(0xee, 0xf0, 0xf2),
    raised_active: Color32::from_rgb(0xe4, 0xe7, 0xeb),
    line: Color32::from_rgb(0xe1, 0xe4, 0xe8),
    line_strong: Color32::from_rgb(0xc5, 0xcb, 0xd3),
    text_strong: Color32::from_rgb(0x11, 0x13, 0x16),
    text: Color32::from_rgb(0x2b, 0x2f, 0x35),
    muted: Color32::from_rgb(0x6b, 0x72, 0x80),
    accent: Color32::from_rgb(0x3d, 0xdc, 0x84),
    on_accent: Color32::from_rgb(0x05, 0x24, 0x11),
    accent_text: Color32::from_rgb(0x0f, 0x7d, 0x3f),
    selection: Color32::from_rgb(0xd6, 0xf6, 0xe4),
    warn: Color32::from_rgb(0xa8, 0x62, 0x00),
    bad: Color32::from_rgb(0xd3, 0x2f, 0x2f),
    shadow_alpha: 40,
};

/// The palette of the theme `ui` is drawn in.
pub fn palette(ui: &egui::Ui) -> &'static Palette {
    of_mode(ui.visuals().dark_mode)
}

/// The palette of the theme the whole window is drawn in.
pub fn palette_of(ctx: &egui::Context) -> &'static Palette {
    of_mode(ctx.style().visuals.dark_mode)
}

fn of_mode(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

pub fn install(ctx: &egui::Context) {
    for (theme, palette) in [(Theme::Dark, &DARK), (Theme::Light, &LIGHT)] {
        ctx.style_mut_of(theme, |style| {
            style.visuals = visuals(theme, palette);
            style.text_styles = [
                (TextStyle::Heading, FontId::proportional(20.0)),
                (TextStyle::Body, FontId::proportional(13.5)),
                (TextStyle::Button, FontId::proportional(13.5)),
                (TextStyle::Monospace, FontId::monospace(12.5)),
                (TextStyle::Small, FontId::proportional(11.0)),
            ]
            .into();
            let spacing = &mut style.spacing;
            spacing.item_spacing = egui::vec2(8.0, 6.0);
            spacing.button_padding = egui::vec2(10.0, 4.0);
            spacing.interact_size = egui::vec2(40.0, 26.0);
            spacing.menu_margin = Margin::same(6);
            spacing.window_margin = Margin::same(16);
            spacing.icon_width = 16.0;
            spacing.icon_width_inner = 9.0;
            spacing.icon_spacing = 6.0;
        });
    }
}

fn visuals(theme: Theme, p: &Palette) -> Visuals {
    let mut v = theme.default_visuals();
    let radius = CornerRadius::same(6);
    let widget = |fill: Color32, stroke: Color32, text: Color32| WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0_f32, stroke),
        fg_stroke: Stroke::new(1.0_f32, text),
        corner_radius: radius,
        expansion: 0.0,
    };
    v.widgets = Widgets {
        noninteractive: widget(p.panel, p.line, p.text),
        inactive: widget(p.raised, p.line_strong, p.text),
        hovered: widget(p.raised_hover, p.line_strong, p.text_strong),
        active: widget(p.raised_active, p.accent_text, p.text_strong),
        open: widget(p.raised_hover, p.line_strong, p.text_strong),
    };
    v.selection = Selection {
        bg_fill: p.selection,
        stroke: Stroke::new(1.0_f32, p.text_strong),
    };
    v.hyperlink_color = p.accent_text;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.bad;
    v.window_fill = p.panel;
    v.panel_fill = p.panel;
    v.faint_bg_color = p.stripe;
    v.extreme_bg_color = p.well;
    v.code_bg_color = p.raised;
    v.window_stroke = Stroke::new(1.0_f32, p.line_strong);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    let shadow = Color32::from_black_alpha(p.shadow_alpha);
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: shadow,
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 16,
        spread: 0,
        color: shadow,
    };
    v.striped = true;
    v
}
