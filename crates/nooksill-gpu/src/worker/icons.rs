//! Icon worker thread: fetching, fitting and grouping icon bitmaps.

use super::*;

pub(super) fn icon_thread(
    rx: Receiver<IconRequest>,
    tx: Sender<ReadyIcon>,
    waker: Waker,
    shader: Option<Arc<IconShader>>,
) {
    // The shell COM objects are used here, so COM must be initialised on this
    // thread; `ensure_com` runs once per thread.
    ensure_com();

    loop {
        // Batch before processing: startup queues N requests at once.
        let Ok(first) = rx.recv() else { return };
        let mut batch = vec![first];
        while batch.len() < BATCH {
            match rx.try_recv() {
                Ok(next) => batch.push(next),
                Err(_) => break,
            }
        }

        // Serial: `GetIcon` hands out objects from a shared image list, which
        // is not safe to touch concurrently.
        let fetched: Vec<(IconRequest, Option<IconBitmap>)> = batch
            .into_iter()
            .map(|req| {
                let bmp = fetch_icon(&req);
                (req, bmp)
            })
            .collect();

        // Parallel: scaling and rounded masking are pure CPU, ~65k pixels per
        // 256x256 icon. This is the one place worth extra cores.
        let ready: Vec<ReadyIcon> = fetched
            .into_par_iter()
            .filter_map(|(req, bmp)| bmp.map(|b| process(&req, b, shader.as_deref())))
            .collect();

        let produced = !ready.is_empty();
        for icon in ready {
            if tx.send(icon).is_err() {
                return; // Receiver gone: the UI thread is dead.
            }
        }
        if produced {
            waker.wake();
        }
    }
}

/// Max batch size; 8 fills common core counts without delaying the first batch.
pub(super) const BATCH: usize = 8;

/// Fetches an icon through the shell.
pub(super) fn fetch_icon(req: &IconRequest) -> Option<IconBitmap> {
    // A custom icon wins: `.ico` goes through the shell image list. Falling
    // back to the item's own icon beats showing nothing on a bad path.
    let custom = (!req.icon_src.is_empty())
        .then(|| icon::shell_icon(&req.icon_src))
        .flatten();
    if custom.is_some() {
        return custom;
    }
    match req.kind {
        // Separators draw no icon; URL icons would need a network fetch.
        ItemKind::Separator | ItemKind::Url => None,
        ItemKind::Trash => icon::shell_icon("shell:RecycleBinFolder"),
        // A group has no icon of its own; compose one from the children.
        ItemKind::Group => group_icon(&req.kids),
        _ => icon::shell_icon(&req.path).or_else(|| icon::resource_icon(&req.path, 0)),
    }
}

/// Runs the CPU half: pick the fill mode and apply the rounded mask if needed.
pub(super) fn process(
    req: &IconRequest,
    bmp: IconBitmap,
    shader: Option<&IconShader>,
) -> ReadyIcon {
    // Group children are already rounded and full-bleed from compositing;
    // running `fit_icon` again would re-crop and blur them.
    let fit = if req.kind == ItemKind::Group {
        IconFit {
            bbox: nooksill_core::icon::BBox::full(bmp.width, bmp.height),
            rounded: None,
            fill: 1.0,
        }
    } else {
        fit_icon(
            &bmp.rgba,
            bmp.width,
            bmp.height,
            FitMode::from_code(req.fit),
        )
    };
    let mut rgba = bmp.rgba;
    if let Some(r) = fit.rounded {
        let bbox = (fit.bbox.x0, fit.bbox.y0, fit.bbox.x1, fit.bbox.y1);
        let masked = shader.and_then(|s| s.round_mask(&rgba, bmp.width, bmp.height, bbox, r));
        match masked {
            Some(out) => rgba = out,
            None => apply_round_mask(&mut rgba, bmp.width, fit.bbox, r),
        }
    }
    ReadyIcon {
        index: req.index,
        path: req.path.clone(),
        width: bmp.width,
        height: bmp.height,
        rgba,
        fit,
    }
}

/// Composes a 2x2 grid from the group's children.
///
/// A group has no path, so `shell_icon` yields nothing and each child must be
/// fetched on its own.
pub(super) fn group_icon(kids: &[IconKid]) -> Option<IconBitmap> {
    // Collect bitmaps first so they outlive the borrow of their pixels.
    let bitmaps: Vec<IconBitmap> = kids.iter().take(4).filter_map(fetch_kid).collect();
    if bitmaps.is_empty() {
        return None;
    }
    let refs: Vec<nooksill_core::icon::QuadSource<'_>> = bitmaps
        .iter()
        .map(|b| nooksill_core::icon::QuadSource {
            rgba: &b.rgba,
            width: b.width,
            height: b.height,
        })
        .collect();
    let out = 256u32;
    let rgba = nooksill_core::icon::composite_quad(&refs, out, out as f32 * 0.18);
    Some(IconBitmap {
        width: out,
        height: out,
        rgba,
    })
}

/// Fetches one child's icon, preferring its custom icon.
pub(super) fn fetch_kid(kid: &IconKid) -> Option<IconBitmap> {
    let custom = (!kid.icon_src.is_empty())
        .then(|| icon::shell_icon(&kid.icon_src))
        .flatten();
    if custom.is_some() {
        return custom;
    }
    match kid.kind {
        ItemKind::Separator | ItemKind::Url | ItemKind::Group => None,
        ItemKind::Trash => icon::shell_icon("shell:RecycleBinFolder"),
        _ => icon::shell_icon(&kid.path).or_else(|| icon::resource_icon(&kid.path, 0)),
    }
}
