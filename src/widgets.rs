use eframe::egui::*;

#[derive(Clone, Copy)]
pub enum Icon {
    Routes,
    Matrix,
    Settings,
    More,
    Volume,
    Muted,
    Play,
    Stop,
    Help,
    Info,
    Link,
    Unlink,
    Plus,
    Save,
}

pub fn draw_icon(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    let at = |x: f32, y: f32| c + vec2(x, y) * (s / 24.0);
    let stroke = Stroke::new(1.65, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    };
    match icon {
        Icon::Link | Icon::Unlink => {
            for (a, b) in [((-7.0, 2.0), (-1.0, -4.0)), ((1.0, 4.0), (7.0, -2.0))] {
                let center = (at(a.0, a.1) + at(b.0, b.1).to_vec2()) * 0.5;
                p.rect_stroke(
                    Rect::from_center_size(center, vec2(s * 0.43, s * 0.24)),
                    3.0,
                    stroke,
                    StrokeKind::Inside,
                );
            }
            if matches!(icon, Icon::Link) {
                line((-3.0, 0.0), (3.0, 0.0));
            } else {
                line((-7.0, -8.0), (7.0, 8.0));
            }
        }
        Icon::Plus => {
            line((-7.0, 0.0), (7.0, 0.0));
            line((0.0, -7.0), (0.0, 7.0));
        }
        Icon::Save => {
            p.rect_stroke(
                Rect::from_center_size(c, vec2(s * 0.67, s * 0.72)),
                2.0,
                stroke,
                StrokeKind::Inside,
            );
            line((-4.0, -8.0), (-4.0, -2.0));
            line((-4.0, -2.0), (4.0, -2.0));
            line((4.0, -2.0), (4.0, -8.0));
            p.rect_stroke(
                Rect::from_center_size(at(0.0, 5.0), vec2(s * 0.33, s * 0.25)),
                1.0,
                stroke,
                StrokeKind::Inside,
            );
        }
        Icon::Routes => {
            line((-8.0, -5.0), (8.0, -5.0));
            line((4.0, -9.0), (8.0, -5.0));
            line((4.0, -1.0), (8.0, -5.0));
            line((8.0, 5.0), (-8.0, 5.0));
            line((-4.0, 1.0), (-8.0, 5.0));
            line((-4.0, 9.0), (-8.0, 5.0));
        }
        Icon::Matrix => {
            for x in [-6.0, 0.0, 6.0] {
                for y in [-6.0, 0.0, 6.0] {
                    p.rect_filled(
                        Rect::from_center_size(at(x, y), vec2(s / 6.0, s / 6.0)),
                        1.0,
                        color,
                    );
                }
            }
        }
        Icon::Settings => {
            p.circle_stroke(c, s * 0.25, stroke);
            p.circle_stroke(c, s * 0.09, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * s * 0.27, c + d * s * 0.39], stroke);
            }
        }
        Icon::More => {
            for x in [-6.0, 0.0, 6.0] {
                p.circle_filled(at(x, 0.0), s * 0.07, color);
            }
        }
        Icon::Volume | Icon::Muted => {
            p.add(Shape::closed_line(
                vec![
                    at(-9.0, -3.0),
                    at(-5.0, -3.0),
                    at(0.0, -7.0),
                    at(0.0, 7.0),
                    at(-5.0, 3.0),
                    at(-9.0, 3.0),
                ],
                stroke,
            ));
            if matches!(icon, Icon::Muted) {
                line((4.0, -4.0), (10.0, 4.0));
                line((10.0, -4.0), (4.0, 4.0));
            } else {
                p.add(Shape::line(
                    vec![at(4.0, -5.0), at(7.0, -2.0), at(7.0, 2.0), at(4.0, 5.0)],
                    stroke,
                ));
            }
        }
        Icon::Play => {
            p.add(Shape::convex_polygon(
                vec![at(-5.0, -8.0), at(8.0, 0.0), at(-5.0, 8.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Stop => {
            p.rect_filled(
                Rect::from_center_size(c, vec2(s * 0.55, s * 0.55)),
                2.0,
                color,
            );
        }
        Icon::Info => {
            p.circle_stroke(c, s * 0.39, stroke);
            p.circle_filled(at(0.0, -5.0), s * 0.055, color);
            line((0.0, -1.0), (0.0, 6.0));
        }
        Icon::Help => {
            p.circle_stroke(c, s * 0.39, stroke);
            p.add(Shape::line(
                vec![
                    at(-3.0, -4.0),
                    at(-2.0, -6.0),
                    at(2.0, -6.0),
                    at(4.0, -4.0),
                    at(4.0, -2.0),
                    at(0.0, 1.0),
                    at(0.0, 3.0),
                ],
                stroke,
            ));
            p.circle_filled(at(0.0, 6.0), s * 0.04, color);
        }
    }
}

pub fn icon_at(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    icon: Icon,
    active: bool,
    color: Color32,
    tooltip: &str,
) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    let palette = ButtonPalette::new(active, color);
    let (fill, foreground) = if !ui.is_enabled() {
        (palette.idle, Color32::from_rgb(113, 123, 143))
    } else if response.is_pointer_button_down_on() {
        (palette.pressed, palette.foreground)
    } else if response.hovered() || response.has_focus() {
        (palette.hovered, palette.foreground)
    } else {
        (palette.idle, palette.foreground)
    };
    ui.painter().rect_filled(rect, 7.0, fill);
    if active || response.hovered() || response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            7.0,
            Stroke::new(1.0, palette.border),
            StrokeKind::Inside,
        );
    }
    draw_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(19.0, 19.0)),
        icon,
        foreground,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), tooltip));
    response.on_hover_text(tooltip)
}

