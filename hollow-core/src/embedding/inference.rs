use crate::HollowError;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;

pub struct EmbeddingModel {
    session: Session,
    tokenizer: tokenizers::Tokenizer,
    dimensions: usize,
}

impl EmbeddingModel {
    pub fn load(model_path: &Path, tokenizer_path: &Path) -> Result<Self, HollowError> {
        let session = Session::builder()
            .map_err(|e| HollowError::InvalidInput(format!("ONNX session builder: {}", e)))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| HollowError::InvalidInput(format!("ONNX opt level: {}", e)))?
            .commit_from_file(model_path)
            .map_err(|e| HollowError::InvalidInput(format!("ONNX load model: {}", e)))?;

        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path)
            .map_err(|e| HollowError::InvalidInput(format!("Load tokenizer: {}", e)))?;

        Ok(Self {
            session,
            tokenizer,
            dimensions: 1024, // Qwen3 output dim
        })
    }

    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>, HollowError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| HollowError::InvalidInput(format!("Tokenize: {}", e)))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let seq_len = input_ids.len();

        let input_ids_tensor = Tensor::from_array(([1, seq_len], input_ids))
            .map_err(|e| HollowError::InvalidInput(format!("input_ids tensor: {}", e)))?;
        let attention_mask_tensor = Tensor::from_array(([1, seq_len], attention_mask))
            .map_err(|e| HollowError::InvalidInput(format!("attention_mask tensor: {}", e)))?;

        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
            ])
            .map_err(|e| HollowError::InvalidInput(format!("ONNX run: {}", e)))?;

        let output = &outputs[0];
        let (shape, data) = output
            .try_extract_tensor::<f32>()
            .map_err(|e| HollowError::InvalidInput(format!("Extract tensor: {}", e)))?;

        let embedding = if shape.len() == 3 {
            // [1, seq_len, dim] — mean pooling
            let seq = shape[1] as usize;
            let dim = shape[2] as usize;
            let mut pooled = vec![0.0_f32; dim];
            for s in 0..seq {
                for d in 0..dim {
                    pooled[d] += data[s * dim + d];
                }
            }
            for d in 0..dim {
                pooled[d] /= seq as f32;
            }
            pooled
        } else if shape.len() == 2 {
            // [1, dim] — already pooled
            data.to_vec()
        } else {
            return Err(HollowError::InvalidInput(format!(
                "Unexpected output shape: {:?}",
                &*shape
            )));
        };

        Ok(l2_normalize(&embedding))
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }
}

fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < 1e-10 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_l2_normalize() {
        let v = vec![3.0, 4.0];
        let n = l2_normalize(&v);
        assert!((n[0] - 0.6).abs() < 1e-6);
        assert!((n[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_l2_normalize_zero_vector() {
        let v = vec![0.0, 0.0, 0.0];
        let n = l2_normalize(&v);
        assert!(n.iter().all(|x| *x == 0.0));
    }
}
