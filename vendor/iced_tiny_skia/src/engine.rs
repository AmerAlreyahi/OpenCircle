use crate::Primitive;
use crate::core::renderer::Quad;
use crate::core::{
    Background, Color, Gradient, Rectangle, Size, Transformation, Vector,
};
use crate::graphics::{Image, Text};
use crate::text;

#[derive(Debug)]
pub struct Engine {
    text_pipeline: text::Pipeline,

    #[cfg(feature = "image")]
    pub(crate) raster_pipeline: crate::raster::Pipeline,
    #[cfg(feature = "svg")]
    pub(crate) vector_pipeline: crate::vector::Pipeline,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            text_pipeline: text::Pipeline::new(),
            #[cfg(feature = "image")]
            raster_pipeline: crate::raster::Pipeline::new(),
            #[cfg(feature = "svg")]
            vector_pipeline: crate::vector::Pipeline::new(),
        }
    }

    pub fn draw_quad(
        &mut self,
        quad: &Quad,
        background: &Background,
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut tiny_skia::Mask,
        clip_bounds: Rectangle,
    ) {
        let physical_bounds = quad.bounds * transformation;

        if !clip_bounds.intersects(&physical_bounds) {
            return;
        }

        let clip_mask = (!physical_bounds.is_within(&clip_bounds))
            .then_some(clip_mask as &_);

        let transform = into_transform(transformation);

        // Make sure the border radius is not larger than the bounds
        let border_width = quad
            .border
            .width
            .min(quad.bounds.width / 2.0)
            .min(quad.bounds.height / 2.0);

        let mut fill_border_radius = <[f32; 4]>::from(quad.border.radius);

        for radius in &mut fill_border_radius {
            *radius = (*radius)
                .min(quad.bounds.width / 2.0)
                .min(quad.bounds.height / 2.0);
        }

        let path = rounded_rectangle(quad.bounds, fill_border_radius);

        let shadow = quad.shadow;

        if shadow.color.a > 0.0 {
            let shadow_bounds = Rectangle {
                x: quad.bounds.x + shadow.offset.x - shadow.blur_radius,
                y: quad.bounds.y + shadow.offset.y - shadow.blur_radius,
                width: quad.bounds.width + shadow.blur_radius * 2.0,
                height: quad.bounds.height + shadow.blur_radius * 2.0,
            } * transformation;

            let radii = fill_border_radius
                .into_iter()
                .map(|radius| radius * transformation.scale_factor())
                .collect::<Vec<_>>();
            let (x, y, width, height) = (
                shadow_bounds.x as u32,
                shadow_bounds.y as u32,
                shadow_bounds.width as u32,
                shadow_bounds.height as u32,
            );
            let half_width = physical_bounds.width / 2.0;
            let half_height = physical_bounds.height / 2.0;

            let colors = (y..y + height)
                .flat_map(|y| (x..x + width).map(move |x| (x as f32, y as f32)))
                .filter_map(|(x, y)| {
                    tiny_skia::Size::from_wh(half_width, half_height).map(
                        |size| {
                            let shadow_distance = rounded_box_sdf(
                                Vector::new(
                                    x - physical_bounds.position().x
                                        - (shadow.offset.x
                                            * transformation.scale_factor())
                                        - half_width,
                                    y - physical_bounds.position().y
                                        - (shadow.offset.y
                                            * transformation.scale_factor())
                                        - half_height,
                                ),
                                size,
                                &radii,
                            )
                            .max(0.0);
                            let shadow_alpha = 1.0
                                - smoothstep(
                                    -shadow.blur_radius
                                        * transformation.scale_factor(),
                                    shadow.blur_radius
                                        * transformation.scale_factor(),
                                    shadow_distance,
                                );

                            let mut color = into_color(shadow.color);
                            color.apply_opacity(shadow_alpha);

                            color.to_color_u8().premultiply()
                        },
                    )
                })
                .collect();

            if let Some(pixmap) = tiny_skia::IntSize::from_wh(width, height)
                .and_then(|size| {
                    tiny_skia::Pixmap::from_vec(
                        bytemuck::cast_vec(colors),
                        size,
                    )
                })
            {
                pixels.draw_pixmap(
                    x as i32,
                    y as i32,
                    pixmap.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    tiny_skia::Transform::default(),
                    None,
                );
            }
        }

        // OpenCircle patch: a solid quad reaching past its clip with none of
        // its round corners in it (a window's or a page's background under a
        // small redraw) is, inside the clip, exactly the rectangle they share:
        // fill only that. tiny-skia rasterizes a path over its whole area and
        // masks each pixel after, so a 4K window's background cost ~6 ms for
        // a sidebar highlight's few pixels.
        let inside = match (background, clip_mask) {
            (Background::Color(color), Some(_)) => {
                let (w, h) = (pixels.width(), pixels.height());
                let [x0, y0, x1, y1] = pixel_rect(clip_bounds, w, h);
                let clip = Rectangle::new(
                    crate::core::Point::new(x0 as f32, y0 as f32),
                    Size::new((x1 - x0) as f32, (y1 - y0) as f32),
                );
                let s = transformation.scale_factor();
                let b = physical_bounds;
                let [tl, tr, br, bl] = fill_border_radius.map(|r| r * s);
                let corners = [
                    (b.x, b.y, tl),
                    (b.x + b.width - tr, b.y, tr),
                    (b.x + b.width - br, b.y + b.height - br, br),
                    (b.x, b.y + b.height - bl, bl),
                ];
                let clear = corners.iter().all(|&(x, y, r)| {
                    r <= 0.0
                        || !Rectangle::new(crate::core::Point::new(x, y), Size::new(r, r))
                            .intersects(&clip)
                });
                clear
                    .then(|| b.intersection(&clip))
                    .flatten()
                    .and_then(|r| {
                        tiny_skia::Rect::from_xywh(r.x, r.y, r.width, r.height)
                    })
                    .map(|r| (r, *color))
            }
            _ => None,
        };
        if let Some((rect, color)) = inside {
            pixels.fill_rect(
                rect,
                &tiny_skia::Paint {
                    shader: tiny_skia::Shader::SolidColor(into_color(color)),
                    anti_alias: true,
                    ..tiny_skia::Paint::default()
                },
                tiny_skia::Transform::identity(),
                None,
            );
        } else {
        pixels.fill_path(
            &path,
            &tiny_skia::Paint {
                shader: match background {
                    Background::Color(color) => {
                        tiny_skia::Shader::SolidColor(into_color(*color))
                    }
                    Background::Gradient(Gradient::Linear(linear)) => {
                        let (start, end) =
                            linear.angle.to_distance(&quad.bounds);

                        let stops: Vec<tiny_skia::GradientStop> = linear
                            .stops
                            .into_iter()
                            .flatten()
                            .map(|stop| {
                                tiny_skia::GradientStop::new(
                                    stop.offset,
                                    tiny_skia::Color::from_rgba(
                                        stop.color.b,
                                        stop.color.g,
                                        stop.color.r,
                                        stop.color.a,
                                    )
                                    .expect("Create color"),
                                )
                            })
                            .collect();

                        tiny_skia::LinearGradient::new(
                            tiny_skia::Point {
                                x: start.x,
                                y: start.y,
                            },
                            tiny_skia::Point { x: end.x, y: end.y },
                            if stops.is_empty() {
                                vec![tiny_skia::GradientStop::new(
                                    0.0,
                                    tiny_skia::Color::BLACK,
                                )]
                            } else {
                                stops
                            },
                            tiny_skia::SpreadMode::Pad,
                            tiny_skia::Transform::identity(),
                        )
                        .expect("Create linear gradient")
                    }
                },
                anti_alias: true,
                ..tiny_skia::Paint::default()
            },
            tiny_skia::FillRule::EvenOdd,
            transform,
            clip_mask,
        );
        }

        if border_width > 0.0 {
            // Border path is offset by half the border width
            let border_bounds = Rectangle {
                x: quad.bounds.x + border_width / 2.0,
                y: quad.bounds.y + border_width / 2.0,
                width: quad.bounds.width - border_width,
                height: quad.bounds.height - border_width,
            };

            // Make sure the border radius is correct
            let mut border_radius = <[f32; 4]>::from(quad.border.radius);
            let mut is_simple_border = true;

            for radius in &mut border_radius {
                *radius = if *radius == 0.0 {
                    // Path should handle this fine
                    0.0
                } else if *radius > border_width / 2.0 {
                    *radius - border_width / 2.0
                } else {
                    is_simple_border = false;
                    0.0
                }
                .min(border_bounds.width / 2.0)
                .min(border_bounds.height / 2.0);
            }

            // Stroking a path works well in this case
            if is_simple_border {
                let border_path =
                    rounded_rectangle(border_bounds, border_radius);

                pixels.stroke_path(
                    &border_path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(into_color(
                            quad.border.color,
                        )),
                        anti_alias: true,
                        ..tiny_skia::Paint::default()
                    },
                    &tiny_skia::Stroke {
                        width: border_width,
                        ..tiny_skia::Stroke::default()
                    },
                    transform,
                    clip_mask,
                );
            } else {
                // Draw corners that have too small border radii as having no border radius,
                // but mask them with the rounded rectangle with the correct border radius.
                let mut temp_pixmap = tiny_skia::Pixmap::new(
                    quad.bounds.width as u32,
                    quad.bounds.height as u32,
                )
                .unwrap();

                let mut quad_mask = tiny_skia::Mask::new(
                    quad.bounds.width as u32,
                    quad.bounds.height as u32,
                )
                .unwrap();

                let zero_bounds = Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: quad.bounds.width,
                    height: quad.bounds.height,
                };
                let path = rounded_rectangle(zero_bounds, fill_border_radius);

                quad_mask.fill_path(
                    &path,
                    tiny_skia::FillRule::EvenOdd,
                    true,
                    transform,
                );
                let path_bounds = Rectangle {
                    x: border_width / 2.0,
                    y: border_width / 2.0,
                    width: quad.bounds.width - border_width,
                    height: quad.bounds.height - border_width,
                };

                let border_radius_path =
                    rounded_rectangle(path_bounds, border_radius);

                temp_pixmap.stroke_path(
                    &border_radius_path,
                    &tiny_skia::Paint {
                        shader: tiny_skia::Shader::SolidColor(into_color(
                            quad.border.color,
                        )),
                        anti_alias: true,
                        ..tiny_skia::Paint::default()
                    },
                    &tiny_skia::Stroke {
                        width: border_width,
                        ..tiny_skia::Stroke::default()
                    },
                    transform,
                    Some(&quad_mask),
                );

                pixels.draw_pixmap(
                    quad.bounds.x as i32,
                    quad.bounds.y as i32,
                    temp_pixmap.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    transform,
                    clip_mask,
                );
            }
        }
    }

    pub fn draw_text(
        &mut self,
        text: &Text,
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut tiny_skia::Mask,
        clip_bounds: Rectangle,
    ) {
        match text {
            Text::Paragraph {
                paragraph,
                position,
                color,
                clip_bounds: local_clip_bounds,
                transformation: local_transformation,
            } => {
                let transformation = transformation * *local_transformation;
                let Some(clip_bounds) = clip_bounds
                    .intersection(&(*local_clip_bounds * transformation))
                else {
                    return;
                };

                let physical_bounds =
                    Rectangle::new(*position, paragraph.min_bounds)
                        * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = match physical_bounds.is_within(&clip_bounds) {
                    true => None,
                    false => {
                        adjust_clip_mask(clip_mask, clip_bounds);
                        Some(clip_mask as &_)
                    }
                };

                self.text_pipeline.draw_paragraph(
                    paragraph,
                    *position,
                    *color,
                    pixels,
                    clip_mask,
                    transformation,
                );
            }
            Text::Editor {
                editor,
                position,
                color,
                clip_bounds: local_clip_bounds,
                transformation: local_transformation,
            } => {
                let transformation = transformation * *local_transformation;
                let Some(clip_bounds) = clip_bounds
                    .intersection(&(*local_clip_bounds * transformation))
                else {
                    return;
                };

                let physical_bounds =
                    Rectangle::new(*position, editor.bounds) * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = match physical_bounds.is_within(&clip_bounds) {
                    true => None,
                    false => {
                        adjust_clip_mask(clip_mask, clip_bounds);
                        Some(clip_mask as &_)
                    }
                };

                self.text_pipeline.draw_editor(
                    editor,
                    *position,
                    *color,
                    pixels,
                    clip_mask,
                    transformation,
                );
            }
            Text::Cached {
                content,
                bounds,
                color,
                size,
                line_height,
                font,
                align_x,
                align_y,
                shaping,
                clip_bounds: local_clip_bounds,
            } => {
                let physical_bounds = *local_clip_bounds * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = match physical_bounds.is_within(&clip_bounds) {
                    true => None,
                    false => {
                        adjust_clip_mask(clip_mask, clip_bounds);
                        Some(clip_mask as &_)
                    }
                };

                self.text_pipeline.draw_cached(
                    content,
                    *bounds,
                    *color,
                    *size,
                    *line_height,
                    *font,
                    *align_x,
                    *align_y,
                    *shaping,
                    pixels,
                    clip_mask,
                    transformation,
                );
            }
            Text::Raw {
                raw,
                transformation: local_transformation,
            } => {
                let Some(buffer) = raw.buffer.upgrade() else {
                    return;
                };

                let transformation = transformation * *local_transformation;
                let (width, height) = buffer.size();

                let physical_bounds = Rectangle::new(
                    raw.position,
                    Size::new(
                        width.unwrap_or(clip_bounds.width),
                        height.unwrap_or(clip_bounds.height),
                    ),
                ) * transformation;

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = (!physical_bounds.is_within(&clip_bounds))
                    .then_some(clip_mask as &_);

                self.text_pipeline.draw_raw(
                    &buffer,
                    raw.position,
                    raw.color,
                    pixels,
                    clip_mask,
                    transformation,
                );
            }
        }
    }

    pub fn draw_primitive(
        &mut self,
        primitive: &Primitive,
        transformation: Transformation,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut tiny_skia::Mask,
        clip_bounds: Rectangle,
    ) {
        match primitive {
            Primitive::Fill { path, paint, rule } => {
                let physical_bounds = {
                    let bounds = path.bounds();

                    Rectangle {
                        x: bounds.x(),
                        y: bounds.y(),
                        width: bounds.width(),
                        height: bounds.height(),
                    } * transformation
                };

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = (!physical_bounds.is_within(&clip_bounds))
                    .then_some(clip_mask as &_);

                if clip_mask.is_none() && fill_rect(path, paint, physical_bounds, pixels) {
                    return;
                }
                let t = into_transform(transformation);
                within(pixels, clip_mask, clip_bounds, physical_bounds, t, |p, t, m| {
                    p.fill_path(path, paint, *rule, t, m);
                });
            }
            Primitive::Stroke {
                path,
                paint,
                stroke,
            } => {
                let physical_bounds = {
                    let bounds = path.bounds();

                    Rectangle {
                        x: bounds.x() - stroke.width / 2.0,
                        y: bounds.y() - stroke.width / 2.0,
                        width: bounds.width() + stroke.width,
                        height: bounds.height() + stroke.width,
                    } * transformation
                };

                if !clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = (!physical_bounds.is_within(&clip_bounds))
                    .then_some(clip_mask as &_);

                let t = into_transform(transformation);
                within(pixels, clip_mask, clip_bounds, physical_bounds, t, |p, t, m| {
                    p.stroke_path(path, paint, stroke, t, m);
                });
            }
        }
    }

    pub fn draw_image(
        &mut self,
        image: &Image,
        _transformation: Transformation,
        _pixels: &mut tiny_skia::PixmapMut<'_>,
        _clip_mask: &mut tiny_skia::Mask,
        _clip_bounds: Rectangle,
        // OpenCircle patch (#89): the one opaque colour under it (a
        // premultiplied pixel), when the caller knows (`Renderer::base`) and
        // skipped the clear: a faded picture is drawn over it in one pass
        _under: Option<u32>,
    ) {
        match image {
            #[cfg(feature = "image")]
            Image::Raster { image, bounds, .. } => {
                let physical_bounds = *bounds * _transformation;

                if !_clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = (!physical_bounds.is_within(&_clip_bounds))
                    .then_some(_clip_mask as &_);

                let center = physical_bounds.center();
                let radians = f32::from(image.rotation);

                let transform = into_transform(_transformation).post_rotate_at(
                    radians.to_degrees(),
                    center.x,
                    center.y,
                );

                // OpenCircle patch (#23): pixel for pixel, no pipeline; the
                // mask's pixels where `draw` would use it
                let (w, h) = (_pixels.width(), _pixels.height());
                let clip = match clip_mask {
                    Some(_) => pixel_rect(_clip_bounds, w, h),
                    None => [0, 0, w, h],
                };
                let under = _under;
                if radians == 0.0
                    && (image.opacity == 1.0 || under.is_some())
                    && image.filter_method == crate::core::image::FilterMethod::Nearest
                    && self.raster_pipeline.blit(
                        &image.handle,
                        *bounds,
                        transform,
                        _pixels,
                        clip,
                        image.opacity,
                        under,
                    )
                {
                    return;
                }

                // (the caller skipped the clear under it: lay its colour first)
                if let (Some(c), Ok(data)) = (
                    under,
                    bytemuck::try_cast_slice_mut::<u8, u32>(_pixels.data_mut()),
                ) {
                    let [x0, y0, x1, y1] = pixel_rect(_clip_bounds, w, h);
                    for row in data.chunks_exact_mut(w as usize).take(y1 as usize).skip(y0 as usize) {
                        row[x0 as usize..x1 as usize].fill(c);
                    }
                }
                let drawn = physical_bounds.intersection(&_clip_bounds);
                let px = drawn.map_or(0.0, |r| r.width * r.height);
                PIPELINE_PIXELS.set(PIPELINE_PIXELS.get() + px as u64);
                self.raster_pipeline.draw(
                    &image.handle,
                    image.filter_method,
                    *bounds,
                    image.opacity,
                    _pixels,
                    transform,
                    clip_mask,
                );
            }
            #[cfg(feature = "svg")]
            Image::Vector { svg, bounds, .. } => {
                let physical_bounds = *bounds * _transformation;

                if !_clip_bounds.intersects(&physical_bounds) {
                    return;
                }

                let clip_mask = (!physical_bounds.is_within(&_clip_bounds))
                    .then_some(_clip_mask as &_);

                let center = physical_bounds.center();
                let radians = f32::from(svg.rotation);

                let transform = into_transform(_transformation).post_rotate_at(
                    radians.to_degrees(),
                    center.x,
                    center.y,
                );

                self.vector_pipeline.draw(
                    &svg.handle,
                    svg.color,
                    *bounds,
                    svg.opacity,
                    _pixels,
                    transform,
                    clip_mask,
                );
            }
            #[cfg(not(feature = "image"))]
            Image::Raster { .. } => {
                log::warn!(
                    "Unsupported primitive in `iced_tiny_skia`: {image:?}",
                );
            }
            #[cfg(not(feature = "svg"))]
            Image::Vector { .. } => {
                log::warn!(
                    "Unsupported primitive in `iced_tiny_skia`: {image:?}",
                );
            }
        }
    }

    pub fn trim(&mut self) {
        self.text_pipeline.trim_cache();

        #[cfg(feature = "image")]
        self.raster_pipeline.trim_cache();

        #[cfg(feature = "svg")]
        self.vector_pipeline.trim_cache();
    }
}