pub fn action(ui: &mut Ui, icon: Icon, label: &str, active: bool) -> Response {
    action_tinted(ui, icon, label, active, Color32::from_rgb(180, 158, 255))
}

pub fn action_tinted(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    active: bool,
    accent: Color32,
) -> Response {
    let palette = ButtonPalette::new(active, accent);
    let foreground = if ui.is_enabled() {
        palette.foreground
    } else {
        Color32::from_rgb(113, 123, 143)
    };
    let galley = ui.painter().layout_no_wrap(
        label.into(),
        TextStyle::Button.resolve(ui.style()),
        foreground,
    );
    let padding = 13.0;
    let icon_size = 17.0;
    let gap = 8.0;
    let (rect, response) = ui.allocate_exact_size(
        vec2(padding * 2.0 + icon_size + gap + galley.size().x, 36.0),
        Sense::click(),
    );
    let fill = if response.is_pointer_button_down_on() {
        palette.pressed
    } else if response.hovered() || response.has_focus() {
        palette.hovered
    } else {
        palette.idle
    };
    ui.painter().rect_filled(rect, 7.0, fill);
    ui.painter().rect_stroke(
        rect,
        7.0,
        Stroke::new(1.0, palette.border),
        StrokeKind::Inside,
    );
    draw_icon(
        ui.painter(),
        Rect::from_center_size(
            pos2(rect.left() + padding + icon_size * 0.5, rect.center().y),
            vec2(icon_size, icon_size),
        ),
        icon,
        foreground,
    );
    ui.painter().galley(
        pos2(
            rect.left() + padding + icon_size + gap,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        foreground,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    response
}

pub fn search(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    ui.add(
        TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(width)
            .margin(Margin::symmetric(12, 9)),
    )
}

pub fn text_button(ui: &mut Ui, label: impl Into<WidgetText>, active: bool) -> Response {
    sized_text_button(
        ui,
        label,
        active,
        Color32::from_rgb(180, 158, 255),
        vec2(0.0, 36.0),
    )
}

/// Opaque, dark state fills keep bright text and semantic accents readable.
/// Selection, hover and press use the same accent family, including icon-only controls.
struct ButtonPalette {
    idle: Color32,
    hovered: Color32,
    pressed: Color32,
    foreground: Color32,
    border: Color32,
}
impl ButtonPalette {
    fn new(active: bool, accent: Color32) -> Self {
        let base = Color32::from_rgb(25, 30, 40);
        let mix = |amount: f32| {
            Color32::from_rgb(
                (base.r() as f32 + (accent.r() as f32 - base.r() as f32) * amount) as u8,
                (base.g() as f32 + (accent.g() as f32 - base.g() as f32) * amount) as u8,
                (base.b() as f32 + (accent.b() as f32 - base.b() as f32) * amount) as u8,
            )
        };
        Self {
            idle: if active {
                mix(0.28)
            } else {
                Color32::from_rgb(31, 37, 49)
            },
            hovered: if active {
                mix(0.36)
            } else {
                Color32::from_rgb(40, 47, 62)
            },
            pressed: if active {
                mix(0.43)
            } else {
                Color32::from_rgb(47, 55, 71)
            },
            foreground: if active {
                accent
            } else {
                Color32::from_rgb(234, 237, 245)
            },
            border: if active {
                mix(0.48)
            } else {
                Color32::from_rgb(83, 95, 119)
            },
        }
    }
}

pub fn sized_text_button(
    ui: &mut Ui,
    label: impl Into<WidgetText>,
    active: bool,
    accent: Color32,
    size: Vec2,
) -> Response {
    let palette = ButtonPalette::new(active, accent);
    ui.scope(|ui| {
        let visuals = ui.visuals_mut();
        visuals.override_text_color = None;
        for (widget, fill) in [
            (&mut visuals.widgets.inactive, palette.idle),
            (&mut visuals.widgets.hovered, palette.hovered),
            (&mut visuals.widgets.active, palette.pressed),
            (&mut visuals.widgets.open, palette.idle),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.fg_stroke = Stroke::new(1.0, palette.foreground);
            widget.bg_stroke = Stroke::new(1.0, palette.border);
        }
        visuals.widgets.noninteractive.fg_stroke =
            Stroke::new(1.0, Color32::from_rgb(113, 123, 143));
        ui.add(Button::new(label).corner_radius(7.0).min_size(size))
    })
    .inner
}

/// The live meter is the fader track. A white marker shows gain independently of signal level.
pub fn metered_fader(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    value: &mut f32,
    level: f32,
    color: Color32,
    muted: bool,
    label: &str,
) -> Response {
    let mut response = ui.interact(rect, id, Sense::click_and_drag());
    let track = rect.shrink2(vec2(5.0, 4.0));
    let previous = *value;
    if response.clicked() || response.dragged() {
        response.request_focus();
        if let Some(pointer) = response.interact_pointer_pos() {
            *value = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0);
        }
    }
    if response.has_focus() {
        let delta = ui.input(|i| {
            (i.num_presses(Key::ArrowRight) as f32 - i.num_presses(Key::ArrowLeft) as f32)
                * if i.modifiers.shift { 0.1 } else { 0.01 }
        });
        *value = (*value + delta).clamp(0.0, 1.0);
    }
    if response.double_clicked() {
        *value = 1.0;
    }
    if *value != previous {
        response.mark_changed();
    }
    let p = ui.painter();
    p.rect_filled(track, 3.0, Color32::from_rgb(14, 19, 27));
    let peak = ((20.0 * level.max(0.0001).log10() + 60.0) / 60.0).clamp(0.0, 1.0);
    p.rect_filled(
        Rect::from_min_size(track.min, vec2(track.width() * peak, track.height())),
        3.0,
        if muted {
            Color32::from_rgb(103, 111, 127)
        } else {
            color.gamma_multiply(0.85)
        },
    );
    // Unity marker stays inside the track at 0 dB, so it cannot be clipped by the card.
    let x = (track.left() + *value * track.width()).clamp(track.left() + 2.0, track.right() - 2.0);
    p.rect_filled(
        Rect::from_center_size(pos2(x, track.center().y), vec2(3.0, track.height() + 5.0)),
        1.5,
        if response.hovered() || response.dragged() {
            Color32::WHITE
        } else {
            Color32::from_rgb(220, 226, 239)
        },
    );
    if response.has_focus() {
        p.rect_stroke(rect, 5.0, Stroke::new(1.0, color), StrokeKind::Inside);
    }
    response.widget_info(|| WidgetInfo::slider(ui.is_enabled(), *value as f64, label));
    response.on_hover_text(format!(
        "{label} · {}\n拖动电平条调整音量，双击恢复 0 dB；方向键微调。",
        bridge::fader_label(*value as f64)
    ))
}
