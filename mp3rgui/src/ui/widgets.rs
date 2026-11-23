//! Small widgets drawn in the theme's colours, for what egui's defaults
//! cannot style through `Visuals` alone (an accent-filled check box, a filled
//! button with its own hover states).

use super::theme;
use egui::{
    pos2, vec2, Color32, Response, RichText, Sense, Stroke, StrokeKind, TextStyle, Ui, Widget,
    WidgetInfo, WidgetText, WidgetType,
};

/// A check box whose box is filled with the accent when checked.
pub struct Check<'a> {
    value: &'a mut bool,
    text: WidgetText,
}

impl<'a> Check<'a> {
    pub fn new(value: &'a mut bool, text: impl Into<WidgetText>) -> Self {
        Self {
            value,
            text: text.into(),
        }
    }
}

impl Widget for Check<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let icon = ui.spacing().icon_width;
        let gap = ui.spacing().icon_spacing;
        let wrap_width = ui.available_width() - icon - gap;
        let galley = self
            .text
            .into_galley(ui, None, wrap_width, TextStyle::Button);
        let size = vec2(
            icon + gap + galley.size().x,
            galley.size().y.max(ui.spacing().interact_size.y),
        );
        let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
        if response.clicked() {
            *self.value = !*self.value;
            response.mark_changed();
        }
        let checked = *self.value;
        response.widget_info(|| {
            WidgetInfo::selected(
                WidgetType::Checkbox,
                ui.is_enabled(),
                checked,
                galley.text(),
            )
        });
        if ui.is_rect_visible(rect) {
            let p = theme::palette(ui);
            let visuals = ui.style().interact(&response);
            let box_rect = egui::Rect::from_center_size(
                pos2(rect.left() + icon / 2.0, rect.center().y),
                vec2(icon, icon),
            );
            let painter = ui.painter();
            if checked {
                let fill = if response.hovered() {
                    lighten(p.accent, 0.14)
                } else {
                    p.accent
                };
                painter.rect_filled(box_rect, 4.0, fill);
                let r = box_rect.shrink(4.0);
                painter.line(
                    vec![
                        pos2(r.left(), r.center().y),
                        pos2(r.left() + r.width() * 0.38, r.bottom() - 0.5),
                        pos2(r.right(), r.top() + 0.5),
                    ],
                    Stroke::new(2.0_f32, p.on_accent),
                );
            } else {
                painter.rect(box_rect, 4.0, p.well, visuals.bg_stroke, StrokeKind::Inside);
            }
            let text_pos = pos2(
                box_rect.right() + gap,
                rect.center().y - galley.size().y / 2.0,
            );
            painter.galley(text_pos, galley, visuals.text_color());
        }
        response
    }
}

/// Lays out `add_contents` as one segmented control: a well holding
/// `ui.selectable_value` segments.
pub fn segmented<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let p = theme::palette(ui);
    egui::Frame::new()
        .fill(p.well)
        .stroke(Stroke::new(1.0_f32, p.line_strong))
        .corner_radius(8)
        .inner_margin(2)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.spacing_mut().interact_size.y = 20.0;
            ui.spacing_mut().button_padding.y = 1.0;
            ui.horizontal(add_contents).inner
        })
        .inner
}

/// A button filled with `fill`, with its own hover and press shades.
pub fn filled_button(
    ui: &mut Ui,
    text: impl Into<WidgetText>,
    fill: Color32,
    text_color: Color32,
) -> Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for (state, fill) in [
            (&mut widgets.noninteractive, fill),
            (&mut widgets.inactive, fill),
            (&mut widgets.hovered, lighten(fill, 0.14)),
            (&mut widgets.active, lighten(fill, -0.1)),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::NONE;
            state.fg_stroke.color = text_color;
        }
        ui.button(text)
    })
    .inner
}

/// The accent-filled button for the action a step leads to. Disabled, it
/// looks like any disabled button, so a faded green does not read as ready.
pub fn primary_button(ui: &mut Ui, text: impl Into<WidgetText>) -> Response {
    if !ui.is_enabled() {
        return ui.button(text);
    }
    let p = theme::palette(ui);
    filled_button(ui, text, p.accent, p.on_accent)
}

/// A circled step number in front of a toolbar group, so the order
/// Add, Analyze, Apply reads at a glance.
pub fn step_badge(ui: &mut Ui, number: u8) {
    let p = theme::palette(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
    let painter = ui.painter();
    painter.circle_stroke(rect.center(), 9.0, Stroke::new(1.2_f32, p.accent_text));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        number.to_string(),
        egui::FontId::proportional(11.5),
        p.accent_text,
    );
}

/// A small tinted label, for flags such as clipping in the table.
pub fn badge(ui: &mut Ui, text: impl Into<String>, color: Color32) -> Response {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(12.0).strong())
        })
        .inner
}

/// Adds `add_contents` as one unit of a wrapping row: when it no longer
/// fits after the widgets before it, the row ends first, so a caption stays
/// with its control and a button's text is not wrapped letter by letter.
/// The width comes from the previous frame.
pub fn keep_together<R>(ui: &mut Ui, id_salt: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let id = ui.id().with(id_salt);
    let width: Option<f32> = ui.ctx().data(|d| d.get_temp(id));
    let at_row_start = ui.cursor().left() <= ui.max_rect().left() + 1.0;
    if !at_row_start && width.is_some_and(|w| w > ui.available_size_before_wrap().x) {
        ui.end_row();
    }
    let inner = ui.horizontal(add_contents);
    let width = inner.response.rect.width();
    ui.ctx().data_mut(|d| d.insert_temp(id, width));
    inner.inner
}

/// Muted label text, for the captions in front of a control.
pub fn caption(ui: &mut Ui, text: impl Into<String>) -> Response {
    let p = theme::palette(ui);
    ui.label(RichText::new(text).color(p.muted))
}

fn lighten(color: Color32, amount: f32) -> Color32 {
    let target = if amount >= 0.0 {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    color.lerp_to_gamma(target, amount.abs())
}
