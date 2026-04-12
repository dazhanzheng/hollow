use crate::HollowError;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum ModelVariant {
    Qwen3Small, // 0.6B INT8
    Qwen3Large, // 4B INT8
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub variant: ModelVariant,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub download_size_mb: u64,
    pub ram_usage_mb: u64,
    pub dimensions: u32,
}

pub struct ModelManager {
    models_dir: PathBuf,
}

impl ModelManager {
    pub fn new(models_dir: PathBuf) -> Self {
        Self { models_dir }
    }

    pub fn available_models() -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                variant: ModelVariant::Qwen3Small,
                name: "qwen3-embedding-0.6b-int8".to_string(),
                display_name: "Qwen3 Embedding 0.6B (INT8)".to_string(),
                description: "Recommended. Good quality for Chinese + English, runs on any Mac."
                    .to_string(),
                download_size_mb: 624,
                ram_usage_mb: 400,
                dimensions: 1024,
            },
            ModelInfo {
                variant: ModelVariant::Qwen3Large,
                name: "qwen3-embedding-4b-int8".to_string(),
                display_name: "Qwen3 Embedding 4B".to_string(),
                description:
                    "Higher accuracy. Requires 32 GB+ RAM. Coming soon."
                        .to_string(),
                download_size_mb: 8000,
                ram_usage_mb: 5000,
                dimensions: 1024,
            },
        ]
    }

    pub fn is_downloaded(&self, variant: &ModelVariant) -> bool {
        let model_dir = self.model_dir(variant);
        model_dir.join("model.onnx").exists() && model_dir.join("tokenizer.json").exists()
    }

    pub fn model_dir(&self, variant: &ModelVariant) -> PathBuf {
        let name = match variant {
            ModelVariant::Qwen3Small => "qwen3-embedding-0.6b-int8",
            ModelVariant::Qwen3Large => "qwen3-embedding-4b-int8",
        };
        self.models_dir.join(name)
    }

    pub fn model_path(&self, variant: &ModelVariant) -> PathBuf {
        self.model_dir(variant).join("model.onnx")
    }

    pub fn tokenizer_path(&self, variant: &ModelVariant) -> PathBuf {
        self.model_dir(variant).join("tokenizer.json")
    }

    pub fn delete_model(&self, variant: &ModelVariant) -> Result<(), HollowError> {
        let dir = self.model_dir(variant);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| {
                HollowError::InvalidInput(format!("Failed to delete model: {}", e))
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_available_models_returns_two() {
        let models = ModelManager::available_models();
        assert_eq!(models.len(), 2);
    }

    #[test]
    fn test_is_downloaded_returns_false_when_missing() {
        let manager = ModelManager::new(PathBuf::from("/nonexistent/path"));
        assert!(!manager.is_downloaded(&ModelVariant::Qwen3Small));
        assert!(!manager.is_downloaded(&ModelVariant::Qwen3Large));
    }

    #[test]
    fn test_model_paths_are_correct() {
        let manager = ModelManager::new(PathBuf::from("/models"));
        assert_eq!(
            manager.model_path(&ModelVariant::Qwen3Small),
            PathBuf::from("/models/qwen3-embedding-0.6b-int8/model.onnx")
        );
        assert_eq!(
            manager.tokenizer_path(&ModelVariant::Qwen3Small),
            PathBuf::from("/models/qwen3-embedding-0.6b-int8/tokenizer.json")
        );
        assert_eq!(
            manager.model_path(&ModelVariant::Qwen3Large),
            PathBuf::from("/models/qwen3-embedding-4b-int8/model.onnx")
        );
        assert_eq!(
            manager.tokenizer_path(&ModelVariant::Qwen3Large),
            PathBuf::from("/models/qwen3-embedding-4b-int8/tokenizer.json")
        );
    }
}
