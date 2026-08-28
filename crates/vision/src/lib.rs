//! Pure-Rust image recognition: locates a reference ("needle") image
//! inside a captured screenshot ("haystack") via normalized
//! cross-correlation template matching. No native OpenCV dependency —
//! see docs/architecture.md for why (vcpkg's OpenCV build is a heavy,
//! multi-GB, 30-60 minute setup we're deferring).
//!
//! `find_exact` requires near-perfect correlation at the template's
//! original size — the "完全一致" match mode. `find_similar` scans a
//! range of scales and accepts a caller-supplied threshold, tolerating
//! the resolution/DPI drift that broke Power Automate Desktop's
//! exact-only matching — this is what "AI類似度" currently maps to.
//! It is honestly *not* a trained model yet: it's multiscale
//! correlation matching. ORB feature matching and a real embedding
//! model are still open (see docs/roadmap.md Phase 2).

use image::GrayImage;
use imageproc::template_matching::{find_extremes, match_template, MatchTemplateMethod};

#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Normalized cross-correlation score. 1.0 is a pixel-perfect match.
    pub score: f64,
    pub scale: f64,
}

const EXACT_SCORE_THRESHOLD: f64 = 0.999;

/// `match_template` is brute-force: it slides the needle over every
/// position in the haystack, at a cost roughly proportional to
/// haystack pixels × needle pixels. Fine for the tiny images in this
/// module's tests, but measured at ~80 seconds for a *single* scale
/// against a real 1920×1080 screenshot — long enough to look hung,
/// and long enough that a single blocking call can't be interrupted
/// by the app's Stop button. Below this haystack size, skip the
/// coarse/refine machinery entirely and go straight to the original
/// brute-force search (keeps small-image behavior, and the existing
/// tests' exact assertions, unchanged).
const LARGE_HAYSTACK_PIXELS: u32 = 200_000;

/// How much `locate`'s coarse pass downscales the haystack before
/// searching every scale step — chosen so the coarse pass costs
/// roughly `1 / COARSE_DOWNSCALE^4` of a full-resolution pass (both
/// dimensions of both haystack and effective template shrink).
const COARSE_DOWNSCALE: u32 = 4;

/// Strict match: the needle must appear at its original size with
/// near-perfect correlation. Fails fast on any resolution/DPI drift —
/// this is the "完全一致" mode's limitation, kept deliberately narrow
/// so callers can fall back to `find_similar` themselves.
pub fn find_exact(haystack: &GrayImage, needle: &GrayImage) -> Option<MatchResult> {
    let result = locate(haystack, needle, 1.0, 1.0, 1)?;
    (result.score >= EXACT_SCORE_THRESHOLD).then_some(result)
}

/// Scans `[min_scale, max_scale]` in `steps` increments, returning the
/// best-scoring match at or above `threshold`, or `None` if nothing
/// cleared the bar at any scale.
pub fn find_similar(
    haystack: &GrayImage,
    needle: &GrayImage,
    min_scale: f64,
    max_scale: f64,
    steps: u32,
    threshold: f64,
) -> Option<MatchResult> {
    let steps = steps.max(1);
    locate(haystack, needle, min_scale, max_scale, steps).filter(|m| m.score >= threshold)
}

/// Finds the best-scoring match across `[min_scale, max_scale]` in
/// `steps` increments. On a haystack too small for the brute-force
/// cost to matter, searches directly at full resolution (identical to
/// this module's original, simpler implementation). On a
/// screen-sized haystack, first runs a cheap coarse pass (haystack
/// downscaled by `COARSE_DOWNSCALE`, at every scale step) to pick the
/// best candidate scale and an approximate location, then refines
/// with a *single* full-resolution pass restricted to a small window
/// around that candidate — turning "N full-resolution scans of the
/// whole screen" into "N cheap downscaled scans + one small
/// full-resolution scan".
fn locate(haystack: &GrayImage, needle: &GrayImage, min_scale: f64, max_scale: f64, steps: u32) -> Option<MatchResult> {
    if haystack.width().saturating_mul(haystack.height()) <= LARGE_HAYSTACK_PIXELS {
        return brute_force(haystack, needle, min_scale, max_scale, steps);
    }

    let coarse_haystack = image::imageops::resize(
        haystack,
        (haystack.width() / COARSE_DOWNSCALE).max(1),
        (haystack.height() / COARSE_DOWNSCALE).max(1),
        image::imageops::FilterType::Triangle,
    );

    let mut best: Option<MatchResult> = None;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let scale = min_scale + (max_scale - min_scale) * t;
        let coarse_scale = scale / COARSE_DOWNSCALE as f64;
        if let Some(candidate) = find_at_scale(&coarse_haystack, needle, coarse_scale) {
            if best.as_ref().is_none_or(|b| candidate.score > b.score) {
                // Keep the *real* scale — `candidate.scale` is the
                // coarse-space one used only to size the downscaled
                // template.
                best = Some(MatchResult { scale, ..candidate });
            }
        }
    }
    let coarse = best?;

    let (needle_w, needle_h) = needle.dimensions();
    let full_w = ((needle_w as f64) * coarse.scale).round() as u32;
    let full_h = ((needle_h as f64) * coarse.scale).round() as u32;
    if full_w == 0 || full_h == 0 || full_w > haystack.width() || full_h > haystack.height() {
        return None;
    }

    // The coarse pass's location is only approximate (downscaled
    // localization plus resampling error) — refine within a generous
    // margin around it rather than trusting it exactly.
    let approx_x = coarse.x.saturating_mul(COARSE_DOWNSCALE);
    let approx_y = coarse.y.saturating_mul(COARSE_DOWNSCALE);
    let margin_x = full_w.max(COARSE_DOWNSCALE * 4);
    let margin_y = full_h.max(COARSE_DOWNSCALE * 4);
    let crop_x = approx_x.saturating_sub(margin_x).min(haystack.width().saturating_sub(1));
    let crop_y = approx_y.saturating_sub(margin_y).min(haystack.height().saturating_sub(1));
    let crop_w = (full_w + margin_x * 2).min(haystack.width() - crop_x);
    let crop_h = (full_h + margin_y * 2).min(haystack.height() - crop_y);
    let cropped = image::imageops::crop_imm(haystack, crop_x, crop_y, crop_w, crop_h).to_image();

    let refined = find_at_scale(&cropped, needle, coarse.scale)?;
    Some(MatchResult {
        x: crop_x + refined.x,
        y: crop_y + refined.y,
        width: refined.width,
        height: refined.height,
        score: refined.score,
        scale: refined.scale,
    })
}