pub fn into_color(color: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(color.b, color.g, color.r, color.a)
        .expect("Convert color from iced to tiny_skia")
}

fn into_transform(transformation: Transformation) -> tiny_skia::Transform {
    let translation = transformation.translation();

    tiny_skia::Transform {
        sx: transformation.scale_factor(),
        kx: 0.0,
        ky: 0.0,
        sy: transformation.scale_factor(),
        tx: translation.x,
        ty: translation.y,
    }
}

fn rounded_rectangle(
    bounds: Rectangle,
    border_radius: [f32; 4],
) -> tiny_skia::Path {
    let [top_left, top_right, bottom_right, bottom_left] = border_radius;

    if top_left == 0.0
        && top_right == 0.0
        && bottom_right == 0.0
        && bottom_left == 0.0
    {
        return tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
            )
            .expect("Build quad rectangle"),
        );
    }

    if top_left == top_right
        && top_left == bottom_right
        && top_left == bottom_left
        && top_left == bounds.width / 2.0
        && top_left == bounds.height / 2.0
    {
        return tiny_skia::PathBuilder::from_circle(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
            top_left,
        )
        .expect("Build circle path");
    }

    let mut builder = tiny_skia::PathBuilder::new();

    builder.move_to(bounds.x + top_left, bounds.y);
    builder.line_to(bounds.x + bounds.width - top_right, bounds.y);

    if top_right > 0.0 {
        arc_to(
            &mut builder,
            bounds.x + bounds.width - top_right,
            bounds.y,
            bounds.x + bounds.width,
            bounds.y + top_right,
            top_right,
        );
    }

    maybe_line_to(
        &mut builder,
        bounds.x + bounds.width,
        bounds.y + bounds.height - bottom_right,
    );

    if bottom_right > 0.0 {
        arc_to(
            &mut builder,
            bounds.x + bounds.width,
            bounds.y + bounds.height - bottom_right,
            bounds.x + bounds.width - bottom_right,
            bounds.y + bounds.height,
            bottom_right,
        );
    }

    maybe_line_to(
        &mut builder,
        bounds.x + bottom_left,
        bounds.y + bounds.height,
    );

    if bottom_left > 0.0 {
        arc_to(
            &mut builder,
            bounds.x + bottom_left,
            bounds.y + bounds.height,
            bounds.x,
            bounds.y + bounds.height - bottom_left,
            bottom_left,
        );
    }

    maybe_line_to(&mut builder, bounds.x, bounds.y + top_left);

    if top_left > 0.0 {
        arc_to(
            &mut builder,
            bounds.x,
            bounds.y + top_left,
            bounds.x + top_left,
            bounds.y,
            top_left,
        );
    }

    builder.finish().expect("Build rounded rectangle path")
}

