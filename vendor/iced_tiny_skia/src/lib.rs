#![allow(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
pub mod window;

mod engine;
mod layer;
mod primitive;
mod settings;
mod text;

#[cfg(feature = "image")]
mod raster;

#[cfg(feature = "svg")]
mod vector;

#[cfg(feature = "geometry")]
pub mod geometry;

use iced_debug as debug;
pub use iced_graphics as graphics;
pub use iced_graphics::core;

pub use engine::PIPELINE_PIXELS;
pub use raster::SPLIT;
pub use layer::Layer;
pub use primitive::Primitive;
pub use settings::Settings;

#[cfg(feature = "geometry")]
pub use geometry::Geometry;

use crate::core::renderer;
use crate::core::{
    Background, Color, Font, Pixels, Point, Rectangle, Size, Transformation,
};
use crate::engine::Engine;
use crate::graphics::Viewport;
use crate::graphics::compositor;
use crate::graphics::text::{Editor, Paragraph};

/// A [`tiny-skia`] graphics renderer for [`iced`].
///
/// [`tiny-skia`]: https://github.com/RazrFalcon/tiny-skia
/// [`iced`]: https://github.com/iced-rs/iced
#[derive(Debug)]
pub struct Renderer {
    default_font: Font,
    default_text_size: Pixels,
    layers: layer::Stack,
    engine: Engine, // TODO: Shared engine
}

impl Renderer {
    pub fn new(default_font: Font, default_text_size: Pixels) -> Self {
        Self {
            default_font,
            default_text_size,
            layers: layer::Stack::new(),
            engine: Engine::new(),
        }
    }

    pub fn layers(&mut self) -> &[Layer] {
        self.layers.flush();
        self.layers.as_slice()
    }

