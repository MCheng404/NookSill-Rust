//! sRGB transfer and premultiplied blending, matched to the GPU path.
//!
//! `Gpu::COLOR_FORMAT` is `Rgba8UnormSrgb`, so egui-wgpu runs
//! `fs_main_linear_framebuffer`: sample and vertex-multiply in gamma space,
//! convert RGB to linear, let the hardware blend there, encode back on store.
//! Doing the arithmetic in gamma space instead would darken every antialiased
//! edge, so the order below is not a stylistic choice.

/// Gamma byte to linear, and linear back to a gamma byte.
pub struct Transfer {
    decode: [f32; 256],
    encode: Vec<u8>,
}

/// Linear steps for the encode table. 16384 keeps the worst rounding error near
/// black under 0.1/255; a 4096 table drifts by a whole byte there.
const ENCODE_STEPS: usize = 16384;

impl Transfer {
    /// Build both tables once; `powf` per pixel would dominate the frame.
    #[must_use]
    pub fn new() -> Self {
        let mut decode = [0.0_f32; 256];
        for (i, slot) in decode.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *slot = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        let mut encode = vec![0_u8; ENCODE_STEPS + 1];
        for (i, slot) in encode.iter_mut().enumerate() {
            let l = i as f32 / ENCODE_STEPS as f32;
            let g = if l <= 0.003_130_8 {
                l * 12.92
            } else {
                1.055 * l.powf(1.0 / 2.4) - 0.055
            };
            *slot = (g * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        Self { decode, encode }
    }

    /// Gamma byte to linear, exact to the table.
    #[must_use]
    pub fn linear(&self, gamma: u8) -> f32 {
        self.decode[gamma as usize]
    }

    /// Linear to gamma byte. Inputs outside 0..=1 clamp, as the GPU does.
    #[must_use]
    pub fn gamma(&self, linear: f32) -> u8 {
        let scaled = linear * ENCODE_STEPS as f32;
        if scaled <= 0.0 {
            return self.encode[0];
        }
        if scaled >= ENCODE_STEPS as f32 {
            return self.encode[ENCODE_STEPS];
        }
        self.encode[scaled.round() as usize]
    }

    /// Premultiplied source-over, blending RGB in linear and alpha in linear
    /// too (alpha carries no transfer curve). `src` is gamma premultiplied.
    ///
    /// The opaque and empty cases skip the tables entirely: most UI pixels are
    /// one or the other, and that is where the frame budget goes.
    pub fn blend(&self, dst: &mut [u8; 4], src: [u8; 4]) {
        if src[3] == 255 {
            *dst = src;
            return;
        }
        if src[3] == 0 {
            return;
        }
        // First touch on a cleared buffer: the linear term drops out and the
        // transfer round-trips, so the tables would only rebuild `src`.
        if *dst == [0, 0, 0, 0] {
            *dst = src;
            return;
        }
        let sa = f32::from(src[3]) / 255.0;
        let keep = 1.0 - sa;
        let da = f32::from(dst[3]) / 255.0;
        for c in 0..3 {
            let s = self.linear(src[c]);
            let d = self.linear(dst[c]);
            dst[c] = self.gamma(s + d * keep);
        }
        dst[3] = ((sa + da * keep) * 255.0).round().clamp(0.0, 255.0) as u8;
    }
}

impl Default for Transfer {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-channel multiply of two gamma premultiplied colors, as the shader does
/// before any transfer conversion.
#[must_use]
pub fn modulate(a: [u8; 4], b: [u8; 4]) -> [u8; 4] {
    // White is the tint on every untextured shape and on most icons.
    if b == [255, 255, 255, 255] {
        return a;
    }
    if a == [255, 255, 255, 255] {
        return b;
    }
    let mut out = [0_u8; 4];
    for c in 0..4 {
        out[c] = ((u32::from(a[c]) * u32::from(b[c]) + 127) / 255) as u8;
    }
    out
}
