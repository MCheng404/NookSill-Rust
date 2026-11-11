//! Offscreen render target with synchronous readback.
//!
//! Windows has no alpha swapchain, so frames render into a texture and are read
//! back for `UpdateLayeredWindow`. 4x MSAA smooths arc segments that egui's 1px
//! feathered edges leave visibly faceted at low DPI.

use egui_wgpu::wgpu;
use std::time::Duration;

/// A hung driver must not freeze the UI thread waiting for a mapped buffer.
pub(crate) const READBACK_TIMEOUT: Duration = Duration::from_millis(200);

/// An offscreen color target plus its readback buffer.
#[derive(Debug)]
pub struct Offscreen {
    /// Resolved 1-sample texture: always the readback source, and the MSAA
    /// resolve target when enabled.
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// MSAA color attachment, present only when `samples > 1`. Write-only.
    msaa: Option<(wgpu::Texture, wgpu::TextureView)>,
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    /// Bytes per row padded to 256, as `copy_texture_to_buffer` requires.
    padded_row: u32,
    samples: u32,
}

impl Offscreen {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        // Resolve target: the render pass uses it as `resolve_target`, so it
        // needs `RENDER_ATTACHMENT` as well as `COPY_SRC`.
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nooksill-offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let msaa = (samples > 1).then(|| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("nooksill-offscreen-msaa"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let v = t.create_view(&wgpu::TextureViewDescriptor::default());
            (t, v)
        });

        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (width * 4).div_ceil(align) * align;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nooksill-readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Self {
            texture,
            view,
            msaa,
            buffer,
            width,
            height,
            padded_row,
            samples,
        }
    }

    /// Color attachment pair `(view, resolve_target)`.
    ///
    /// With MSAA the pass draws into the multisampled texture and the hardware
    /// resolves into `self.texture`; without it both point at the same texture
    /// and there is no resolve. The pipeline's `msaa_samples` must match.
    #[must_use]
    pub fn color_target(&self) -> (&wgpu::TextureView, Option<&wgpu::TextureView>) {
        match &self.msaa {
            Some((_, v)) => (v, Some(&self.view)),
            None => (&self.view, None),
        }
    }

    #[must_use]
    pub const fn samples(&self) -> u32 {
        self.samples
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Copy the texture into the readback buffer. Must run before the command
    /// buffer is submitted.
    ///
    /// Reads the resolved `self.texture`: the resolve already happened in the
    /// render pass.
    pub fn copy_to_buffer(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Synchronous readback into tightly packed premultiplied RGBA.
    /// Blocking 1-2 ms keeps the pipeline simple; revisit for continuous 60 fps.
    pub fn read_pixels(&self, device: &wgpu::Device) -> Result<Vec<u8>, String> {
        self.with_mapped_pixels(device, |data, stride| {
            unpad_rows(data, self.width, self.height, stride)
        })
    }

    /// Map the readback buffer and run `f` with the padded bytes and row stride.
    /// Unmaps on every path, including `f` returning an error. The present path
    /// uses this to write straight into the DIB without a full-frame copy.
    pub fn with_mapped_pixels<T>(
        &self,
        device: &wgpu::Device,
        f: impl FnOnce(&[u8], u32) -> Result<T, String>,
    ) -> Result<T, String> {
        let slice = self.buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        // Wait returns only after callbacks ran, so try_recv cannot hang.
        // unmap() cancels a pending map_async, so every path must reach it.
        let result = (|| {
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .map_err(|e| format!("poll 失败: {e}"))?;
            match rx.try_recv() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(format!("map_async 失败: {e}")),
                Err(e) => return Err(format!("回读回调未投递: {e}")),
            }
            let data = slice
                .get_mapped_range()
                .map_err(|e| format!("映射回读缓冲失败: {e}"))?;
            let out = f(&data[..], self.padded_row);
            drop(data);
            out
        })();
        self.buffer.unmap();
        result
    }
}

/// Remove the 256-byte row padding added by `copy_texture_to_buffer`.
/// Kept as a pure function so the copy can be tested without a GPU.
fn unpad_rows(data: &[u8], width: u32, height: u32, padded_row: u32) -> Result<Vec<u8>, String> {
    let row_bytes = width as usize * 4;
    let stride = padded_row as usize;
    if stride < row_bytes {
        return Err(format!("行距 {stride} 小于行宽 {row_bytes}"));
    }
    let rows = height as usize;
    if rows == 0 {
        return Ok(Vec::new());
    }
    let needed = stride
        .checked_mul(rows - 1)
        .and_then(|n| n.checked_add(row_bytes))
        .ok_or_else(|| "回读尺寸溢出".to_owned())?;
    if data.len() < needed {
        return Err(format!("回读缓冲过短：{} < {needed}", data.len()));
    }

    let mut out = Vec::with_capacity(row_bytes * rows);
    for row in 0..rows {
        let start = row * stride;
        out.extend_from_slice(&data[start..start + row_bytes]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::unpad_rows;

    #[test]
    fn unpad_rows_removes_alignment_padding() {
        // 1 px wide = 4 valid bytes, stride padded to 8.
        let data = [1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8, 0, 0, 0, 0];
        assert_eq!(
            unpad_rows(&data, 1, 2, 8).unwrap(),
            vec![1, 2, 3, 4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn unpad_rows_handles_already_aligned_rows() {
        // 64 px = 256 bytes, already aligned; no byte may be dropped.
        let data = vec![0xABu8; 256 * 2];
        let out = unpad_rows(&data, 64, 2, 256).unwrap();
        assert_eq!(out.len(), 256 * 2);
        assert!(out.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn unpad_rows_rejects_bad_geometry() {
        assert!(unpad_rows(&[0; 8], 4, 2, 4).is_err(), "行距小于行宽");
        assert!(unpad_rows(&[0; 8], 1, 3, 8).is_err(), "缓冲比声明的高度短");
    }

    #[test]
    fn unpad_rows_zero_height_is_empty() {
        assert!(unpad_rows(&[], 0, 0, 256).unwrap().is_empty());
        assert!(unpad_rows(&[], 10, 0, 256).unwrap().is_empty());
    }
}