    pub fn draw(
        &mut self,
        pixels: &mut tiny_skia::PixmapMut<'_>,
        clip_mask: &mut tiny_skia::Mask,
        viewport: &Viewport,
        damage: &[Rectangle],
        background_color: Color,
    ) {
        let scale_factor = viewport.scale_factor();

        self.layers.flush();

        // OpenCircle patch: many rectangles that cover much of their bounding
        // box (a scrolled page: dozens of overlapping ones) are drawn as that
        // box, once. Each rectangle draws every layer's primitives it meets,
        // so overlapping ones drew the same quads and text over and over (a
        // scroll frame of Settings: ~90 rectangles, 6000 quad draws, 150 ms
        // at 4K). Far apart ones (a sidebar glide and a preview) stay apart.
        let merged;
        let damage = match damage {
            [_, _, ..] => {
                let all = damage.iter().skip(1).fold(damage[0], |a, r| a.union(r));
                let sum: f32 = damage.iter().map(|r| r.width * r.height).sum();
                merged = [all];
                if sum * 2.0 >= all.width * all.height {
                    &merged[..]
                } else {
                    damage
                }
            }
            _ => damage,
        };

        for &damage_bounds in damage {
            let damage_bounds = damage_bounds * scale_factor;

            // OpenCircle patch (#23): the damage's pixels by the clip mask's
            // rule (`engine::pixel_rect`), set straight to the background.
            let (w, h) = (pixels.width(), pixels.height());
            let [x0, y0, x1, y1] = engine::pixel_rect(damage_bounds, w, h);
            let c = engine::into_color(background_color)
                .premultiply()
                .to_color_u8();
            // (a whole pixel at a time: byte by byte cost ~3 ms a 4K frame)
            let c = u32::from_ne_bytes([c.red(), c.green(), c.blue(), c.alpha()]);
            // OpenCircle patch (#89): not under a picture that covers it
            let base = (self.layers.iter().next())
                .and_then(|layer| self.base(layer, [x0, y0, x1, y1], scale_factor, c));
            let data: &mut [u32] = bytemuck::cast_slice_mut(pixels.data_mut());
            for row in data
                .chunks_exact_mut(w as usize)
                .skip(y0 as usize)
                .take((y1 - y0) as usize)
                .filter(|_| base.is_none())
            {
                row[x0 as usize..x1 as usize].fill(c);
            }

            for (i, layer) in self.layers.iter().enumerate() {
                let base = base.filter(|_| i == 0);
                let Some(layer_bounds) =
                    damage_bounds.intersection(&(layer.bounds * scale_factor))
                else {
                    continue;
                };

                engine::adjust_clip_mask(clip_mask, layer_bounds);

                if !layer.quads.is_empty() && base.is_none() {
                    let render_span = debug::render(debug::Primitive::Quad);
                    for (quad, background) in &layer.quads {
                        self.engine.draw_quad(
                            quad,
                            background,
                            Transformation::scale(scale_factor),
                            pixels,
                            clip_mask,
                            layer_bounds,
                        );
                    }
                    render_span.finish();
                }

                if !layer.primitives.is_empty() && base.is_none() {
                    let render_span = debug::render(debug::Primitive::Triangle);

                    for group in &layer.primitives {
                        let Some(group_bounds) = (group.clip_bounds()
                            * scale_factor)
                            .intersection(&layer_bounds)
                        else {
                            continue;
                        };

                        engine::adjust_clip_mask(clip_mask, group_bounds);

                        for primitive in group.as_slice() {
                            self.engine.draw_primitive(
                                primitive,
                                Transformation::scale(scale_factor)
                                    * group.transformation(),
                                pixels,
                                clip_mask,
                                group_bounds,
                            );
                        }

                        engine::adjust_clip_mask(clip_mask, layer_bounds);
                    }

                    render_span.finish();
                }

                if !layer.images.is_empty() {
                    let render_span = debug::render(debug::Primitive::Image);

                    for (j, image) in layer.images.iter().enumerate() {
                        self.engine.draw_image(
                            image,
                            Transformation::scale(scale_factor),
                            pixels,
                            clip_mask,
                            layer_bounds,
                            base.filter(|_| j == 0),
                        );
                    }

                    render_span.finish();
                }

                if !layer.text.is_empty() {
                    let render_span = debug::render(debug::Primitive::Image);

                    for group in &layer.text {
                        for text in group.as_slice() {
                            self.engine.draw_text(
                                text,
                                Transformation::scale(scale_factor)
                                    * group.transformation(),
                                pixels,
                                clip_mask,
                                layer_bounds,
                            );
                        }
                    }

                    render_span.finish();
                }
            }
        }

        self.engine.trim();
    }

    /// OpenCircle patch (#89): the bottom of the window's picture in the
    /// whole pixels `r` (x0, y0, x1, y1), when `layer` (the first) starts
    /// with a picture covering them: an opaque raster image drawn Nearest,
    /// unturned (the frozen screen). The colour under it then: the
    /// background `clear`, or the one opaque solid rectangle its layer fills
    /// under it (`Overlay`'s zoomed frame: black, the picture faded over it,
    /// which is the dim). The caller skips the clear and what the layer draws
    /// under it (hidden, or that colour), and the picture is drawn over that
    /// colour in one pass. `None`: draw it all as usual.
    fn base(&self, layer: &Layer, r: [u32; 4], k: f32, clear: u32) -> Option<u32> {
        // a clip or a solid fill holds the pixels whose middle it holds
        // (`engine::pixel_rect`), and so does the picture (`raster::nearest`;
        // blitted, it starts at its position truncated: within half a px;
        // a little less, for float error)
        let holds = |b: Rectangle| {
            let [bx0, by0, bx1, by1] = engine::pixel_rect(b, u32::MAX, u32::MAX);
            bx0 <= r[0] && by0 <= r[1] && bx1 >= r[2] && by1 >= r[3]
        };
        let [x0, y0, x1, y1] = r.map(|v| v as f32);
        let covers = |b: Rectangle| {
            b.x <= x0 + 0.45
                && b.y <= y0 + 0.45
                && b.x + b.width > x1 - 0.45
                && b.y + b.height > y1 - 0.45
        };
        let Some(graphics::Image::Raster { image, bounds, .. }) = layer.images.first() else {
            return None;
        };
        let (iw, ih, opaque) = self.engine.raster_pipeline.loaded(&image.handle)?;
        let fits = raster::placed(iw, ih, *bounds, tiny_skia::Transform::from_scale(k, k));
        if !opaque
            || fits.is_none()
            || f32::from(image.rotation) != 0.0
            || image.filter_method != core::image::FilterMethod::Nearest
            || !covers(*bounds * k)
            || !holds(layer.bounds * k)
        {
            return None;
        }
        if image.opacity >= 1.0 {
            return Some(clear); // what is under it is hidden
        }
        let fills: Vec<_> = (layer.primitives.iter())
            .flat_map(|item| item.as_slice().iter().map(move |p| (item, p)))
            .collect();
        match (&layer.quads[..], &fills[..]) {
            ([], []) => Some(clear),
            ([], [(item, Primitive::Fill { path, paint, .. })]) => {
                let at = Transformation::scale(k) * item.transformation();
                let solid = engine::solid_rect(path, paint)?;
                let drawn = item.as_slice()[0].visible_bounds() * at;
                (solid.to_ne_bytes()[3] == 255 && holds(drawn) && holds(item.clip_bounds() * k))
                    .then_some(solid)
            }
            _ => None,
        }
    }
}

