use alloc::vec::Vec;
use jpeg_encoder::{ColorType, Encoder};
use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;
use zune_png::PngDecoder;

use crate::Failure;

struct Pixels {
    width: usize,
    height: usize,
    colors: ColorSpace,
    bytes: Vec<u8>,
}

fn invalid() -> Failure {
    Failure::new(
        "INVALID_INPUT",
        "SeedEdit 底图须为可解码的 JPEG 或 PNG，并满足图片预算",
    )
}

impl Pixels {
    fn decode(source: &[u8]) -> Result<Self, Failure> {
        let limits = DecoderOptions::new_safe()
            .set_max_width(4096)
            .set_max_height(4096);
        let bytes = ZCursor::new(source);
        let (width, height, colors, pixels) = if source.starts_with(b"\x89PNG\r\n\x1a\n") {
            let mut decoder =
                PngDecoder::new_with_options(bytes, limits.png_set_strip_to_8bit(true));
            let pixels = decoder.decode_raw().map_err(|_| invalid())?;
            let (width, height) = decoder.dimensions().ok_or_else(invalid)?;
            let colors = decoder.colorspace().ok_or_else(invalid)?;
            (width, height, colors, pixels)
        } else {
            let options = limits.jpeg_set_out_colorspace(ColorSpace::RGB);
            let mut decoder = JpegDecoder::new_with_options(bytes, options);
            let pixels = decoder.decode().map_err(|_| invalid())?;
            let facts = decoder.info().ok_or_else(invalid)?;
            let colors = decoder.output_colorspace().ok_or_else(invalid)?;
            (
                usize::from(facts.width),
                usize::from(facts.height),
                colors,
                pixels,
            )
        };
        if !matches!(
            colors,
            ColorSpace::RGB | ColorSpace::RGBA | ColorSpace::Luma | ColorSpace::LumaA
        ) || pixels.len() != width * height * colors.num_components()
            || width.min(height) == 0
            || width.max(height) > width.min(height) * 3
        {
            return Err(invalid());
        }
        Ok(Self {
            width,
            height,
            colors,
            bytes: pixels,
        })
    }

    fn rgb_on_white(&self, x: usize, y: usize) -> [u8; 3] {
        let channels = self.colors.num_components();
        let index = (y * self.width + x) * channels;
        let pixel = &self.bytes[index..index + channels];
        let [r, g, b, alpha] = match pixel {
            [gray] => [*gray, *gray, *gray, 255],
            [gray, alpha] => [*gray, *gray, *gray, *alpha],
            [r, g, b] => [*r, *g, *b, 255],
            [r, g, b, alpha] => [*r, *g, *b, *alpha],
            _ => unreachable!("decoded channels are checked"),
        };
        let alpha = u32::from(alpha);
        [r, g, b].map(|channel| ((u32::from(channel) * alpha + 255 * (255 - alpha)) / 255) as u8)
    }

    fn scale_to(&self, width: usize, height: usize) -> Vec<u8> {
        let mut target = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            let top = y * self.height / height;
            let bottom = (y + 1) * self.height / height;
            for x in 0..width {
                let left = x * self.width / width;
                let right = (x + 1) * self.width / width;
                let count = ((right - left) * (bottom - top)) as u32;
                let mut sum = [0u32; 3];
                for source_y in top..bottom {
                    for source_x in left..right {
                        let [r, g, b] = self.rgb_on_white(source_x, source_y);
                        sum[0] += u32::from(r);
                        sum[1] += u32::from(g);
                        sum[2] += u32::from(b);
                    }
                }
                target.extend(sum.map(|channel| ((channel + count / 2) / count) as u8));
            }
        }
        target
    }
}

pub(crate) fn normalized_jpeg(source: &[u8]) -> Result<Vec<u8>, Failure> {
    let pixels = Pixels::decode(source)?;
    let long = pixels.width.max(pixels.height);
    for edge in [1536, 1280, 1024, 896] {
        let limit = edge.min(long);
        let width = (pixels.width * limit + long / 2) / long;
        let height = (pixels.height * limit + long / 2) / long;
        let rgb = pixels.scale_to(width, height);
        for quality in [88, 78, 68, 58] {
            let mut output = Vec::new();
            Encoder::new(&mut output, quality)
                .encode(&rgb, width as u16, height as u16, ColorType::Rgb)
                .map_err(|_| invalid())?;
            if output.len() <= 4 * 1024 * 1024 {
                return Ok(output);
            }
        }
    }
    Err(invalid())
}