fn brute_force(haystack: &GrayImage, needle: &GrayImage, min_scale: f64, max_scale: f64, steps: u32) -> Option<MatchResult> {
    let mut best: Option<MatchResult> = None;

    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let scale = min_scale + (max_scale - min_scale) * t;
        if let Some(candidate) = find_at_scale(haystack, needle, scale) {
            if best.as_ref().is_none_or(|b| candidate.score > b.score) {
                best = Some(candidate);
            }
        }
    }

    best
}

fn find_at_scale(haystack: &GrayImage, needle: &GrayImage, scale: f64) -> Option<MatchResult> {
    let (needle_w, needle_h) = needle.dimensions();
    let scaled_w = ((needle_w as f64) * scale).round() as u32;
    let scaled_h = ((needle_h as f64) * scale).round() as u32;

    if scaled_w == 0 || scaled_h == 0 || scaled_w > haystack.width() || scaled_h > haystack.height() {
        return None;
    }

    let scaled_needle = if (scale - 1.0).abs() < f64::EPSILON {
        needle.clone()
    } else {
        image::imageops::resize(needle, scaled_w, scaled_h, image::imageops::FilterType::Lanczos3)
    };

    let correlation = match_template(haystack, &scaled_needle, MatchTemplateMethod::CrossCorrelationNormalized);
    let extremes = find_extremes(&correlation);

    Some(MatchResult {
        x: extremes.max_value_location.0,
        y: extremes.max_value_location.1,
        width: scaled_w,
        height: scaled_h,
        score: extremes.max_value as f64,
        scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};

    /// A textured patch (diagonal gradient), not a flat colour — a
    /// constant-value needle has zero variance, which makes normalized
    /// cross-correlation degenerate (0/0) and match everywhere.
    fn textured_patch(size: u32) -> GrayImage {
        GrayImage::from_fn(size, size, |x, y| Luma([((x * 37 + y * 91) % 256) as u8]))
    }

    /// A `field_size`x`field_size` white field with `patch` stamped at (x, y).
    fn field_with_patch(field_size: u32, patch: &GrayImage, x: u32, y: u32) -> GrayImage {
        let mut img = GrayImage::from_pixel(field_size, field_size, Luma([255]));
        image::imageops::overlay(&mut img, patch, x as i64, y as i64);
        img
    }

    #[test]
    fn find_exact_locates_the_needle_at_its_true_position() {
        let needle = textured_patch(8);
        let haystack = field_with_patch(40, &needle, 15, 22);

        let result = find_exact(&haystack, &needle).expect("expected an exact match");
        assert_eq!((result.x, result.y), (15, 22));
        assert!(result.score >= EXACT_SCORE_THRESHOLD);
    }

    #[test]
    fn find_exact_returns_none_when_the_needle_is_not_present() {
        let haystack = GrayImage::from_pixel(40, 40, Luma([255]));
        let needle = textured_patch(8);

        assert!(find_exact(&haystack, &needle).is_none());
    }

    #[test]
    fn find_exact_fails_when_only_a_resized_copy_is_present() {
        // Exact match is deliberately strict: a needle present only at
        // a different scale must NOT satisfy find_exact (that's what
        // find_similar is for).
        let needle = textured_patch(8);
        let resized = image::imageops::resize(&needle, 16, 16, image::imageops::FilterType::Lanczos3);
        let haystack = field_with_patch(60, &resized, 10, 10);

        assert!(find_exact(&haystack, &needle).is_none());
    }

    #[test]
    fn find_similar_locates_a_needle_present_at_a_different_scale() {
        let needle = textured_patch(8);
        let resized = image::imageops::resize(&needle, 16, 16, image::imageops::FilterType::Lanczos3);
        let haystack = field_with_patch(60, &resized, 10, 10);

        let result = find_similar(&haystack, &needle, 0.5, 2.5, 16, 0.9).expect("expected a scaled match");
        assert_eq!((result.x, result.y), (10, 10));
        assert!(result.score >= 0.9);
    }

    #[test]
    fn find_similar_respects_the_threshold() {
        let needle = textured_patch(8);
        let haystack = field_with_patch(40, &needle, 15, 22);

        // An impossibly high threshold should reject even a real match.
        assert!(find_similar(&haystack, &needle, 1.0, 1.0, 1, 1.5).is_none());
    }
}