impl core::Renderer for Renderer {
    fn start_layer(&mut self, bounds: Rectangle) {
        self.layers.push_clip(bounds);
    }

    fn end_layer(&mut self) {
        self.layers.pop_clip();
    }

    fn start_transformation(&mut self, transformation: Transformation) {
        self.layers.push_transformation(transformation);
    }

    fn end_transformation(&mut self) {
        self.layers.pop_transformation();
    }

    fn fill_quad(
        &mut self,
        quad: renderer::Quad,
        background: impl Into<Background>,
    ) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_quad(quad, background.into(), transformation);
    }

    fn reset(&mut self, new_bounds: Rectangle) {
        self.layers.reset(new_bounds);
    }

    fn allocate_image(
        &mut self,
        _handle: &core::image::Handle,
        callback: impl FnOnce(Result<core::image::Allocation, core::image::Error>)
        + Send
        + 'static,
    ) {
        #[cfg(feature = "image")]
        #[allow(unsafe_code)]
        // TODO: Concurrency
        callback(self.engine.raster_pipeline.load(_handle));

        #[cfg(not(feature = "image"))]
        callback(Err(core::image::Error::Unsupported));
    }
}

impl core::text::Renderer for Renderer {
    type Font = Font;
    type Paragraph = Paragraph;
    type Editor = Editor;

    const ICON_FONT: Font = Font::with_name("Iced-Icons");
    const CHECKMARK_ICON: char = '\u{f00c}';
    const ARROW_DOWN_ICON: char = '\u{e800}';
    const ICED_LOGO: char = '\u{e801}';
    const SCROLL_UP_ICON: char = '\u{e802}';
    const SCROLL_DOWN_ICON: char = '\u{e803}';
    const SCROLL_LEFT_ICON: char = '\u{e804}';
    const SCROLL_RIGHT_ICON: char = '\u{e805}';

    fn default_font(&self) -> Self::Font {
        self.default_font
    }

    fn default_size(&self) -> Pixels {
        self.default_text_size
    }

    fn fill_paragraph(
        &mut self,
        text: &Self::Paragraph,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
    ) {
        let (layer, transformation) = self.layers.current_mut();

        layer.draw_paragraph(
            text,
            position,
            color,
            clip_bounds,
            transformation,
        );
    }

    fn fill_editor(
        &mut self,
        editor: &Self::Editor,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
    ) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_editor(editor, position, color, clip_bounds, transformation);
    }

    fn fill_text(
        &mut self,
        text: core::Text,
        position: Point,
        color: Color,
        clip_bounds: Rectangle,
    ) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_text(text, position, color, clip_bounds, transformation);
    }
}