fn maybe_line_to(path: &mut tiny_skia::PathBuilder, x: f32, y: f32) {
    if path.last_point() != Some(tiny_skia::Point { x, y }) {
        path.line_to(x, y);
    }
}

fn arc_to(
    path: &mut tiny_skia::PathBuilder,
    x_from: f32,
    y_from: f32,
    x_to: f32,
    y_to: f32,
    radius: f32,
) {
    let svg_arc = kurbo::SvgArc {
        from: kurbo::Point::new(f64::from(x_from), f64::from(y_from)),
        to: kurbo::Point::new(f64::from(x_to), f64::from(y_to)),
        radii: kurbo::Vec2::new(f64::from(radius), f64::from(radius)),
        x_rotation: 0.0,
        large_arc: false,
        sweep: true,
    };

    match kurbo::Arc::from_svg_arc(&svg_arc) {
        Some(arc) => {
            arc.to_cubic_beziers(0.1, |p1, p2, p| {
                path.cubic_to(
                    p1.x as f32,
                    p1.y as f32,
                    p2.x as f32,
                    p2.y as f32,
                    p.x as f32,
                    p.y as f32,
                );
            });
        }
        None => {
            path.line_to(x_to, y_to);
        }
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let x = ((x - a) / (b - a)).clamp(0.0, 1.0);

    x * x * (3.0 - 2.0 * x)
}

fn rounded_box_sdf(
    to_center: Vector,
    size: tiny_skia::Size,
    radii: &[f32],
) -> f32 {
    let radius = match (to_center.x > 0.0, to_center.y > 0.0) {
        (true, true) => radii[2],
        (true, false) => radii[1],
        (false, true) => radii[3],
        (false, false) => radii[0],
    };

    let x = (to_center.x.abs() - size.width() + radius).max(0.0);
    let y = (to_center.y.abs() - size.height() + radius).max(0.0);

    (x.powf(2.0) + y.powf(2.0)).sqrt() - radius
}

thread_local! {
    /// OpenCircle patch (#134): the pixels of raster images this thread drew
    /// through tiny-skia's pipeline (not `Pipeline::blit`), for the app's
    /// tests: an animation over the frozen screen must blit its tiles and edge
    /// strips (a small icon moving by fractions of a pixel may not).
    pub static PIPELINE_PIXELS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// OpenCircle patch (#89): a canvas's solid rectangle (`fill_rectangle`: not
/// anti-aliased, source-over, inside its clip), blended straight into the
/// pixels: the pixels whose middle it covers (tiny-skia's non-AA rule),
/// premultiplied, two channels at a time. The overlay's dim is a whole
/// screen of them, redrawn on every frame of the wheel zoom (tiny-skia's
/// pipeline: ~13 ms a 4K frame). `false` (nothing drawn): not such a fill.
fn fill_rect(
    path: &tiny_skia::Path,
    paint: &tiny_skia::Paint<'_>,
    physical: Rectangle,
    pixels: &mut tiny_skia::PixmapMut<'_>,
) -> bool {
    let Some(src) = solid_rect(path, paint) else {
        return false;
    };
    let (w, h) = (pixels.width(), pixels.height());
    let [x0, y0, x1, y1] = pixel_rect(physical, w, h);
    let keep = 255 - u32::from(src.to_ne_bytes()[3]);
    let Ok(data) = bytemuck::try_cast_slice_mut::<u8, u32>(pixels.data_mut()) else {
        return false;
    };
    if x0 >= x1 || y0 >= y1 {
        return true;
    }
    // each byte: src + d * keep / 255 (rounded), 8 pixels at a time (the
    // compiler turns the fixed arrays into vector code)
    let c: [u8; 32] = std::array::from_fn(|i| src.to_ne_bytes()[i % 4]);
    let keep = keep as u16;
    let mix = |d: &mut [u8], c: &[u8]| {
        for (d, &c) in d.iter_mut().zip(c) {
            let t = u16::from(*d) * keep + 128;
            *d = c + ((t + (t >> 8)) >> 8) as u8;
        }
    };
    let blend = |row: &mut [u32]| match keep {
        0 => row.fill(src),
        255 => {}
        _ => {
            let bytes: &mut [u8] = bytemuck::cast_slice_mut(row);
            let mut chunks = bytes.chunks_exact_mut(32);
            for chunk in &mut chunks {
                let chunk: &mut [u8; 32] = chunk.try_into().expect("32 bytes");
                mix(chunk, &c);
            }
            mix(chunks.into_remainder(), &c);
        }
    };
    crate::raster::rows(data, w as usize, (x1 - x0) as usize, y0 as usize..y1 as usize, |_, row| {
        blend(&mut row[x0 as usize..x1 as usize]);
    });
    true
}

/// OpenCircle patch (#89): `path` filled with `paint` is a solid rectangle
/// (`fill_rect`): its colour, premultiplied, as a pixel.
pub(crate) fn solid_rect(path: &tiny_skia::Path, paint: &tiny_skia::Paint<'_>) -> Option<u32> {
    use tiny_skia::PathSegment as S;
    let tiny_skia::Shader::SolidColor(color) = paint.shader else {
        return None;
    };
    let b = path.bounds();
    let corner = |p: &tiny_skia::Point| {
        (p.x == b.left() || p.x == b.right()) && (p.y == b.top() || p.y == b.bottom())
    };
    if paint.anti_alias
        || paint.blend_mode != tiny_skia::BlendMode::SourceOver
        || !matches!(
            path.segments().collect::<Vec<_>>()[..],
            [S::MoveTo(a), S::LineTo(b), S::LineTo(c), S::LineTo(d), S::Close]
                if [a, b, c, d].iter().all(corner)
        )
    {
        return None;
    }
    let c = color.premultiply().to_color_u8();
    Some(u32::from_ne_bytes([c.red(), c.green(), c.blue(), c.alpha()]))
}

/// OpenCircle patch: `draw` (a fill or stroke `drawn` over this much, at
/// `transform`) clipped to `clip` by `mask`. tiny-skia rasterizes a path over
/// all of it and the mask only drops what is outside, so every damage
/// rectangle a big path met drew all of it (a selection's glow, its outline
/// crossed by a dozen strips and tiles: ~10 ms a frame of a snip area's
/// resize). Much bigger than `clip` (4x), it is drawn into a pixmap of just
/// the clip's pixels (the mask's, `pixel_rect`), copied in and back out: the
/// same pixels, rasterized over the clip only.
fn within(
    pixels: &mut tiny_skia::PixmapMut<'_>,
    mask: Option<&tiny_skia::Mask>,
    clip: Rectangle,
    drawn: Rectangle,
    transform: tiny_skia::Transform,
    draw: impl FnOnce(&mut tiny_skia::PixmapMut<'_>, tiny_skia::Transform, Option<&tiny_skia::Mask>),
) {
    let (w, h) = (pixels.width(), pixels.height());
    let [x0, y0, x1, y1] = pixel_rect(clip, w, h);
    let (cw, ch) = (x1 - x0, y1 - y0);
    let small = mask.is_some() && 4.0 * cw as f32 * ch as f32 <= drawn.width * drawn.height;
    let Some(mut part) = small.then(|| tiny_skia::Pixmap::new(cw, ch)).flatten() else {
        return draw(pixels, transform, mask);
    };
    let (stride, row) = (w as usize * 4, cw as usize * 4);
    let at = |y: usize| (y0 as usize + y) * stride + x0 as usize * 4;
    let (src, dst) = (pixels.data_mut(), part.data_mut());
    for (y, line) in dst.chunks_exact_mut(row).enumerate() {
        line.copy_from_slice(&src[at(y)..at(y) + row]);
    }
    let moved = transform.post_translate(-(x0 as f32), -(y0 as f32));
    draw(&mut part.as_mut(), moved, None);
    let src = pixels.data_mut();
    for (y, line) in part.data().chunks_exact(row).enumerate() {
        src[at(y)..at(y) + row].copy_from_slice(line);
    }
}

/// OpenCircle patch (#23): the whole pixels a clip to `r` (device px) holds,
/// (x0, y0, x1, y1) inside a `w` x `h` target: those whose middle is in `r`.
/// One rule for the mask, the damage's clearing and `raster::Pipeline::blit`.
pub fn pixel_rect(r: Rectangle, w: u32, h: u32) -> [u32; 4] {
    let px = |v: f32, max: u32| ((v + 0.5).floor().max(0.0) as u32).min(max);
    let (x0, y0) = (px(r.x, w), px(r.y, h));
    [x0, y0, px(r.x + r.width, w).max(x0), px(r.y + r.height, h).max(y0)]
}

/// OpenCircle patch (#23): sets the mask to `bounds` (`pixel_rect`),
/// touching only the pixels that change. Upstream cleared and refilled the
/// whole window-sized mask for every layer, group and damage rectangle
/// (dozens of full-screen passes per frame: most of a small redraw's time).
/// The rectangle last set is remembered with the mask's address and size,
/// and trusted only while its first pixel is still set (a new mask at the
/// same address is all clear): the same rectangle again costs nothing,
/// another one clears only the old one's pixels.
pub fn adjust_clip_mask(clip_mask: &mut tiny_skia::Mask, bounds: Rectangle) {
    use std::cell::Cell;
    // (mask pixels, width, height, the pixels set)
    type Set = (usize, u32, u32, [u32; 4]);
    thread_local!(static SET: Cell<Option<Set>> = const { Cell::new(None) });

    let (w, h) = (clip_mask.width(), clip_mask.height());
    let id = clip_mask.data().as_ptr() as usize;
    let rect = pixel_rect(bounds, w, h);
    let fill = |data: &mut [u8], [x0, y0, x1, y1]: [u32; 4], v: u8| {
        for y in y0..y1 {
            let row = (y * w) as usize;
            data[row + x0 as usize..row + x1 as usize].fill(v);
        }
    };
    let ours = SET.get().filter(|&(at, lw, lh, [x0, y0, x1, y1])| {
        (at, lw, lh) == (id, w, h)
            && x0 < x1
            && y0 < y1
            && clip_mask.data()[(y0 * w + x0) as usize] == 255
    });
    match ours {
        Some((.., was)) if was == rect => return,
        Some((.., was)) => fill(clip_mask.data_mut(), was, 0),
        None => clip_mask.clear(),
    }
    fill(clip_mask.data_mut(), rect, 255);
    SET.set(Some((id, w, h, rect)));
}
