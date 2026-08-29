//! CNN embedding-based similarity — the "AI" match tier. Loads a small
//! pretrained MobileNetV2 (ImageNet, ONNX, Apache-2.0, from the ONNX
//! Model Zoo) via `candle-onnx`, a pure-Rust ONNX interpreter, so this
//! stays consistent with the rest of this crate's "no heavy native
//! dependency" policy (see the module doc in `lib.rs`) — no
//! `onnxruntime.dll` to bundle in the installer.
//!
//! The model's final classifier output (1000 ImageNet class logits,
//! softmaxed) is used directly as the image's descriptor vector. This
//! deliberately skips slicing the graph at an internal pooling layer
//! to get a "proper" embedding — comparing softmax probability
//! distributions between two crops is a simpler, well-understood
//! proxy for "are these semantically the same thing" and avoids any
//! graph surgery in candle-onnx, which is the highest-risk dependency
//! in this feature.

use candle_core::{DType, Device, Tensor};
use image::{imageops::FilterType, RgbImage};
use std::sync::OnceLock;

const MODEL_BYTES: &[u8] = include_bytes!("../assets/mobilenetv2.onnx");
const MODEL_INPUT_SIZE: u32 = 224;
const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];

fn model() -> &'static candle_onnx::onnx::ModelProto {
    static MODEL: OnceLock<candle_onnx::onnx::ModelProto> = OnceLock::new();
    MODEL.get_or_init(|| {
        use prost::Message as _;
        let mut model = candle_onnx::onnx::ModelProto::decode(MODEL_BYTES).expect("bundled mobilenetv2.onnx is a valid ONNX model");
        patch_global_average_pool(&mut model);
        model
    })
}

/// `candle-onnx` 0.11 doesn't implement the `GlobalAveragePool` op
/// (confirmed via a spike test against this exact model — it fails
/// with "unsupported op_type GlobalAveragePool"), but every
/// ImageNet-classifier CNN ends its feature extractor with one before
/// the final fully-connected layer, so skipping models that use it
/// isn't a realistic option. `GlobalAveragePool` is mathematically
/// identical to `ReduceMean` over the spatial (H, W) axes with
/// `keepdims=1` — both existing ONNX ops that ARE implemented — so
/// rewrite the node in place rather than pull in a different runtime.
fn patch_global_average_pool(model: &mut candle_onnx::onnx::ModelProto) {
    use candle_onnx::onnx::attribute_proto::AttributeType;
    use candle_onnx::onnx::AttributeProto;

    let Some(graph) = model.graph.as_mut() else { return };
    for node in graph.node.iter_mut() {
        if node.op_type != "GlobalAveragePool" {
            continue;
        }
        node.op_type = "ReduceMean".to_string();
        node.attribute = vec![
            AttributeProto {
                name: "axes".to_string(),
                r#type: AttributeType::Ints as i32,
                ints: vec![2, 3],
                ..Default::default()
            },
            AttributeProto {
                name: "keepdims".to_string(),
                r#type: AttributeType::Int as i32,
                i: 1,
                ..Default::default()
            },
        ];
    }
}

fn input_name() -> &'static str {
    &model().graph.as_ref().expect("model has a graph").input[0].name
}

fn output_name() -> &'static str {
    &model().graph.as_ref().expect("model has a graph").output[0].name
}

/// Computes a 1000-dim softmaxed descriptor vector for an RGB image.
/// Returns `None` if inference fails (malformed input, graph op
/// candle-onnx doesn't support, etc.) — callers should skip that
/// candidate rather than fail the whole match.
pub fn embed(img: &RgbImage) -> Option<Vec<f32>> {
    let resized = image::imageops::resize(img, MODEL_INPUT_SIZE, MODEL_INPUT_SIZE, FilterType::Triangle);
    let device = Device::Cpu;

    let mut chw = vec![0f32; 3 * MODEL_INPUT_SIZE as usize * MODEL_INPUT_SIZE as usize];
    let plane = (MODEL_INPUT_SIZE * MODEL_INPUT_SIZE) as usize;
    for (x, y, pixel) in resized.enumerate_pixels() {
        let idx = (y * MODEL_INPUT_SIZE + x) as usize;
        for c in 0..3 {
            let v = pixel[c] as f32 / 255.0;
            chw[c * plane + idx] = (v - IMAGENET_MEAN[c]) / IMAGENET_STD[c];
        }
    }

    let input = Tensor::from_vec(chw, (1, 3, MODEL_INPUT_SIZE as usize, MODEL_INPUT_SIZE as usize), &device)
        .ok()?
        .to_dtype(DType::F32)
        .ok()?;

    let mut inputs = std::collections::HashMap::new();
    inputs.insert(input_name().to_string(), input);

    let outputs = candle_onnx::simple_eval(model(), inputs).ok()?;
    let logits = outputs.get(output_name())?;
    let probs = candle_nn_softmax(logits).ok()?;
    probs.flatten_all().ok()?.to_vec1::<f32>().ok()
}

fn candle_nn_softmax(t: &Tensor) -> candle_core::Result<Tensor> {
    let max = t.max_keepdim(candle_core::D::Minus1)?;
    let diff = t.broadcast_sub(&max)?;
    let exp = diff.exp()?;
    let sum = exp.sum_keepdim(candle_core::D::Minus1)?;
    exp.broadcast_div(&sum)
}

/// Cosine similarity of two equal-length vectors, in `[-1.0, 1.0]`
/// (softmax outputs are non-negative, so in practice `[0.0, 1.0]`).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b).map(|(x, y)| *x as f64 * *y as f64).sum();
    let norm_a: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_of_identical_vectors_is_one() {
        let v = vec![0.1, 0.2, 0.3, 0.4];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_similarity_of_orthogonal_vectors_is_zero() {
        assert!((cosine_similarity(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-9);
    }

    /// Spike test: confirms the bundled ONNX model actually loads via
    /// candle-onnx and produces a well-formed embedding for a trivial
    /// solid-color image. This is the single highest-risk step in the
    /// whole AI match mode — candle-onnx's op coverage is narrower
    /// than full ONNX Runtime, so this must pass before anything else
    /// in this module is trusted.
    #[test]
    fn embed_runs_the_bundled_model_and_returns_a_1000_dim_probability_vector() {
        let img = RgbImage::from_pixel(300, 200, image::Rgb([120, 80, 200]));
        let v = embed(&img).expect("embedding should succeed on the bundled model");
        assert_eq!(v.len(), 1000);
        let sum: f32 = v.iter().sum();
        assert!((sum - 1.0).abs() < 0.01, "softmax output should sum to ~1.0, got {sum}");
    }

    #[test]
    fn embed_of_identical_images_is_more_similar_than_of_different_colors() {
        let a = RgbImage::from_pixel(300, 200, image::Rgb([200, 30, 30]));
        let a2 = RgbImage::from_pixel(300, 200, image::Rgb([200, 30, 30]));
        let b = RgbImage::from_pixel(300, 200, image::Rgb([30, 30, 200]));

        let ea = embed(&a).unwrap();
        let ea2 = embed(&a2).unwrap();
        let eb = embed(&b).unwrap();

        let same = cosine_similarity(&ea, &ea2);
        let different = cosine_similarity(&ea, &eb);
        assert!(same > different, "identical images ({same}) should be more similar than different-colored ones ({different})");
    }
}