impl graphics::text::Renderer for Renderer {
    fn fill_raw(&mut self, raw: graphics::text::Raw) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_text_raw(raw, transformation);
    }
}

#[cfg(feature = "geometry")]
impl graphics::geometry::Renderer for Renderer {
    type Geometry = Geometry;
    type Frame = geometry::Frame;

    fn new_frame(&self, bounds: Rectangle) -> Self::Frame {
        geometry::Frame::new(bounds)
    }

    fn draw_geometry(&mut self, geometry: Self::Geometry) {
        let (layer, transformation) = self.layers.current_mut();

        match geometry {
            Geometry::Live {
                primitives,
                images,
                text,
                clip_bounds,
            } => {
                layer.draw_primitive_group(
                    primitives,
                    clip_bounds,
                    transformation,
                );

                for image in images {
                    layer.draw_image(image, transformation);
                }

                layer.draw_text_group(text, clip_bounds, transformation);
            }
            Geometry::Cache(cache) => {
                layer.draw_primitive_cache(
                    cache.primitives,
                    cache.clip_bounds,
                    transformation,
                );

                for image in cache.images.iter() {
                    layer.draw_image(image.clone(), transformation);
                }

                layer.draw_text_cache(
                    cache.text,
                    cache.clip_bounds,
                    transformation,
                );
            }
        }
    }
}

impl graphics::mesh::Renderer for Renderer {
    fn draw_mesh(&mut self, _mesh: graphics::Mesh) {
        log::warn!("iced_tiny_skia does not support drawing meshes");
    }

    fn draw_mesh_cache(&mut self, _cache: iced_graphics::mesh::Cache) {
        log::warn!("iced_tiny_skia does not support drawing meshes");
    }
}

#[cfg(feature = "image")]
impl core::image::Renderer for Renderer {
    type Handle = core::image::Handle;

    fn load_image(
        &self,
        handle: &Self::Handle,
    ) -> Result<core::image::Allocation, core::image::Error> {
        self.engine.raster_pipeline.load(handle)
    }

    fn measure_image(
        &self,
        handle: &Self::Handle,
    ) -> Option<crate::core::Size<u32>> {
        self.engine.raster_pipeline.dimensions(handle)
    }

    fn draw_image(
        &mut self,
        image: core::Image,
        bounds: Rectangle,
        clip_bounds: Rectangle,
    ) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_raster(image, bounds, clip_bounds, transformation);
    }
}

#[cfg(feature = "svg")]
impl core::svg::Renderer for Renderer {
    fn measure_svg(
        &self,
        handle: &core::svg::Handle,
    ) -> crate::core::Size<u32> {
        self.engine.vector_pipeline.viewport_dimensions(handle)
    }

    fn draw_svg(
        &mut self,
        svg: core::Svg,
        bounds: Rectangle,
        clip_bounds: Rectangle,
    ) {
        let (layer, transformation) = self.layers.current_mut();
        layer.draw_svg(svg, bounds, clip_bounds, transformation);
    }
}

impl compositor::Default for Renderer {
    type Compositor = window::Compositor;
}

impl renderer::Headless for Renderer {
    async fn new(
        default_font: Font,
        default_text_size: Pixels,
        backend: Option<&str>,
    ) -> Option<Self> {
        if backend.is_some_and(|backend| {
            !["tiny-skia", "tiny_skia"].contains(&backend)
        }) {
            return None;
        }

        Some(Self::new(default_font, default_text_size))
    }

    fn name(&self) -> String {
        "tiny-skia".to_owned()
    }

    fn screenshot(
        &mut self,
        size: Size<u32>,
        scale_factor: f32,
        background_color: Color,
    ) -> Vec<u8> {
        let viewport = Viewport::with_physical_size(size, scale_factor);

        window::compositor::screenshot(self, &viewport, background_color)
    }
}
