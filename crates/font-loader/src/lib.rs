use std::fs::{self, File};
use std::io;
use std::path::PathBuf;

use image::{DynamicImage, ImageFormat, Rgba};
use log::{error, info};
use rusttype::{point, Font, Scale};
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::{Canvas, Texture, TextureCreator};
use sdl2::surface::Surface;
use sdl2::video::{Window, WindowContext};

const PRINTABLE_ASCII_CHAR_OFFSET: u8 = 0x20;

/// Render ttf fonts to font sprite aliases.
pub struct FontRenderer {
    fonts_path: PathBuf,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("io error:")]
    Io(#[from] io::Error),

    #[error("no font data")]
    NoFontData,

    #[error("image error: {0:?}")]
    Image(#[from] image::ImageError),
}

impl FontRenderer {
    pub fn new(fonts_path: PathBuf) -> Result<Self, Error> {
        Ok(Self { fonts_path })
    }

    /// Render a font to a png file, return the width of the font.
    pub fn render_font_to_file(
        &self,
        font_family: &str,
        size: f32,
        color: (u8, u8, u8),
        filename: &str,
    ) -> Result<(u32, u32), Error> {
        let font_path = self.fonts_path.join(font_family);
        let data = fs::read(&font_path)?;
        let font = Font::try_from_vec(data).ok_or(Error::NoFontData)?;

        // The font size to use
        let scale = Scale::uniform(size);

        // All ascii characters to render.
        let text = (b'!'..=b'~').map(char::from).collect::<String>();

        let v_metrics = font.v_metrics(scale);

        let scale = Scale::uniform(size);
        let glyphs: Vec<_> = font
            .layout(&text, scale, point(0.0, 0.0 + v_metrics.ascent))
            .collect();

        // work out the layout size
        let glyphs_height = (v_metrics.ascent - v_metrics.descent).ceil() as u32;

        let max_width = glyphs.iter().fold(0u32, |acc, item| {
            (item.pixel_bounding_box().unwrap().width() as u32).max(acc)
        });

        // Create a new rgba image with some padding
        let mut image =
            DynamicImage::new_rgba8(max_width * glyphs.len() as u32, glyphs_height + 40).to_rgba8();

        for (num, glyph) in glyphs.iter().enumerate() {
            if let Some(bounding_box) = glyph.pixel_bounding_box() {
                // Draw the glyph into the image per-pixel by using the draw closure
                glyph.draw(|x, y, v| {
                    image.put_pixel(
                        // Offset the position by the glyph bounding box
                        x + (num as u32 * max_width),
                        y + bounding_box.min.y as u32,
                        // Turn the coverage into an alpha value
                        Rgba([color.0, color.1, color.2, (v * 255.0) as u8]),
                    )
                });
            }
        }

        // Save the image to a png file
        image.save_with_format(&filename, ImageFormat::Png)?;
        info!("Generated: {filename} width: {max_width}, height: {glyphs_height}");
        Ok((max_width, glyphs_height))
    }
}

/// Sprite-based font (fixed width) based on a pre-rendered atlas.
pub struct SpriteFont<'a> {
    texture: Texture<'a>,
    glyph_width: u32,
    glyph_height: u32,
}

impl<'r, 'a> SpriteFont<'r>
where
    'a: 'r,
{
    /// Load a font from known dimensions at a given path.
    pub fn load_from_png_with_dimensions(
        font_path: &str,
        glyph_width: u32,
        glyph_height: u32,
        texture_creator: &'r TextureCreator<WindowContext>,
    ) -> Result<Self, anyhow::Error> {
        let image_data =
            image::load(io::BufReader::new(File::open(font_path)?), ImageFormat::Png)?.into_rgba8();

        info!(
            "image dimensions {}, {}",
            image_data.width(),
            image_data.height()
        );
        let surface = Surface::new(
            image_data.width(),
            image_data.height(),
            PixelFormatEnum::ABGR32,
        )
        .map_err(|err_string| anyhow::anyhow!(err_string))?;
        let surface_rect = surface.rect();
        let mut texture = texture_creator.create_texture_from_surface(surface)?;
        texture.update(
            Some(surface_rect),
            &image_data,
            (32 / 8) * image_data.width() as usize,
        )?;
        Ok(Self {
            texture,
            glyph_height,
            glyph_width,
        })
    }

    /// Print a string to a canvas at the given location.
    pub fn print_string(
        &self,
        x: i32,
        y: i32,
        canvas: &mut Canvas<Window>,
        s: &str,
    ) -> Result<(), anyhow::Error> {
        let font_width = self.glyph_width;
        let font_height = self.glyph_height;
        for (i, c) in s.as_bytes().iter().enumerate() {
            if *c == 0 || *c <= PRINTABLE_ASCII_CHAR_OFFSET || *c > b'~' {
                if *c != b' ' {
                    error!("Tried to print non-printable char code {}", *c);
                }
                continue;
            }
            let charcode = c - PRINTABLE_ASCII_CHAR_OFFSET - 1;
            let src = Rect::new(
                charcode as i32 * font_width as i32,
                0,
                font_width,
                font_height,
            );
            let dest = Rect::new(x + i as i32 * font_width as i32, y, font_width, font_height);
            canvas.set_draw_color(Color::RGB(0, 255, 0));
            canvas
                .copy(&self.texture, Some(src), Some(dest))
                .map_err(|err_string| anyhow::anyhow!(err_string))?;
        }
        Ok(())
    }
}
